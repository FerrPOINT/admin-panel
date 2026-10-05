-- Products include this SQL in an immutable migration of their own database.
CREATE TABLE messaging_inbox (
    handler_id TEXT NOT NULL CHECK (length(handler_id) BETWEEN 1 AND 160),
    source TEXT NOT NULL CHECK (length(source) BETWEEN 1 AND 64),
    event_id UUID NOT NULL,
    fingerprint BYTEA NOT NULL CHECK (octet_length(fingerprint) = 32),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'processed', 'quarantined')),
    business_failures INTEGER NOT NULL DEFAULT 0 CHECK (business_failures BETWEEN 0 AND 8),
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ,
    PRIMARY KEY (handler_id, source, event_id)
);

CREATE INDEX messaging_inbox_retention ON messaging_inbox (updated_at)
    WHERE state IN ('processed', 'quarantined');

CREATE TABLE messaging_quarantine (
    id UUID PRIMARY KEY,
    handler_id TEXT NOT NULL CHECK (length(handler_id) BETWEEN 1 AND 160),
    source TEXT NOT NULL CHECK (length(source) BETWEEN 1 AND 64),
    event_id UUID,
    stream TEXT NOT NULL CHECK (length(stream) BETWEEN 1 AND 160),
    stream_sequence BIGINT NOT NULL CHECK (stream_sequence > 0),
    fingerprint BYTEA NOT NULL CHECK (octet_length(fingerprint) = 32),
    reason_code TEXT NOT NULL CHECK (reason_code IN (
        'invalid_schema', 'unsupported_contract', 'oversized', 'identity_conflict', 'handler_exhausted'
    )),
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (handler_id, stream, stream_sequence)
);

CREATE INDEX messaging_quarantine_retention ON messaging_quarantine (observed_at);

CREATE TABLE platform_events (
    id UUID PRIMARY KEY,
    source TEXT NOT NULL CHECK (source = 'ci-cd'),
    event_type TEXT NOT NULL CHECK (event_type = 'platform.cicd.pipeline.finished.v1'),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    occurred_at TIMESTAMPTZ NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    correlation_id UUID NOT NULL,
    project_id UUID NOT NULL,
    pipeline_id UUID NOT NULL,
    completion_seq BIGINT NOT NULL CHECK (completion_seq > 0),
    status TEXT NOT NULL CHECK (status IN ('success','failed','canceled')),
    finished_at TIMESTAMPTZ NOT NULL,
    UNIQUE (source, pipeline_id, completion_seq)
);
CREATE INDEX platform_events_feed ON platform_events (received_at DESC, id DESC);
CREATE INDEX platform_events_status_feed ON platform_events (status, received_at DESC, id DESC);
CREATE INDEX platform_events_correlation_feed ON platform_events (correlation_id, received_at DESC, id DESC);
CREATE INDEX platform_events_occurred ON platform_events (occurred_at);
