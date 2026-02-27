# postgres-service

gRPC structured CRUD service backed by PostgreSQL.

## What it does

- Serves create/read/list/update/delete RPCs.
- Uses SQLx against PostgreSQL.
- Runs DB migrations from `db/migrations/` on startup.

## Default address

- `POSTGRES_SERVICE_ADDR=[::1]:50051`

## Key environment variables

- `POSTGRES_SERVICE_ADDR` (default `[::1]:50051`)
- `DATABASE_URL` (required unless resolved via Bitwarden)
- `BWS_POSTGRES_DATABASE_URL_ID` (optional Bitwarden secret-id env var)

## Run

```bash
cargo run -p postgres-service
```
