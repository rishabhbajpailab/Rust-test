# database-supervisor

gRPC backend service for telemetry ingest and supervision.

## What it does

- Accepts telemetry envelopes from `event-router`.
- Writes/forwards telemetry via a sink implementation.
- Evaluates threshold/status behavior and can publish updates to RabbitMQ.

## Default address

- `SUPERVISOR_ADDR=[::1]:50053`

## Key environment variables

- `DATABASE_URL` (required)
- `SUPERVISOR_ADDR` (default `[::1]:50053`)
- `INFLUXDB_URL` (optional)
- `INFLUXDB_ORG` (optional)
- `INFLUXDB_TOKEN` (optional)
- `INFLUXDB_BUCKET` (optional)
- `AMQP_URL` (optional)

If Influx env vars are missing, the service falls back to an internal fake telemetry sink.

## Run

```bash
cargo run -p database-supervisor
```
