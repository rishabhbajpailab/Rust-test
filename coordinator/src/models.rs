//! HTTP request/response models for the coordinator's public REST API.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ------------------------------------------------------------------ //
//  Inbound (client → coordinator)                                     //
// ------------------------------------------------------------------ //

/// A single structured record destined for PostgreSQL.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StructuredRecord {
    /// Target table / collection name.
    pub table: String,
    /// JSON-serialisable payload for the record.
    pub payload: serde_json::Value,
}

/// A single time-series data point destined for InfluxDB.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeSeriesPoint {
    pub measurement: String,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    pub fields: HashMap<String, f64>,
    /// Unix nanoseconds; 0 → let the DB assign the timestamp.
    #[serde(default)]
    pub timestamp_ns: i64,
}

/// Top-level request body accepted by `POST /data`.
///
/// At least one of `structured` or `timeseries` must be present.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DataRequest {
    /// One or more structured records to persist in PostgreSQL.
    pub structured: Option<Vec<StructuredRecord>>,
    /// One or more time-series points to persist in InfluxDB.
    pub timeseries: Option<Vec<TimeSeriesPoint>>,
}

/// Request body for `PUT /data/structured/{table}/{id}`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateStructuredRequest {
    pub payload: serde_json::Value,
}

/// Request body for `POST /data/timeseries/query`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeSeriesQueryRequest {
    pub measurement: String,
    pub start: String,
    pub stop: String,
    #[serde(default)]
    pub tag_filters: HashMap<String, String>,
    #[serde(default)]
    pub limit: u32,
}

/// Request body for `DELETE /data/timeseries`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeleteTimeSeriesRequest {
    pub measurement: String,
    pub start: String,
    pub stop: String,
    #[serde(default)]
    pub tag_filters: HashMap<String, String>,
}

// ------------------------------------------------------------------ //
//  Outbound (coordinator → client)                                    //
// ------------------------------------------------------------------ //

/// Outcome of writing a single structured record.
#[derive(Debug, Serialize)]
pub struct StructuredWriteResult {
    pub table: String,
    pub id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

/// Combined response for `POST /data`.
#[derive(Debug, Serialize)]
pub struct DataResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<Vec<StructuredWriteResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeseries: Option<TimeSeriesWriteResult>,
}

/// Outcome of writing time-series data.
#[derive(Debug, Serialize)]
pub struct TimeSeriesWriteResult {
    pub success: bool,
    pub error: Option<String>,
}
