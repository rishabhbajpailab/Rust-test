//! TelemetrySink trait and implementations.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;

// ------------------------------------------------------------------ //
//  Domain types                                                       //
// ------------------------------------------------------------------ //

/// A normalised telemetry data point ready to be stored in the time-series DB.
#[derive(Debug, Clone)]
pub struct TelemetryPoint {
    pub measurement: String,
    /// Tag key → value (e.g. plant_id, device_uid)
    pub tags: std::collections::HashMap<String, String>,
    /// Field key → value (e.g. soil_moisture, ambient_temp_c)
    pub fields: std::collections::HashMap<String, f64>,
    /// Unix nanoseconds timestamp.
    pub timestamp_ns: i64,
}

// ------------------------------------------------------------------ //
//  Trait                                                              //
// ------------------------------------------------------------------ //

/// Async sink that accepts normalised telemetry points.
#[async_trait]
pub trait TelemetrySink: Send + Sync {
    async fn write_points(&self, points: Vec<TelemetryPoint>) -> Result<()>;
}

// ------------------------------------------------------------------ //
//  FakeTelemetrySink (for tests)                                      //
// ------------------------------------------------------------------ //

/// In-memory sink that collects written points for test assertions.
#[derive(Debug, Default, Clone)]
pub struct FakeTelemetrySink {
    pub points: Arc<Mutex<Vec<TelemetryPoint>>>,
}

impl FakeTelemetrySink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume all points written so far (drains the buffer).
    pub fn drain(&self) -> Vec<TelemetryPoint> {
        self.points.lock().unwrap().drain(..).collect()
    }

    /// Non-destructive snapshot of currently collected points.
    pub fn snapshot(&self) -> Vec<TelemetryPoint> {
        self.points.lock().unwrap().clone()
    }
}

#[async_trait]
impl TelemetrySink for FakeTelemetrySink {
    async fn write_points(&self, points: Vec<TelemetryPoint>) -> Result<()> {
        self.points.lock().unwrap().extend(points);
        Ok(())
    }
}

// ------------------------------------------------------------------ //
//  InfluxTelemetrySink (production)                                   //
// ------------------------------------------------------------------ //

fn escape_lp(s: &str) -> String {
    s.replace(' ', "\\ ").replace(',', "\\,").replace('=', "\\=")
}

/// Production sink that writes to InfluxDB 2.x via the `influxdb2` client.
pub struct InfluxTelemetrySink {
    client: influxdb2::Client,
    org: String,
    bucket: String,
}

impl InfluxTelemetrySink {
    pub fn new(url: &str, org: &str, token: &str, bucket: &str) -> Self {
        let client = influxdb2::Client::new(url, org, token);
        Self {
            client,
            org: org.to_string(),
            bucket: bucket.to_string(),
        }
    }
}

#[async_trait]
impl TelemetrySink for InfluxTelemetrySink {
    async fn write_points(&self, points: Vec<TelemetryPoint>) -> Result<()> {
        let mut lines = Vec::with_capacity(points.len());
        for p in &points {
            let tags: String = p
                .tags
                .iter()
                .map(|(k, v)| format!(",{}={}", escape_lp(k), escape_lp(v)))
                .collect();
            let fields: String = p
                .fields
                .iter()
                .enumerate()
                .map(|(i, (k, v))| {
                    let sep = if i == 0 { "" } else { "," };
                    format!("{}{k}={v}", sep)
                })
                .collect();
            let line = if p.timestamp_ns != 0 {
                format!(
                    "{}{} {} {}",
                    escape_lp(&p.measurement),
                    tags,
                    fields,
                    p.timestamp_ns
                )
            } else {
                format!("{}{} {}", escape_lp(&p.measurement), tags, fields)
            };
            lines.push(line);
        }

        let data = lines.join("\n");
        self.client
            .write_line_protocol(&self.org, &self.bucket, data)
            .await
            .map_err(|e| anyhow::anyhow!("InfluxDB write failed: {e}"))?;

        Ok(())
    }
}
