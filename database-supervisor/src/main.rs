//! Database Supervisor service entry point.
//!
//! # Environment variables
//! | Var                         | Default              |
//! |-----------------------------|----------------------|
//! | `DATABASE_URL`              | required             |
//! | `SUPERVISOR_ADDR`           | `[::1]:50053`        |
//! | `INFLUXDB_URL`              | optional             |
//! | `INFLUXDB_ORG`              | optional             |
//! | `INFLUXDB_TOKEN`            | optional             |
//! | `INFLUXDB_BUCKET`           | optional             |
//! | `AMQP_URL`                  | optional             |

use std::sync::Arc;

use anyhow::Result;
use proto::supervisor_service::supervisor_service_server::SupervisorServiceServer;
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing::info;

use database_supervisor::ingest::SupervisorServiceImpl;
use database_supervisor::telemetry_sink::{FakeTelemetrySink, InfluxTelemetrySink, TelemetrySink};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("database_supervisor=info".parse()?),
        )
        .json()
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    // Optionally connect to InfluxDB
    let sink: Arc<dyn TelemetrySink> = match (
        std::env::var("INFLUXDB_URL").ok(),
        std::env::var("INFLUXDB_ORG").ok(),
        std::env::var("INFLUXDB_TOKEN").ok(),
        std::env::var("INFLUXDB_BUCKET").ok(),
    ) {
        (Some(url), Some(org), Some(token), Some(bucket)) => {
            info!("Using InfluxTelemetrySink");
            Arc::new(InfluxTelemetrySink::new(&url, &org, &token, &bucket))
        }
        _ => {
            info!("No InfluxDB config; using FakeTelemetrySink");
            Arc::new(FakeTelemetrySink::new())
        }
    };

    // Optionally connect to RabbitMQ
    let amqp_chan = match std::env::var("AMQP_URL").ok() {
        Some(url) => {
            let conn = lapin::Connection::connect(
                &url,
                lapin::ConnectionProperties::default(),
            )
            .await?;
            let chan = conn.create_channel().await?;
            chan.queue_declare(
                "plant.status_change",
                lapin::options::QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await?;
            chan.queue_declare(
                "plant.ticker_update",
                lapin::options::QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await?;
            info!("RabbitMQ channel ready");
            Some(chan)
        }
        None => {
            info!("No AMQP_URL; RabbitMQ publishing disabled");
            None
        }
    };

    let addr = std::env::var("SUPERVISOR_ADDR")
        .unwrap_or_else(|_| "[::1]:50053".to_string())
        .parse()?;

    let svc = SupervisorServiceImpl::new(pool, sink, amqp_chan);

    info!(%addr, "database-supervisor listening");

    Server::builder()
        .add_service(SupervisorServiceServer::new(svc))
        .serve(addr)
        .await?;

    Ok(())
}
