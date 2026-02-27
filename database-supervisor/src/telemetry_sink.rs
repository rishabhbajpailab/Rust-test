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
        let mut guard = self.points.lock().unwrap();
        std::mem::take(&mut *guard)
    }

    /// Non-destructive snapshot of currently collected points.
    pub fn snapshot(&self) -> Vec<TelemetryPoint> {
        self.points.lock().unwrap().clone()
    }
}

#[async_trait]
impl TelemetrySink for FakeTelemetrySink {
    async fn write_points(&self, mut points: Vec<TelemetryPoint>) -> Result<()> {
        let mut guard = self.points.lock().unwrap();
        guard.append(&mut points);
        Ok(())
    }
}

// ------------------------------------------------------------------ //
//  InfluxTelemetrySink (production)                                   //
// ------------------------------------------------------------------ //

fn escape_lp(s: &str) -> String {
    s.replace(' ', "\\ ")
        .replace(',', "\\,")
        .replace('=', "\\=")
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
        let mut data = String::with_capacity(points.len() * 96);
        for (idx, p) in points.iter().enumerate() {
            if idx > 0 {
                data.push('\n');
            }

            data.push_str(&escape_lp(&p.measurement));
            for (k, v) in &p.tags {
                data.push(',');
                data.push_str(&escape_lp(k));
                data.push('=');
                data.push_str(&escape_lp(v));
            }

            data.push(' ');
            for (field_idx, (k, v)) in p.fields.iter().enumerate() {
                if field_idx > 0 {
                    data.push(',');
                }
                data.push_str(k);
                data.push('=');
                data.push_str(&v.to_string());
            }

            if p.timestamp_ns != 0 {
                data.push(' ');
                data.push_str(&p.timestamp_ns.to_string());
            }
        }
        self.client
            .write_line_protocol(&self.org, &self.bucket, data)
            .await
            .map_err(|e| anyhow::anyhow!("InfluxDB write failed: {e}"))?;

        Ok(())
    }
}
