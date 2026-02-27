//! IngestTelemetry gRPC handler.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use proto::supervisor_service::{
    supervisor_service_server::SupervisorService, IngestResult, IngestTelemetryRequest,
    IngestTelemetryResponse, ItemResult, Severity, StatusChange, TelemetryEnvelope,
};
use sqlx::{PgPool, Row};
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::telemetry_sink::{TelemetryPoint, TelemetrySink};
use crate::threshold::{self, MetricThreshold, Severity as ThreshSeverity};

// ------------------------------------------------------------------ //
//  gRPC service implementation                                        //
// ------------------------------------------------------------------ //

pub struct SupervisorServiceImpl {
    pub pool: PgPool,
    pub sink: Arc<dyn TelemetrySink>,
    pub amqp_chan: Option<lapin::Channel>,
}

impl SupervisorServiceImpl {
    pub fn new(
        pool: PgPool,
        sink: Arc<dyn TelemetrySink>,
        amqp_chan: Option<lapin::Channel>,
    ) -> Self {
        Self {
            pool,
            sink,
            amqp_chan,
        }
    }
}

// ------------------------------------------------------------------ //
//  Ingest logic                                                       //
// ------------------------------------------------------------------ //

async fn process_envelope(
    envelope: &TelemetryEnvelope,
    pool: &PgPool,
    sink: &dyn TelemetrySink,
    amqp_chan: Option<&lapin::Channel>,
) -> Result<(IngestResult, Option<StatusChange>)> {
    let plant_id = match Uuid::parse_str(&envelope.plant_id) {
        Ok(id) => id,
        Err(_) => return Ok((IngestResult::Error, None)),
    };

    // Deduplication check
    let existing: Option<String> =
        sqlx::query_scalar("SELECT result FROM telemetry_ingest_ledger WHERE ingest_id = $1")
            .bind(&envelope.ingest_id)
            .fetch_optional(pool)
            .await?;

    if existing.is_some() {
        let _ = sqlx::query("UPDATE device SET last_seen_at = NOW() WHERE device_uid = $1")
            .bind(&envelope.device_uid)
            .execute(pool)
            .await;
        return Ok((IngestResult::Duplicate, None));
    }

    // Plant lookup
    let plant_row =
        sqlx::query("SELECT id, plant_type_id FROM plant WHERE id = $1 AND is_active = TRUE")
            .bind(plant_id)
            .fetch_optional(pool)
            .await?;

    let (plant_id_db, plant_type_id): (Uuid, Uuid) = match plant_row {
        Some(row) => (row.try_get("id")?, row.try_get("plant_type_id")?),
        None => {
            record_ledger(pool, envelope, "ERROR").await?;
            return Ok((IngestResult::Error, None));
        }
    };

    // Thresholds
    let threshold_rows = sqlx::query(
        r#"SELECT metric, warn_min, warn_max, crit_min, crit_max
           FROM plant_type_metric_threshold
           WHERE plant_type_id = $1"#,
    )
    .bind(plant_type_id)
    .fetch_all(pool)
    .await?;

    let thresholds: Vec<MetricThreshold> = threshold_rows
        .iter()
        .map(|r| MetricThreshold {
            metric: r.try_get("metric").unwrap_or_default(),
            warn_min: r.try_get("warn_min").unwrap_or(None),
            warn_max: r.try_get("warn_max").unwrap_or(None),
            crit_min: r.try_get("crit_min").unwrap_or(None),
            crit_max: r.try_get("crit_max").unwrap_or(None),
        })
        .collect();

    // Per-metric severity
    let readings: &[(&str, Option<f64>)] = &[
        ("soil_moisture", envelope.soil_moisture),
        ("ambient_light_lux", envelope.ambient_light_lux),
        ("ambient_humidity_rh", envelope.ambient_humidity_rh),
        ("ambient_temp_c", envelope.ambient_temp_c),
    ];

    let mut metric_severities: HashMap<String, ThreshSeverity> =
        HashMap::with_capacity(readings.len());
    for (metric_name, opt_val) in readings {
        if let Some(val) = opt_val {
            let thresh = thresholds.iter().find(|t| t.metric == *metric_name);
            let sev = match thresh {
                Some(t) => threshold::evaluate_metric(*val, t),
                None => ThreshSeverity::Normal,
            };
            metric_severities.insert(metric_name.to_string(), sev);
        }
    }

    let overall_severity = threshold::aggregate_severity(metric_severities.values().copied());

    // Previous severity
    let prev_row = sqlx::query("SELECT severity FROM plant_current_state WHERE plant_id = $1")
        .bind(plant_id_db)
        .fetch_optional(pool)
        .await?;

    let prev_severity = prev_row
        .as_ref()
        .and_then(|r| r.try_get::<String, _>("severity").ok())
        .map(|s| ThreshSeverity::from_str(&s))
        .unwrap_or(ThreshSeverity::Normal);

    // Write to TelemetrySink
    let mut tags = HashMap::with_capacity(3);
    tags.insert("plant_id".to_string(), envelope.plant_id.clone());
    tags.insert("device_uid".to_string(), envelope.device_uid.clone());
    tags.insert("plant_type_id".to_string(), plant_type_id.to_string());

    let mut fields: HashMap<String, f64> = HashMap::with_capacity(readings.len());
    if let Some(v) = envelope.soil_moisture {
        fields.insert("soil_moisture".into(), v);
    }
    if let Some(v) = envelope.ambient_light_lux {
        fields.insert("ambient_light_lux".into(), v);
    }
    if let Some(v) = envelope.ambient_humidity_rh {
        fields.insert("ambient_humidity_rh".into(), v);
    }
    if let Some(v) = envelope.ambient_temp_c {
        fields.insert("ambient_temp_c".into(), v);
    }

    if !fields.is_empty() {
        let point = TelemetryPoint {
            measurement: "plant_telemetry".to_string(),
            tags,
            fields,
            timestamp_ns: envelope.timestamp_ns,
        };
        if let Err(e) = sink.write_points(vec![point]).await {
            warn!(error = %e, "TelemetrySink write failed (non-fatal)");
        }
    }

    // Update plant_current_state
    let metric_sev_json = serde_json::Value::Object(
        metric_severities
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.as_str().to_owned())))
            .collect(),
    );

    sqlx::query(r#"
        INSERT INTO plant_current_state
            (plant_id, updated_at, last_ingest_id, severity,
             soil_moisture, ambient_light_lux, ambient_humidity_rh, ambient_temp_c,
             metric_severity)
        VALUES ($1, NOW(), $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (plant_id) DO UPDATE SET
            updated_at          = EXCLUDED.updated_at,
            last_ingest_id      = EXCLUDED.last_ingest_id,
            severity            = EXCLUDED.severity,
            soil_moisture       = COALESCE(EXCLUDED.soil_moisture, plant_current_state.soil_moisture),
            ambient_light_lux   = COALESCE(EXCLUDED.ambient_light_lux, plant_current_state.ambient_light_lux),
            ambient_humidity_rh = COALESCE(EXCLUDED.ambient_humidity_rh, plant_current_state.ambient_humidity_rh),
            ambient_temp_c      = COALESCE(EXCLUDED.ambient_temp_c, plant_current_state.ambient_temp_c),
            metric_severity     = EXCLUDED.metric_severity
    "#)
    .bind(plant_id_db)
    .bind(&envelope.ingest_id)
    .bind(overall_severity.as_str())
    .bind(envelope.soil_moisture)
    .bind(envelope.ambient_light_lux)
    .bind(envelope.ambient_humidity_rh)
    .bind(envelope.ambient_temp_c)
    .bind(metric_sev_json)
    .execute(pool)
    .await?;

    // Update device
    sqlx::query(
        "UPDATE device SET last_seen_at = NOW(), last_ingest_id = $2 WHERE device_uid = $1",
    )
    .bind(&envelope.device_uid)
    .bind(&envelope.ingest_id)
    .execute(pool)
    .await?;

    // Ticker event
    let message = format!(
        "Plant {} reading: severity={}",
        envelope.plant_id, overall_severity
    );
    sqlx::query(
        r#"
        INSERT INTO ticker_event (plant_id, device_uid, severity, message, payload)
        VALUES ($1, $2, $3, $4, $5)
    "#,
    )
    .bind(plant_id_db)
    .bind(&envelope.device_uid)
    .bind(overall_severity.as_str())
    .bind(&message)
    .bind(serde_json::json!({"ingest_id": &envelope.ingest_id}))
    .execute(pool)
    .await?;

    // Status change event
    let status_change = if overall_severity != prev_severity {
        let change = StatusChange {
            plant_id: envelope.plant_id.clone(),
            prev_severity: severity_to_proto(prev_severity) as i32,
            new_severity: severity_to_proto(overall_severity) as i32,
            occurred_at_ns: envelope.timestamp_ns,
        };

        if let Some(chan) = amqp_chan {
            let payload = serde_json::json!({
                "type":          "PlantStatusChanged.v1",
                "plant_id":      &envelope.plant_id,
                "prev_severity": prev_severity.as_str(),
                "new_severity":  overall_severity.as_str(),
                "occurred_at_ns": envelope.timestamp_ns,
            });
            let body = serde_json::to_vec(&payload).unwrap_or_default();
            let _ = chan
                .basic_publish(
                    "",
                    "plant.status_change",
                    lapin::options::BasicPublishOptions::default(),
                    &body,
                    lapin::BasicProperties::default().with_content_type("application/json".into()),
                )
                .await;
        }

        Some(change)
    } else {
        None
    };

    record_ledger(pool, envelope, "OK").await?;

    Ok((IngestResult::Ok, status_change))
}

fn severity_to_proto(s: ThreshSeverity) -> Severity {
    match s {
        ThreshSeverity::Normal => Severity::Normal,
        ThreshSeverity::Warn => Severity::Warn,
        ThreshSeverity::Critical => Severity::Critical,
    }
}

async fn record_ledger(pool: &PgPool, env: &TelemetryEnvelope, result: &str) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO telemetry_ingest_ledger
            (ingest_id, device_uid, plant_id, timestamp_ns, result)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (ingest_id) DO NOTHING
    "#,
    )
    .bind(&env.ingest_id)
    .bind(&env.device_uid)
    .bind(Uuid::parse_str(&env.plant_id).ok())
    .bind(env.timestamp_ns)
    .bind(result)
    .execute(pool)
    .await?;
    Ok(())
}

