# event-router

UDP telemetry ingest service.

## What it does

- Listens for UDP packets from edge devices.
- Decodes telemetry payloads.
- Computes stable `ingest_id` values.
- Batches and forwards telemetry to `database-supervisor` over gRPC.

## Default addresses

- `ROUTER_UDP_ADDR=0.0.0.0:7000`
- `SUPERVISOR_ADDR=http://[::1]:50053`

## Key environment variables

- `ROUTER_UDP_ADDR` (default `0.0.0.0:7000`)
- `SUPERVISOR_ADDR` (default `http://[::1]:50053`)
- `ROUTER_BATCH_SIZE` (default `64`)

## Run

```bash
cargo run -p event-router
```
