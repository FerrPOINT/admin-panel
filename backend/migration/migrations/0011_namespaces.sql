-- Registry metadata only. Product data remains in the owning database.
CREATE TABLE namespaces (
    id uuid PRIMARY KEY,
    registry_instance_id uuid NOT NULL,
    slug varchar(64) NOT NULL UNIQUE,
    name varchar(200) NOT NULL,
    description text NOT NULL DEFAULT '',
    responsible_subject varchar(255) NOT NULL,
    state text NOT NULL CHECK (state IN ('provisioning','active','archiving','archived','restoring')),
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE namespace_operations (
    id uuid PRIMARY KEY,
    namespace_id uuid NOT NULL REFERENCES namespaces(id) ON DELETE RESTRICT,
    actor_subject varchar(255) NOT NULL,
    command jsonb NOT NULL,
    state text NOT NULL CHECK (state IN ('pending','completed')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX namespace_one_pending_lifecycle ON namespace_operations(namespace_id)
    WHERE state = 'pending' AND command->>'action' IN ('archive','restore');
CREATE TABLE namespace_bindings (
    namespace_id uuid NOT NULL REFERENCES namespaces(id) ON DELETE RESTRICT,
    kind text NOT NULL CHECK (kind IN ('tracker_project','wiki_space','git_group')),
    resource_instance_id uuid NOT NULL,
    resource_id uuid NOT NULL,
    operation_id uuid NOT NULL REFERENCES namespace_operations(id) ON DELETE RESTRICT,
    generation bigint NOT NULL CHECK (generation > 0),
    desired_state text NOT NULL CHECK (desired_state IN ('active','archived')),
    confirmed boolean NOT NULL DEFAULT false,
    create_spec jsonb,
    last_error text,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(namespace_id,kind),
    UNIQUE(kind,resource_instance_id,resource_id)
);
CREATE INDEX namespace_operations_readback ON namespace_operations(namespace_id,created_at,id);
