//! Axum HTTP handlers for the coordinator service.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sqlx::Row;
use chrono::{DateTime, Utc};
use tracing::{error, info};

use crate::{
    models::{
        DataRequest, DataResponse, DeleteTimeSeriesRequest, StructuredWriteResult,
        TimeSeriesQueryRequest, TimeSeriesWriteResult, UpdateStructuredRequest,
    },
    AppState,
};
use proto::{
    influxdb_service::{DataPoint, DeleteRequest as InfluxDeleteRequest, QueryRequest, WriteRequest},
    postgres_service::{
        CreateRequest, DeleteRequest as PgDeleteRequest, ListRequest, ReadRequest, UpdateRequest,
    },
};

// ------------------------------------------------------------------ //
//  POST /data                                                         //
// ------------------------------------------------------------------ //

/// Accept a request that may contain structured data, time-series data, or both.
/// Forwards each kind to the appropriate backend service concurrently via gRPC.
pub async fn post_data(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DataRequest>,
) -> impl IntoResponse {
    if req.structured.is_none() && req.timeseries.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "at least one of 'structured' or 'timeseries' must be present"})),
        );
    }

    // Fan-out both calls concurrently.
    let (structured_result, timeseries_result) = tokio::join!(
        handle_structured(&state, req.structured),
        handle_timeseries(&state, req.timeseries),
    );

    let resp = DataResponse {
        structured: structured_result,
        timeseries: timeseries_result,
    };

    info!("POST /data processed");
    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
}

