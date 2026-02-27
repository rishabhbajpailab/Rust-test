# influxdb-service

gRPC time-series service backed by InfluxDB 2.x.

## What it does

- Accepts time-series point writes.
- Queries time-series ranges.
- Deletes ranges with optional tag predicates.

## Default address

- `INFLUXDB_SERVICE_ADDR=[::1]:50052`

## Key environment variables

- `INFLUXDB_SERVICE_ADDR` (default `[::1]:50052`)
- `INFLUXDB_URL`
- `INFLUXDB_TOKEN`
- `INFLUXDB_ORG`
- `INFLUXDB_BUCKET`

Optional Bitwarden secret-id env vars:

- `BWS_INFLUXDB_URL_ID`
- `BWS_INFLUXDB_TOKEN_ID`
- `BWS_INFLUXDB_ORG_ID`
- `BWS_INFLUXDB_BUCKET_ID`

## Run

```bash
cargo run -p influxdb-service
```
