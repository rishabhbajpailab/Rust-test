//! InfluxDB time-series gRPC service.
//!
//! Listens on `INFLUXDB_SERVICE_ADDR` (default `[::1]:50052`) and serves the
//! `InfluxDbService` gRPC interface defined in
//! `protos/influxdb_service.proto`.
//!
//! # Secrets (via Bitwarden)
//! | Env var                        | BWS secret ID env var              |
//! |--------------------------------|------------------------------------|
//! | `INFLUXDB_URL`                 | `BWS_INFLUXDB_URL_ID`              |
//! | `INFLUXDB_TOKEN`               | `BWS_INFLUXDB_TOKEN_ID`            |
//! | `INFLUXDB_ORG`                 | `BWS_INFLUXDB_ORG_ID`              |
//! | `INFLUXDB_BUCKET`              | `BWS_INFLUXDB_BUCKET_ID`           |

mod db;
mod secrets;

use std::sync::Arc;

use anyhow::Result;
use proto::influxdb_service::{
    influx_db_service_server::{InfluxDbService, InfluxDbServiceServer},
    DataPoint, DeleteRequest, DeleteResponse, QueryRequest, QueryResponse, WriteRequest,
    WriteResponse,
};
use tonic::{transport::Server, Request, Response, Status};
use tracing::{error, info};

// ------------------------------------------------------------------ //
//  Helper: build line-protocol from a DataPoint                      //
// ------------------------------------------------------------------ //

fn to_line_protocol(pt: &DataPoint) -> String {
    // measurement,tag1=v1,tag2=v2 field1=1.0,field2=2.0 <timestamp>
    let tags: String = pt
        .tags
        .iter()
        .map(|(k, v)| format!(",{}={}", escape_lp(k), escape_lp(v)))
        .collect();

    let fields: String = pt
        .fields
        .iter()
        .enumerate()
        .map(|(i, (k, v))| {
            let sep = if i == 0 { "" } else { "," };
            format!("{}{}={}", sep, escape_lp(k), v)
        })
        .collect();

    if pt.timestamp_ns == 0 {
        format!("{}{} {}", escape_lp(&pt.measurement), tags, fields)
    } else {
        format!(
            "{}{} {} {}",
            escape_lp(&pt.measurement),
            tags,
            fields,
            pt.timestamp_ns
        )
    }
}

fn escape_lp(s: &str) -> String {
    s.replace(' ', "\\ ").replace(',', "\\,").replace('=', "\\=")
}

// ------------------------------------------------------------------ //
//  gRPC service implementation                                        //
// ------------------------------------------------------------------ //

pub struct InfluxDbServiceImpl {
    db: Arc<db::Db>,
}

