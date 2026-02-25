-- Plant type definitions
CREATE TABLE IF NOT EXISTS plant_type (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Per-metric thresholds for each plant type
CREATE TABLE IF NOT EXISTS plant_type_metric_threshold (
    plant_type_id UUID    NOT NULL REFERENCES plant_type(id) ON DELETE CASCADE,
    metric        TEXT    NOT NULL,
    warn_min      DOUBLE PRECISION,
    warn_max      DOUBLE PRECISION,
    crit_min      DOUBLE PRECISION,
    crit_max      DOUBLE PRECISION,
    unit          TEXT,
    PRIMARY KEY (plant_type_id, metric)
);

-- Physical plants
CREATE TABLE IF NOT EXISTS plant (
    id           UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    plant_type_id UUID   NOT NULL REFERENCES plant_type(id),
    display_name TEXT    NOT NULL,
    location     TEXT,
    notes        TEXT,
    is_active    BOOLEAN NOT NULL DEFAULT TRUE,
    device_id    UUID,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- IoT edge devices
CREATE TABLE IF NOT EXISTS device (
    id               UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    device_uid       TEXT    NOT NULL UNIQUE,
    firmware_version TEXT,
    last_seen_at     TIMESTAMPTZ,
    last_ingest_id   TEXT,
    is_active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Latest snapshot of plant sensor readings
CREATE TABLE IF NOT EXISTS plant_current_state (
    plant_id             UUID    PRIMARY KEY REFERENCES plant(id) ON DELETE CASCADE,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_ingest_id       TEXT,
    severity             TEXT    NOT NULL DEFAULT 'NORMAL',  -- NORMAL | WARN | CRITICAL
    soil_moisture        DOUBLE PRECISION,
    ambient_light_lux    DOUBLE PRECISION,
    ambient_humidity_rh  DOUBLE PRECISION,
    ambient_temp_c       DOUBLE PRECISION,
    metric_severity      JSONB
);

-- Append-only ticker of plant events
CREATE TABLE IF NOT EXISTS ticker_event (
    id          BIGSERIAL   PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    plant_id    UUID        REFERENCES plant(id),
    device_uid  TEXT,
    severity    TEXT        NOT NULL,
    message     TEXT        NOT NULL,
    payload     JSONB
);

-- Deduplication ledger for telemetry ingestion
CREATE TABLE IF NOT EXISTS telemetry_ingest_ledger (
    ingest_id    TEXT        PRIMARY KEY,
    device_uid   TEXT        NOT NULL,
    plant_id     UUID,
    received_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    timestamp_ns BIGINT      NOT NULL,
    payload_hash TEXT,
    result       TEXT        NOT NULL  -- OK | DUPLICATE | ERROR
);

-- Indexes for dashboard queries
CREATE INDEX IF NOT EXISTS idx_plant_current_state_severity_updated
    ON plant_current_state(severity, updated_at);

CREATE INDEX IF NOT EXISTS idx_ticker_event_occurred_at
    ON ticker_event(occurred_at DESC);

CREATE INDEX IF NOT EXISTS idx_device_last_seen_at
    ON device(last_seen_at DESC);
