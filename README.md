# Rust API Monorepo

A Cargo workspace containing three Rust microservices that communicate internally via **gRPC / protobuf**:

| Service | Role | Default port |
|---------|------|--------------|
| **coordinator** | HTTP gateway — receives client requests and fans them out to the backend services | `8080` |
| **postgres-service** | gRPC service — structured CRUD against PostgreSQL | `50051` |
| **influxdb-service** | gRPC service — time-series CRUD against InfluxDB 2.x | `50052` |

---

## Architecture

```
Client (HTTP/JSON)
       │
       ▼
  coordinator          ← Axum HTTP API
  ├── POST /data        (structured + time-series, or either alone)
  ├── GET/PUT/DELETE /data/structured/:table/:id
  ├── POST /data/timeseries/query
  └── DELETE /data/timeseries
       │                     │
       │ gRPC (protobuf)      │ gRPC (protobuf)
       ▼                     ▼
postgres-service       influxdb-service
  (SQLx + PostgreSQL)   (influxdb2 + InfluxDB 2.x)
```

Internal service calls are serialised as **protobuf messages** over gRPC (tonic / prost).

---

## Workspace layout

```
.
├── Cargo.toml              # workspace manifest
├── protos/
│   ├── postgres_service.proto
│   └── influxdb_service.proto
├── proto/                  # shared crate: compiled gRPC stubs
├── coordinator/            # HTTP gateway
├── postgres-service/       # PostgreSQL CRUD gRPC service
└── influxdb-service/       # InfluxDB time-series gRPC service
```

---

## Secrets management (Bitwarden)

Every service resolves its secrets through **Bitwarden Secrets Manager** when a
`BWS_ACCESS_TOKEN` machine-account token is present in the environment.  If the
token is absent the services fall back to plain environment variables, which is
convenient for local development.

### Required secrets

#### postgres-service
| Env var (fallback) | BWS secret-ID env var |
|--------------------|-----------------------|
| `DATABASE_URL` | `BWS_POSTGRES_DATABASE_URL_ID` |

#### influxdb-service
| Env var (fallback) | BWS secret-ID env var |
|--------------------|-----------------------|
| `INFLUXDB_URL` | `BWS_INFLUXDB_URL_ID` |
| `INFLUXDB_TOKEN` | `BWS_INFLUXDB_TOKEN_ID` |
| `INFLUXDB_ORG` | `BWS_INFLUXDB_ORG_ID` |
| `INFLUXDB_BUCKET` | `BWS_INFLUXDB_BUCKET_ID` |

#### coordinator
| Env var (fallback) | BWS secret-ID env var |
|--------------------|-----------------------|
| `POSTGRES_SERVICE_ADDR` | `BWS_POSTGRES_SERVICE_ADDR_ID` |
| `INFLUXDB_SERVICE_ADDR` | `BWS_INFLUXDB_SERVICE_ADDR_ID` |

Set `BWS_API_URL` to override the Bitwarden API base URL (defaults to
`https://api.bitwarden.com`).

---

## Quick start (local development)

### 1. Prerequisites

- Rust 1.75+
- `protoc` (Protocol Buffer compiler)
- A running PostgreSQL instance
- A running InfluxDB 2.x instance

### 2. Configure environment

Copy and edit the example env files:

```bash
cp postgres-service/.env.example postgres-service/.env
cp influxdb-service/.env.example influxdb-service/.env
cp coordinator/.env.example coordinator/.env
```

### 3. Run services

```bash
# Terminal 1 — postgres-service
cargo run -p postgres-service

# Terminal 2 — influxdb-service
cargo run -p influxdb-service

# Terminal 3 — coordinator
cargo run -p coordinator
```

### 4. Test the coordinator

```bash
# Write structured + time-series data in one request
curl -X POST http://localhost:8080/data \
  -H 'Content-Type: application/json' \
  -d '{
    "structured": [
      {"table": "events", "payload": {"name": "login", "user": "alice"}}
    ],
    "timeseries": [
      {
        "measurement": "cpu",
        "tags": {"host": "web-01"},
        "fields": {"usage": 42.5},
        "timestamp_ns": 0
      }
    ]
  }'

# Read a structured record
curl http://localhost:8080/data/structured/events/<id>

# Query time-series data
curl -X POST http://localhost:8080/data/timeseries/query \
  -H 'Content-Type: application/json' \
  -d '{
    "measurement": "cpu",
    "start": "2024-01-01T00:00:00Z",
    "stop": "2025-01-01T00:00:00Z"
  }'
```

---

## Building

```bash
cargo build --release
```

Individual services:

```bash
cargo build -p coordinator --release
cargo build -p postgres-service --release
cargo build -p influxdb-service --release
```

---

## Proto definitions

Proto files live in `protos/`.  The `proto` crate's `build.rs` compiles them
at build time using `tonic-build` + `prost-build`.  Re-run `cargo build` after
editing any `.proto` file.