#[tonic::async_trait]
impl InfluxDbService for InfluxDbServiceImpl {
    async fn write(
        &self,
        request: Request<WriteRequest>,
    ) -> Result<Response<WriteResponse>, Status> {
        let req = request.into_inner();
        let line_proto: String = req
            .points
            .iter()
            .map(to_line_protocol)
            .collect::<Vec<_>>()
            .join("\n");

        match self.db.write_line_protocol(line_proto).await {
            Ok(()) => Ok(Response::new(WriteResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => {
                error!(error = %e, "write failed");
                Ok(Response::new(WriteResponse {
                    success: false,
                    error: e.to_string(),
                }))
            }
        }
    }

    async fn query(
        &self,
        request: Request<QueryRequest>,
    ) -> Result<Response<QueryResponse>, Status> {
        let req = request.into_inner();

        // Build a basic Flux query.
        let mut flux = format!(
            r#"from(bucket: "{}")
  |> range(start: {}, stop: {})
  |> filter(fn: (r) => r._measurement == "{}")"#,
            self.db.bucket, req.start, req.stop, req.measurement
        );

        for (k, v) in &req.tag_filters {
            flux.push_str(&format!(
                r#"
  |> filter(fn: (r) => r["{}"] == "{}")"#,
                k, v
            ));
        }

        if req.limit > 0 {
            flux.push_str(&format!("\n  |> limit(n: {})", req.limit));
        }

        match self.db.query_raw(&flux).await {
            Ok(records) => {
                // Convert FluxRecord values into DataPoints.
                let points: Vec<DataPoint> = records
                    .into_iter()
                    .map(|r| {
                        let mut fields: std::collections::HashMap<String, f64> =
                            std::collections::HashMap::new();
                        let mut tags: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        for (k, v) in &r.values {
                            use influxdb2_structmap::value::Value;
                            match v {
                                Value::Double(d) => {
                                    fields.insert(k.clone(), (*d).into());
                                }
                                Value::Long(l) => {
                                    fields.insert(k.clone(), *l as f64);
                                }
                                Value::UnsignedLong(u) => {
                                    fields.insert(k.clone(), *u as f64);
                                }
                                Value::Bool(b) => {
                                    fields.insert(k.clone(), if *b { 1.0 } else { 0.0 });
                                }
                                Value::String(s) => {
                                    tags.insert(k.clone(), s.clone());
                                }
                                _ => {}
                            }
                        }
                        DataPoint {
                            measurement: req.measurement.clone(),
                            tags,
                            fields,
                            timestamp_ns: 0,
                        }
                    })
                    .collect();

                Ok(Response::new(QueryResponse {
                    points,
                    success: true,
                    error: String::new(),
                }))
            }
            Err(e) => {
                error!(error = %e, "query failed");
                Ok(Response::new(QueryResponse {
                    points: vec![],
                    success: false,
                    error: e.to_string(),
                }))
            }
        }
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();

        let predicate: String = req
            .tag_filters
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, v))
            .collect::<Vec<_>>()
            .join(" AND ");

        match self
            .db
            .delete(&req.measurement, &req.start, &req.stop, &predicate)
            .await
        {
            Ok(()) => Ok(Response::new(DeleteResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => {
                error!(error = %e, "delete failed");
                Ok(Response::new(DeleteResponse {
                    success: false,
                    error: e.to_string(),
                }))
            }
        }
    }
}

// ------------------------------------------------------------------ //
//  Entry point                                                        //
// ------------------------------------------------------------------ //

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("influxdb_service=info".parse()?),
        )
        .json()
        .init();

    // Resolve secrets via Bitwarden (or env fallback).
    let influx_url = secrets::get_secret(
        &std::env::var("BWS_INFLUXDB_URL_ID").unwrap_or_else(|_| "influxdb-url".to_string()),
        "INFLUXDB_URL",
    )
    .await?;

    let influx_token = secrets::get_secret(
        &std::env::var("BWS_INFLUXDB_TOKEN_ID")
            .unwrap_or_else(|_| "influxdb-token".to_string()),
        "INFLUXDB_TOKEN",
    )
    .await?;

    let influx_org = secrets::get_secret(
        &std::env::var("BWS_INFLUXDB_ORG_ID").unwrap_or_else(|_| "influxdb-org".to_string()),
        "INFLUXDB_ORG",
    )
    .await?;

    let influx_bucket = secrets::get_secret(
        &std::env::var("BWS_INFLUXDB_BUCKET_ID")
            .unwrap_or_else(|_| "influxdb-bucket".to_string()),
        "INFLUXDB_BUCKET",
    )
    .await?;

    let db = db::Db::connect(&influx_url, &influx_token, &influx_org, &influx_bucket);

    let addr = std::env::var("INFLUXDB_SERVICE_ADDR")
        .unwrap_or_else(|_| "[::1]:50052".to_string())
        .parse()?;

    let svc = InfluxDbServiceImpl { db: Arc::new(db) };

    info!(%addr, "influxdb-service listening");

    Server::builder()
        .add_service(InfluxDbServiceServer::new(svc))
        .serve(addr)
        .await?;

    Ok(())
}