// ------------------------------------------------------------------ //
//  tonic trait impl                                                   //
// ------------------------------------------------------------------ //

#[tonic::async_trait]
impl SupervisorService for SupervisorServiceImpl {
    async fn ingest_telemetry(
        &self,
        request: Request<IngestTelemetryRequest>,
    ) -> Result<Response<IngestTelemetryResponse>, Status> {
        let req = request.into_inner();
        let mut results = Vec::with_capacity(req.envelopes.len());
        let mut status_changes = Vec::with_capacity(req.envelopes.len());

        for envelope in &req.envelopes {
            match process_envelope(envelope, &self.pool, &*self.sink, self.amqp_chan.as_ref()).await
            {
                Ok((code, opt_change)) => {
                    results.push(ItemResult {
                        ingest_id: envelope.ingest_id.clone(),
                        result: code as i32,
                        error: String::new(),
                    });
                    if let Some(c) = opt_change {
                        status_changes.push(c);
                    }
                }
                Err(e) => {
                    error!(error = %e, ingest_id = %envelope.ingest_id, "ingest failed");
                    results.push(ItemResult {
                        ingest_id: envelope.ingest_id.clone(),
                        result: IngestResult::Error as i32,
                        error: e.to_string(),
                    });
                }
            }
        }

        info!(
            processed = results.len(),
            transitions = status_changes.len(),
            "IngestTelemetry complete"
        );
        Ok(Response::new(IngestTelemetryResponse {
            results,
            status_changes,
        }))
    }
}
