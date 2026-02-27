//! Event Router service — UDP telemetry ingestion and forwarding.
//!
//! Listens for UDP packets from ESP32-S3 devices, decodes them (JSON),
//! assigns a stable ingest_id, and forwards batches to the Database Supervisor
//! via gRPC.
//!
//! # Environment variables
//! | Var                  | Default              |
//! |----------------------|----------------------|
//! | `ROUTER_UDP_ADDR`    | `0.0.0.0:7000`       |
//! | `SUPERVISOR_ADDR`    | `http://[::1]:50053` |
//! | `ROUTER_BATCH_SIZE`  | `64`                 |

use std::mem;
use std::sync::Arc;

use anyhow::Result;
use proto::supervisor_service::{
    supervisor_service_client::SupervisorServiceClient, IngestTelemetryRequest, TelemetryEnvelope,
};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tonic::transport::Channel;
use tracing::{error, info, warn};

mod codec;
mod ingest_id;

const MAX_PACKET_SIZE: usize = 4096;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("event_router=info".parse()?),
        )
        .json()
        .init();

    let udp_addr = std::env::var("ROUTER_UDP_ADDR").unwrap_or_else(|_| "0.0.0.0:7000".to_string());
    let supervisor_addr =
        std::env::var("SUPERVISOR_ADDR").unwrap_or_else(|_| "http://[::1]:50053".to_string());
    let batch_size: usize = std::env::var("ROUTER_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64);

    let socket = Arc::new(UdpSocket::bind(&udp_addr).await?);
    info!(addr = udp_addr, "UDP listener bound");

    let channel = Channel::from_shared(supervisor_addr)?.connect_lazy();
    let client = SupervisorServiceClient::new(channel);

    let (tx, rx) = mpsc::channel::<TelemetryEnvelope>(1024);

    tokio::spawn(batch_sender(rx, client, batch_size));

    let mut buf = vec![0u8; MAX_PACKET_SIZE];
    loop {
        let (len, peer) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "UDP recv_from error");
                continue;
            }
        };

        let bytes = &buf[..len];

        match codec::decode(bytes) {
            Ok(msg) => {
                let id =
                    ingest_id::compute(&msg.device_uid, &msg.plant_id, msg.seq, msg.timestamp_ns);

                let envelope = TelemetryEnvelope {
                    ingest_id: id,
                    device_uid: msg.device_uid,
                    plant_id: msg.plant_id,
                    timestamp_ns: msg.timestamp_ns,
                    seq: msg.seq,
                    soil_moisture: msg.soil_moisture,
                    ambient_light_lux: msg.ambient_light_lux,
                    ambient_humidity_rh: msg.ambient_humidity_rh,
                    ambient_temp_c: msg.ambient_temp_c,
                };

                if let Err(e) = tx.try_send(envelope) {
                    warn!(peer = %peer, error = %e, "envelope channel full, dropping packet");
                }
            }
            Err(e) => {
                warn!(peer = %peer, error = %e, "decode error");
            }
        }
    }
}

async fn batch_sender(
    mut rx: mpsc::Receiver<TelemetryEnvelope>,
    mut client: SupervisorServiceClient<Channel>,
    batch_size: usize,
) {
    let mut batch = Vec::with_capacity(batch_size);

    loop {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(100);

        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Some(env)) => {
                    batch.push(env);
                    if batch.len() >= batch_size {
                        break;
                    }
                }
                Ok(None) => return,
                Err(_) => break,
            }
        }

        if batch.is_empty() {
            continue;
        }

        let req = IngestTelemetryRequest {
            envelopes: mem::take(&mut batch),
        };
        match client.ingest_telemetry(req).await {
            Ok(resp) => {
                let inner = resp.into_inner();
                info!(
                    sent = inner.results.len(),
                    changes = inner.status_changes.len(),
                    "batch forwarded"
                );
            }
            Err(e) => {
                error!(error = %e, "gRPC IngestTelemetry failed");
            }
        }
        batch = Vec::with_capacity(batch_size);
    }
}
