-- 0003: health probe results for registry declarations.
-- Stores the outcome of the background health.read capability check
-- performed against each service's declared integration base URL.
ALTER TABLE service_registry_entries
    ADD COLUMN IF NOT EXISTS health_status TEXT,
    ADD COLUMN IF NOT EXISTS health_checked_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS health_detail TEXT;

COMMENT ON COLUMN service_registry_entries.health_status IS
    'last probe outcome: healthy | unreachable | unknown (never probed)';
COMMENT ON COLUMN service_registry_entries.health_checked_at IS
    'when the last health probe ran';
COMMENT ON COLUMN service_registry_entries.health_detail IS
    'human-readable probe detail, e.g. HTTP status or transport error';