async fn handle_structured(
    state: &AppState,
    records: Option<Vec<crate::models::StructuredRecord>>,
) -> Option<Vec<StructuredWriteResult>> {
    let records = records?;
    let mut results = Vec::with_capacity(records.len());

    for r in records {
        let payload = r.payload.to_string();
        let mut pg_client = state.pg_client.clone();

        let result = pg_client
            .create(CreateRequest {
                table_name: r.table.clone(),
                payload,
            })
            .await;

        match result {
            Ok(resp) => {
                let inner = resp.into_inner();
                results.push(StructuredWriteResult {
                    table: r.table,
                    id: if inner.success {
                        Some(inner.id)
                    } else {
                        None
                    },
                    success: inner.success,
                    error: if inner.error.is_empty() {
                        None
                    } else {
                        Some(inner.error)
                    },
                });
            }
            Err(e) => {
                error!(error = %e, "postgres create rpc failed");
                results.push(StructuredWriteResult {
                    table: r.table,
                    id: None,
                    success: false,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    Some(results)
}

async fn handle_timeseries(
    state: &AppState,
    points: Option<Vec<crate::models::TimeSeriesPoint>>,
) -> Option<TimeSeriesWriteResult> {
    let points = points?;
    let proto_points: Vec<DataPoint> = points
        .into_iter()
        .map(|p| DataPoint {
            measurement: p.measurement,
            tags: p.tags,
            fields: p.fields,
            timestamp_ns: p.timestamp_ns,
        })
        .collect();

    let mut influx_client = state.influx_client.clone();
    let result = influx_client
        .write(WriteRequest {
            points: proto_points,
        })
        .await;

    match result {
        Ok(resp) => {
            let inner = resp.into_inner();
            Some(TimeSeriesWriteResult {
                success: inner.success,
                error: if inner.error.is_empty() {
                    None
                } else {
                    Some(inner.error)
                },
            })
        }
        Err(e) => {
            error!(error = %e, "influxdb write rpc failed");
            Some(TimeSeriesWriteResult {
                success: false,
                error: Some(e.to_string()),
            })
        }
    }
}

// ------------------------------------------------------------------ //
//  Structured (PostgreSQL) endpoints                                  //
// ------------------------------------------------------------------ //

/// GET /data/structured/:table/:id
pub async fn get_structured(
    State(state): State<Arc<AppState>>,
    Path((table, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let mut client = state.pg_client.clone();
    match client.read(ReadRequest { id, table_name: table }).await {
        Ok(resp) => {
            let inner = resp.into_inner();
            if inner.success {
                (StatusCode::OK, Json(serde_json::to_value(inner.record).unwrap()))
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": inner.error})),
                )
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /data/structured/:table
pub async fn list_structured(
    State(state): State<Arc<AppState>>,
    Path(table): Path<String>,
) -> impl IntoResponse {
    let mut client = state.pg_client.clone();
    match client
        .list(ListRequest {
            table_name: table,
            filter: String::new(),
            limit: 100,
            offset: 0,
        })
        .await
    {
        Ok(resp) => {
            let inner = resp.into_inner();
            (StatusCode::OK, Json(serde_json::to_value(inner.records).unwrap()))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// PUT /data/structured/:table/:id
pub async fn update_structured(
    State(state): State<Arc<AppState>>,
    Path((table, id)): Path<(String, String)>,
    Json(body): Json<UpdateStructuredRequest>,
) -> impl IntoResponse {
    let mut client = state.pg_client.clone();
    let payload = body.payload.to_string();
    match client
        .update(UpdateRequest {
            id,
            table_name: table,
            payload,
        })
        .await
    {
        Ok(resp) => {
            let inner = resp.into_inner();
            if inner.success {
                (StatusCode::OK, Json(serde_json::json!({"success": true})))
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": inner.error})),
                )
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// DELETE /data/structured/:table/:id
pub async fn delete_structured(
    State(state): State<Arc<AppState>>,
    Path((table, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let mut client = state.pg_client.clone();
    match client
        .delete(PgDeleteRequest { id, table_name: table })
        .await
    {
        Ok(resp) => {
            let inner = resp.into_inner();
            if inner.success {
                StatusCode::NO_CONTENT.into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": inner.error})),
                )
                    .into_response()
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ------------------------------------------------------------------ //
//  Time-series (InfluxDB) endpoints                                   //
// ------------------------------------------------------------------ //

/// POST /data/timeseries/query
pub async fn query_timeseries(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TimeSeriesQueryRequest>,
) -> impl IntoResponse {
    let mut client = state.influx_client.clone();
    match client
        .query(QueryRequest {
            measurement: body.measurement,
            start: body.start,
            stop: body.stop,
            tag_filters: body.tag_filters,
            limit: body.limit,
        })
        .await
    {
        Ok(resp) => {
            let inner = resp.into_inner();
            (StatusCode::OK, Json(serde_json::to_value(inner).unwrap()))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// DELETE /data/timeseries
pub async fn delete_timeseries(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DeleteTimeSeriesRequest>,
) -> impl IntoResponse {
    let mut client = state.influx_client.clone();
    match client
        .delete(InfluxDeleteRequest {
            measurement: body.measurement,
            start: body.start,
            stop: body.stop,
            tag_filters: body.tag_filters,
        })
        .await
    {
        Ok(resp) => {
            let inner = resp.into_inner();
            if inner.success {
                StatusCode::NO_CONTENT.into_response()
            } else {
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"error": inner.error})),
                )
                    .into_response()
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ------------------------------------------------------------------ //
//  Health                                                             //
// ------------------------------------------------------------------ //

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

// ------------------------------------------------------------------ //
//  Dashboard endpoints                                                //
// ------------------------------------------------------------------ //

/// GET /dashboard/attention — plants needing attention (WARN or CRITICAL)
pub async fn dashboard_attention(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let pool = match &state.db_pool {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "dashboard database not configured"})),
            );
        }
    };

    let rows = sqlx::query(r#"
        SELECT
            p.id::text         AS plant_id,
            p.display_name,
            p.location,
            pt.name            AS plant_type_name,
            pcs.severity,
            pcs.updated_at,
            pcs.soil_moisture,
            pcs.ambient_light_lux,
            pcs.ambient_humidity_rh,
            pcs.ambient_temp_c
        FROM plant_current_state pcs
        JOIN plant p    ON p.id = pcs.plant_id
        JOIN plant_type pt ON pt.id = p.plant_type_id
        WHERE pcs.severity IN ('WARN', 'CRITICAL')
          AND p.is_active = TRUE
        ORDER BY pcs.severity DESC, pcs.updated_at DESC
    "#)
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => {
            let data: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "plant_id":            r.try_get::<String, _>("plant_id").ok(),
                        "display_name":        r.try_get::<String, _>("display_name").ok(),
                        "location":            r.try_get::<Option<String>, _>("location").ok().flatten(),
                        "plant_type_name":     r.try_get::<String, _>("plant_type_name").ok(),
                        "severity":            r.try_get::<String, _>("severity").ok(),
                        "updated_at":          r.try_get::<DateTime<Utc>, _>("updated_at").ok().map(|t: DateTime<Utc>| t.to_rfc3339()),
                        "soil_moisture":       r.try_get::<Option<f64>, _>("soil_moisture").ok().flatten(),
                        "ambient_light_lux":   r.try_get::<Option<f64>, _>("ambient_light_lux").ok().flatten(),
                        "ambient_humidity_rh": r.try_get::<Option<f64>, _>("ambient_humidity_rh").ok().flatten(),
                        "ambient_temp_c":      r.try_get::<Option<f64>, _>("ambient_temp_c").ok().flatten(),
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"plants": data})))
        }
        Err(e) => {
            error!(error = %e, "dashboard_attention query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}

/// GET /dashboard/ticker?limit=N — latest ticker events
pub async fn dashboard_ticker(
    State(state): State<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let pool = match &state.db_pool {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "dashboard database not configured"})),
            );
        }
    };

    let limit: i64 = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_i64)
        .min(200);

    let rows = sqlx::query(r#"
        SELECT
            id,
            occurred_at,
            plant_id::text AS plant_id,
            device_uid,
            severity,
            message
        FROM ticker_event
        ORDER BY occurred_at DESC
        LIMIT $1
    "#)
    .bind(limit)
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => {
            let data: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id":          r.try_get::<i64, _>("id").ok(),
                        "occurred_at": r.try_get::<DateTime<Utc>, _>("occurred_at").ok().map(|t: DateTime<Utc>| t.to_rfc3339()),
                        "plant_id":    r.try_get::<Option<String>, _>("plant_id").ok().flatten(),
                        "device_uid":  r.try_get::<Option<String>, _>("device_uid").ok().flatten(),
                        "severity":    r.try_get::<String, _>("severity").ok(),
                        "message":     r.try_get::<String, _>("message").ok(),
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"events": data})))
        }
        Err(e) => {
            error!(error = %e, "dashboard_ticker query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}

/// GET /dashboard/edges?ttl_seconds=T — edge node online/offline status
pub async fn dashboard_edges(
    State(state): State<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let pool = match &state.db_pool {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "dashboard database not configured"})),
            );
        }
    };

    let ttl_seconds: i64 = params
        .get("ttl_seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(300_i64);

    let rows = sqlx::query(r#"
        SELECT
            id::text AS id,
            device_uid,
            firmware_version,
            last_seen_at,
            is_active,
            CASE
                WHEN last_seen_at IS NULL THEN FALSE
                WHEN last_seen_at >= NOW() - ($1 * INTERVAL '1 second') THEN TRUE
                ELSE FALSE
            END AS online
        FROM device
        WHERE is_active = TRUE
        ORDER BY last_seen_at DESC NULLS LAST
    "#)
    .bind(ttl_seconds)
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => {
            let data: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id":               r.try_get::<String, _>("id").ok(),
                        "device_uid":       r.try_get::<String, _>("device_uid").ok(),
                        "firmware_version": r.try_get::<Option<String>, _>("firmware_version").ok().flatten(),
                        "last_seen_at":     r.try_get::<Option<DateTime<Utc>>, _>("last_seen_at").ok().flatten().map(|t: DateTime<Utc>| t.to_rfc3339()),
                        "is_active":        r.try_get::<bool, _>("is_active").ok(),
                        "online":           r.try_get::<bool, _>("online").ok(),
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"devices": data})))
        }
        Err(e) => {
            error!(error = %e, "dashboard_edges query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}
