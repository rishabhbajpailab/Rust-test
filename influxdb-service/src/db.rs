//! InfluxDB 2.x client wrapper.

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use influxdb2::Client;
use influxdb2::models::Query;

/// Thin wrapper around the [`influxdb2::Client`].
pub struct Db {
    pub client: Client,
    pub org: String,
    pub bucket: String,
}

impl Db {
    /// Connect to InfluxDB.
    pub fn connect(url: &str, token: &str, org: &str, bucket: &str) -> Self {
        Self {
            client: Client::new(url, org, token),
            org: org.to_string(),
            bucket: bucket.to_string(),
        }
    }

    // ------------------------------------------------------------------ //
    //  Write                                                               //
    // ------------------------------------------------------------------ //

    /// Write line-protocol data directly to InfluxDB.
    pub async fn write_line_protocol(&self, data: String) -> Result<()> {
        self.client
            .write_line_protocol(&self.org, &self.bucket, data)
            .await
            .context("InfluxDB write failed")
    }

    // ------------------------------------------------------------------ //
    //  Query                                                               //
    // ------------------------------------------------------------------ //

    /// Run a raw Flux query and return the parsed FluxRecords as JSON strings.
    pub async fn query_raw(&self, flux: &str) -> Result<Vec<influxdb2::api::query::FluxRecord>> {
        let query = Query::new(flux.to_string());
        let records = self
            .client
            .query_raw(Some(query))
            .await
            .context("InfluxDB query failed")?;
        Ok(records)
    }

    // ------------------------------------------------------------------ //
    //  Delete                                                              //
    // ------------------------------------------------------------------ //

    /// Delete points in the given time range / predicate.
    ///
    /// `start` and `stop` must be RFC3339 strings, e.g. `"2024-01-01T00:00:00Z"`.
    pub async fn delete(
        &self,
        measurement: &str,
        start: &str,
        stop: &str,
        extra_predicate: &str,
    ) -> Result<()> {
        let start_dt = parse_naive_dt(start)
            .with_context(|| format!("Invalid start timestamp: {start}"))?;
        let stop_dt = parse_naive_dt(stop)
            .with_context(|| format!("Invalid stop timestamp: {stop}"))?;

        let predicate = if extra_predicate.is_empty() {
            format!("_measurement=\"{}\"", measurement)
        } else {
            format!("_measurement=\"{}\" AND {}", measurement, extra_predicate)
        };

        self.client
            .delete(&self.bucket, start_dt, stop_dt, Some(predicate))
            .await
            .context("InfluxDB delete failed")
    }
}

/// Parse an RFC3339 / ISO-8601 string into a `NaiveDateTime`.
fn parse_naive_dt(s: &str) -> Result<NaiveDateTime> {
    // Try parsing common formats.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.naive_utc());
    }
    // Fallback: naive format without timezone.
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .context("Failed to parse datetime")
}
