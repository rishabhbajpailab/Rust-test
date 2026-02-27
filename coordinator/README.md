# coordinator

HTTP gateway service built with Axum.

## What it does

- Exposes client-facing HTTP/JSON endpoints.
- Calls `postgres-service` and `influxdb-service` over gRPC.
- Optionally opens a direct PostgreSQL pool for dashboard endpoints.

## Default address

- `COORDINATOR_ADDR=0.0.0.0:8080`

## Key environment variables

- `COORDINATOR_ADDR` (default `0.0.0.0:8080`)
- `POSTGRES_SERVICE_ADDR` (default `http://[::1]:50051`)
- `INFLUXDB_SERVICE_ADDR` (default `http://[::1]:50052`)
- `DATABASE_URL` (optional, enables direct dashboard DB queries)

Bitwarden-backed resolution is supported for service address values:

- `BWS_POSTGRES_SERVICE_ADDR_ID`
- `BWS_INFLUXDB_SERVICE_ADDR_ID`

## Run

```bash
cargo run -p coordinator
```
