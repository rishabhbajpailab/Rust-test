//! Compiled protobuf / gRPC types shared across the monorepo.
//!
//! All client and server stubs are generated at build time from the
//! `.proto` files in the workspace-level `protos/` directory.

/// gRPC types and stubs for the PostgreSQL CRUD service.
pub mod postgres_service {
    tonic::include_proto!("postgres_service");
}

/// gRPC types and stubs for the InfluxDB time-series service.
pub mod influxdb_service {
    tonic::include_proto!("influxdb_service");
}

/// gRPC types and stubs for the supervisor service.
pub mod supervisor_service {
    tonic::include_proto!("supervisor_service");
}
