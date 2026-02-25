//! Axum HTTP handlers for the coordinator service.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
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
