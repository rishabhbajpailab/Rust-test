//! Coordinator service — HTTP API gateway.
//!
//! Receives JSON requests from clients and fans them out to the appropriate
//! backend gRPC services (`postgres-service` and/or `influxdb-service`).
//! Internal communication uses serialised protobuf messages over gRPC.
//!
//! # Configuration
//! All addresses and secrets are resolved via Bitwarden Secrets Manager
//! (when `BWS_ACCESS_TOKEN` is set) or plain environment variables.
//!
//! | Env var                          | Default                |
//! |----------------------------------|------------------------|
//! | `COORDINATOR_ADDR`               | `0.0.0.0:8080`         |
//! | `POSTGRES_SERVICE_ADDR`          | `http://[::1]:50051`   |
//! | `INFLUXDB_SERVICE_ADDR`          | `http://[::1]:50052`   |

mod handlers;
mod models;
mod secrets;

use std::sync::Arc;

use anyhow::Result;
use axum::{
    routing::{delete, get, post},
    Router,
};
use proto::{
    influxdb_service::influx_db_service_client::InfluxDbServiceClient,
    postgres_service::postgres_service_client::PostgresServiceClient,
};
use tonic::transport::Channel;
use tower_http::trace::TraceLayer;
use tracing::info;

// ------------------------------------------------------------------ //
//  Shared application state                                           //
// ------------------------------------------------------------------ //

/// Shared state injected into every Axum handler via `State`.
pub struct AppState {
    /// gRPC client stub for the PostgreSQL service.
    pub pg_client: PostgresServiceClient<Channel>,
    /// gRPC client stub for the InfluxDB service.
    pub influx_client: InfluxDbServiceClient<Channel>,
    /// Direct Postgres connection pool for dashboard queries (optional).
    pub db_pool: Option<sqlx::PgPool>,
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
                .add_directive("coordinator=info".parse()?),
        )
        .json()
        .init();

    // Resolve downstream service addresses (Bitwarden → env fallback).
    let pg_addr = secrets::get_secret(
        &std::env::var("BWS_POSTGRES_SERVICE_ADDR_ID")
            .unwrap_or_else(|_| "postgres-service-addr".to_string()),
        "POSTGRES_SERVICE_ADDR",
    )
    .await
    .unwrap_or_else(|_| "http://[::1]:50051".to_string());

    let influx_addr = secrets::get_secret(
        &std::env::var("BWS_INFLUXDB_SERVICE_ADDR_ID")
            .unwrap_or_else(|_| "influxdb-service-addr".to_string()),
        "INFLUXDB_SERVICE_ADDR",
    )
    .await
    .unwrap_or_else(|_| "http://[::1]:50052".to_string());

    info!(pg_addr, influx_addr, "connecting to backend services");

    let pg_channel = Channel::from_shared(pg_addr)?.connect_lazy();
    let influx_channel = Channel::from_shared(influx_addr)?.connect_lazy();

    // Optionally connect directly to Postgres for dashboard queries.
    let db_pool = match std::env::var("DATABASE_URL").ok() {
        Some(url) => {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await
                .ok();
            if pool.is_some() {
                info!("Dashboard Postgres pool connected");
            }
            pool
        }
        None => None,
    };

    let state = Arc::new(AppState {
        pg_client: PostgresServiceClient::new(pg_channel),
        influx_client: InfluxDbServiceClient::new(influx_channel),
        db_pool,
    });

    let app = Router::new()
        // Health check
        .route("/health", get(handlers::health))
        // Combined data endpoint (structured + time-series in one request)
        .route("/data", post(handlers::post_data))
        // Structured (PostgreSQL) CRUD
        .route(
            "/data/structured/:table",
            get(handlers::list_structured),
        )
        .route(
            "/data/structured/:table/:id",
            get(handlers::get_structured)
                .put(handlers::update_structured)
                .delete(handlers::delete_structured),
        )
        // Time-series (InfluxDB) endpoints
        .route("/data/timeseries/query", post(handlers::query_timeseries))
        .route("/data/timeseries", delete(handlers::delete_timeseries))
        // Dashboard endpoints
        .route("/dashboard/attention", get(handlers::dashboard_attention))
        .route("/dashboard/ticker", get(handlers::dashboard_ticker))
        .route("/dashboard/edges", get(handlers::dashboard_edges))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let bind_addr = std::env::var("COORDINATOR_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!(addr = bind_addr, "coordinator listening");

    axum::serve(listener, app).await?;

    Ok(())
}
