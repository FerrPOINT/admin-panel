-- Additive messaging maintenance; 0006 remains immutable.
CREATE TABLE messaging_maintenance_state (
    scope TEXT PRIMARY KEY,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_attempt_at TIMESTAMPTZ NOT NULL,
    last_success_at TIMESTAMPTZ,
    last_result JSONB,
    last_error_code TEXT CHECK (last_error_code IS NULL OR last_error_code IN ('storage_unavailable','operation_timeout'))
);
CREATE INDEX messaging_inbox_scoped_retention ON messaging_inbox(handler_id,source,updated_at,event_id) WHERE state IN ('processed','quarantined');
CREATE INDEX messaging_quarantine_scoped_retention ON messaging_quarantine(handler_id,source,observed_at,id);
