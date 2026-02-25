//! PostgreSQL CRUD gRPC service.
//!
//! Listens on the address specified by `POSTGRES_SERVICE_ADDR` (default
//! `[::1]:50051`) and serves the `PostgresService` gRPC interface defined in
//! `protos/postgres_service.proto`.
//!
//! # Secrets
//! The `DATABASE_URL` is resolved via Bitwarden Secrets Manager
//! (`BWS_ACCESS_TOKEN` + `BWS_POSTGRES_DATABASE_URL_ID`) with a fallback to
//! the `DATABASE_URL` environment variable for local development.

mod db;
mod secrets;

use std::sync::Arc;

use anyhow::Result;
use proto::postgres_service::{
    postgres_service_server::{PostgresService, PostgresServiceServer},
    CreateRequest, CreateResponse, DeleteRequest, DeleteResponse, ListRequest, ListResponse,
    ReadRequest, ReadResponse, Record, UpdateRequest, UpdateResponse,
};
use tonic::{transport::Server, Request, Response, Status};
use tracing::{error, info};

// ------------------------------------------------------------------ //
//  gRPC service implementation                                        //
// ------------------------------------------------------------------ //

pub struct PostgresServiceImpl {
    db: Arc<db::Db>,
}

#[tonic::async_trait]
impl PostgresService for PostgresServiceImpl {
    async fn create(
        &self,
        request: Request<CreateRequest>,
    ) -> Result<Response<CreateResponse>, Status> {
        let req = request.into_inner();
        match self.db.create(&req.table_name, &req.payload).await {
            Ok(id) => Ok(Response::new(CreateResponse {
                id,
                success: true,
                error: String::new(),
            })),
            Err(e) => {
                error!(error = %e, "create failed");
                Ok(Response::new(CreateResponse {
                    id: String::new(),
                    success: false,
                    error: e.to_string(),
                }))
            }
        }
    }

    async fn read(
        &self,
        request: Request<ReadRequest>,
    ) -> Result<Response<ReadResponse>, Status> {
        let req = request.into_inner();
        match self.db.read(&req.id, &req.table_name).await {
            Ok(Some(row)) => Ok(Response::new(ReadResponse {
                record: Some(Record {
                    id: row.id,
                    table_name: row.table_name,
                    payload: row.payload,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                }),
                success: true,
                error: String::new(),
            })),
            Ok(None) => Ok(Response::new(ReadResponse {
                record: None,
                success: false,
                error: "record not found".to_string(),
            })),
            Err(e) => {
                error!(error = %e, "read failed");
                Ok(Response::new(ReadResponse {
                    record: None,
                    success: false,
                    error: e.to_string(),
                }))
            }
        }
    }

    async fn list(
        &self,
        request: Request<ListRequest>,
    ) -> Result<Response<ListResponse>, Status> {
        let req = request.into_inner();
        let limit = if req.limit == 0 { 100 } else { req.limit };
        match self.db.list(&req.table_name, &req.filter, limit, req.offset).await {
            Ok(rows) => Ok(Response::new(ListResponse {
                records: rows
                    .into_iter()
                    .map(|r| Record {
                        id: r.id,
                        table_name: r.table_name,
                        payload: r.payload,
                        created_at: r.created_at,
                        updated_at: r.updated_at,
                    })
                    .collect(),
                success: true,
                error: String::new(),
            })),
            Err(e) => {
                error!(error = %e, "list failed");
                Ok(Response::new(ListResponse {
                    records: vec![],
                    success: false,
                    error: e.to_string(),
                }))
            }
        }
    }

    async fn update(
        &self,
        request: Request<UpdateRequest>,
    ) -> Result<Response<UpdateResponse>, Status> {
        let req = request.into_inner();
        match self.db.update(&req.id, &req.table_name, &req.payload).await {
            Ok(found) => Ok(Response::new(UpdateResponse {
                success: found,
                error: if found {
                    String::new()
                } else {
                    "record not found".to_string()
                },
            })),
            Err(e) => {
                error!(error = %e, "update failed");
                Ok(Response::new(UpdateResponse {
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
        match self.db.delete(&req.id, &req.table_name).await {
            Ok(found) => Ok(Response::new(DeleteResponse {
                success: found,
                error: if found {
                    String::new()
                } else {
                    "record not found".to_string()
                },
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
    // Load .env for local development.
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("postgres_service=info".parse()?),
        )
        .json()
        .init();

    // Resolve DATABASE_URL via Bitwarden (or env fallback).
    let database_url = secrets::get_secret(
        &std::env::var("BWS_POSTGRES_DATABASE_URL_ID")
            .unwrap_or_else(|_| "postgres-database-url".to_string()),
        "DATABASE_URL",
    )
    .await?;

    let db = db::Db::connect(&database_url).await?;
    db.migrate().await?;

    let addr = std::env::var("POSTGRES_SERVICE_ADDR")
        .unwrap_or_else(|_| "[::1]:50051".to_string())
        .parse()?;

    let svc = PostgresServiceImpl { db: Arc::new(db) };

    info!(%addr, "postgres-service listening");

    Server::builder()
        .add_service(PostgresServiceServer::new(svc))
        .serve(addr)
        .await?;

    Ok(())
}
