-- Admin Panel v1 schema (docs/DATA_MODEL.md).

CREATE TABLE capability_catalog (
    key          varchar(80) PRIMARY KEY,
    description  text NOT NULL,
    fixed_method varchar(8)  NOT NULL,
    fixed_path   varchar(255) NOT NULL,
    is_active    boolean NOT NULL DEFAULT TRUE,
    created_at   timestamptz NOT NULL DEFAULT now()
);

INSERT INTO capability_catalog (key, description, fixed_method, fixed_path) VALUES
    ('health.read', 'Service availability probe', 'GET', '/health'),
    ('integration.status.read', 'Compact integration status', 'GET', '/integration/status'),
    ('branding.runtime.read', 'Consumes central branding runtime document', 'GET', '/branding/contract');

CREATE TABLE service_declarations (
    id                        uuid PRIMARY KEY,
    registry_entry_id         uuid NOT NULL,
    declaration_version       integer NOT NULL CHECK (declaration_version = 1),
    integration_base_url      text NOT NULL
        CHECK (integration_base_url ~ '^https://[^/@?#]+(:[0-9]+)?$'),
    capabilities              jsonb NOT NULL
        CHECK (jsonb_typeof(capabilities) = 'array'
           AND jsonb_array_length(capabilities) > 0),
    service_contract_version  varchar(64) NOT NULL,
    declared_by_subject       varchar(255) NOT NULL,
    declared_at               timestamptz NOT NULL DEFAULT now(),
    approval_status           varchar(16) NOT NULL DEFAULT 'pending'
        CHECK (approval_status IN ('pending','approved','rejected','superseded')),
    approved_by_subject       varchar(255),
    approved_at               timestamptz,
    rejection_reason          varchar(500),
    content_hash              char(64) NOT NULL
);

CREATE UNIQUE INDEX idx_declaration_content
    ON service_declarations (registry_entry_id, content_hash);
CREATE INDEX idx_declaration_entry_time
    ON service_declarations (registry_entry_id, declared_at DESC);

CREATE TABLE service_registry_entries (
    id                     uuid PRIMARY KEY,
    service_key            varchar(64) NOT NULL UNIQUE
        CHECK (service_key ~ '^[a-z][a-z0-9-]*$' AND service_key !~ '--' AND service_key !~ '-$'),
    display_name           varchar(160) NOT NULL,
    owner_team             varchar(160) NOT NULL,
    status                 varchar(16) NOT NULL
        CHECK (status IN ('pending','active','disabled','retired')),
    active_declaration_id  uuid,
    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz NOT NULL DEFAULT now(),
    version                bigint NOT NULL DEFAULT 1
);

ALTER TABLE service_registry_entries
    ADD CONSTRAINT fk_active_declaration
    FOREIGN KEY (active_declaration_id) REFERENCES service_declarations (id);

ALTER TABLE service_declarations
    ADD CONSTRAINT fk_declaration_entry
    FOREIGN KEY (registry_entry_id) REFERENCES service_registry_entries (id) ON DELETE RESTRICT;

CREATE TABLE service_check_runs (
    id                     uuid PRIMARY KEY,
    registry_entry_id      uuid NOT NULL
        REFERENCES service_registry_entries (id) ON DELETE RESTRICT,
    declaration_id         uuid NOT NULL
        REFERENCES service_declarations (id) ON DELETE RESTRICT,
    capability_key         varchar(80) NOT NULL
        REFERENCES capability_catalog (key),
    triggered_by_subject   varchar(255) NOT NULL,
    started_at             timestamptz NOT NULL,
    finished_at            timestamptz,
    outcome                varchar(16) NOT NULL
        CHECK (outcome IN ('success','unreachable','timeout','rejected','invalid_response','internal_error')),
    http_status            smallint,
    summary                varchar(500) NOT NULL,
    request_id             uuid NOT NULL
);

CREATE INDEX idx_checks_entry_time
    ON service_check_runs (registry_entry_id, started_at DESC);

CREATE TABLE branding_revisions (
    id                     uuid PRIMARY KEY,
    revision               bigint NOT NULL UNIQUE,
    state                  varchar(16) NOT NULL
        CHECK (state IN ('draft','published','superseded','withdrawn')),
    document               jsonb NOT NULL,
    document_hash          char(64) NOT NULL,
    etag                   varchar(128) NOT NULL UNIQUE,
    created_by_subject     varchar(255) NOT NULL,
    created_at             timestamptz NOT NULL DEFAULT now(),
    published_by_subject   varchar(255),
    published_at           timestamptz,
    based_on_revision      bigint
);

-- At most one published revision.
CREATE UNIQUE INDEX idx_branding_one_published
    ON branding_revisions (state) WHERE state = 'published';

CREATE TABLE role_bindings (
    id                  uuid PRIMARY KEY,
    claim_name          varchar(80) NOT NULL,
    claim_value         varchar(160) NOT NULL,
    panel_role          varchar(32) NOT NULL
        CHECK (panel_role IN ('platform_viewer','platform_operator','platform_admin')),
    created_by_subject  varchar(255) NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (claim_name, claim_value, panel_role)
);

CREATE TABLE audit_events (
    id              uuid PRIMARY KEY,
    occurred_at     timestamptz NOT NULL DEFAULT now(),
    request_id      uuid NOT NULL,
    actor_subject   varchar(255),
    actor_role      varchar(32),
    action          varchar(100) NOT NULL,
    entity_type     varchar(64) NOT NULL,
    entity_id       uuid,
    metadata        jsonb NOT NULL DEFAULT '{}'::jsonb,
    source_ip       inet
);

CREATE INDEX idx_audit_time ON audit_events (occurred_at DESC);
CREATE INDEX idx_audit_entity ON audit_events (entity_type, entity_id, occurred_at DESC);
CREATE INDEX idx_audit_actor ON audit_events (actor_subject, occurred_at DESC);
CREATE INDEX idx_audit_action ON audit_events (action, occurred_at DESC);
