# Rust API Monorepo

A Cargo workspace for a multi-service plant telemetry stack built in Rust.

## Services

| Crate | Type | Purpose | Default bind |
|---|---|---|---|
| `coordinator` | HTTP API gateway (Axum) | Client-facing API, fans out requests to backend gRPC services | `0.0.0.0:8080` |
| `postgres-service` | gRPC service (tonic) | Structured CRUD backed by PostgreSQL | `[::1]:50051` |
| `influxdb-service` | gRPC service (tonic) | Time-series write/query/delete backed by InfluxDB 2.x | `[::1]:50052` |
| `database-supervisor` | gRPC service (tonic) | Telemetry ingestion + threshold/status processing + optional RabbitMQ publishing | `[::1]:50053` |
| `event-router` | UDP ingest daemon | Decodes ESP32-S3 telemetry and forwards batched envelopes to `database-supervisor` | `0.0.0.0:7000` |
| `proto` | Shared library crate | Compiled protobuf/gRPC types and client/server stubs used by all services | n/a |

## Repository layout

```text
.
├── Cargo.toml
├── Makefile
├── protos/                # protobuf definitions
├── proto/                 # generated protobuf/gRPC Rust crate
├── coordinator/           # HTTP gateway
├── postgres-service/      # PostgreSQL CRUD service
├── influxdb-service/      # InfluxDB time-series service
├── database-supervisor/   # telemetry supervisor service
└── event-router/          # UDP ingest + forwarder
```

## Per-service documentation

Each service now has a local README with service-specific setup and environment variables:

- [`coordinator/README.md`](coordinator/README.md)
- [`postgres-service/README.md`](postgres-service/README.md)
- [`influxdb-service/README.md`](influxdb-service/README.md)
- [`database-supervisor/README.md`](database-supervisor/README.md)
- [`event-router/README.md`](event-router/README.md)

## Build and release workflows

A root [`Makefile`](Makefile) provides repeatable release builds for x86_64 and ARM64, including a Raspberry Pi 5 optimized ARM profile.

### Prerequisites

- Rust toolchain via `rustup`
- `protoc` installed
- For ARM cross-compile on x86 hosts:
  - target installed: `rustup target add aarch64-unknown-linux-gnu`
  - cross-linker (example): `aarch64-linux-gnu-gcc`

### Common commands

```bash
# Show all build helpers
make help

# Rebuild generated protobuf/gRPC code (proto crate only)
make proto

# Host release build (all workspace packages)
make build

# Explicit x86_64 release build
make build-x86

# ARM64 release build (generic)
make build-arm

# ARM64 release build optimized for Raspberry Pi 5 (Cortex-A76)
make build-arm-pi5

# Build all targets
make build-all
```

The Makefile sets `CARGO_INCREMENTAL=1` for build targets to speed up iterative local builds.

### Raspberry Pi 5 optimization profile

`make build-arm-pi5` applies known Rust/LLVM flags suited for Pi 5's Cortex-A76 CPU:

- `-C target-cpu=cortex-a76`
- `-C target-feature=+neon,+crc,+crypto`
- `-C lto=thin`
- `-C codegen-units=1`

This is a strong default for throughput-focused release artifacts on Pi 5 8GB systems.

## Running services locally

Run each service in its own shell:

```bash
cargo run -p postgres-service
cargo run -p influxdb-service
cargo run -p database-supervisor
cargo run -p event-router
cargo run -p coordinator
```

## Notes

- `coordinator` can talk to backend services over gRPC using configured service addresses.
- Secret resolution for some services supports Bitwarden Secrets Manager with environment fallback.
- Protobuf definitions live in `protos/` and are compiled by the `proto` crate at build time.
