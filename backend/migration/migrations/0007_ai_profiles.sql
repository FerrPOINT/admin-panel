-- Admin owns settings/publication only. Provider secrets belong to ai-runtime.
CREATE TABLE ai_provider_settings (
    workspace text NOT NULL CHECK (workspace IN ('sdlc1','sdlc2')),
    provider text NOT NULL CHECK (provider IN ('chatgpt','openrouter')),
    model text NOT NULL CHECK (length(model) BETWEEN 1 AND 256 AND model = btrim(model)),
    context_window_tokens bigint NOT NULL DEFAULT 256000 CHECK (context_window_tokens BETWEEN 64000 AND 4294967295),
    draft_revision bigint NOT NULL DEFAULT 1 CHECK (draft_revision > 0),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace, provider)
);

CREATE TABLE ai_profile_revisions (
    workspace text NOT NULL CHECK (workspace IN ('sdlc1','sdlc2')),
    revision bigint NOT NULL CHECK (revision > 0),
    profile jsonb NOT NULL CHECK (
        jsonb_typeof(profile) = 'object'
        AND profile - ARRAY['schema_version','revision','workspace','provider','model','context_window_tokens','verification_id']::text[] = '{}'::jsonb
        AND profile ?& ARRAY['schema_version','revision','workspace','provider','model','context_window_tokens','verification_id']::text[]
        AND profile->>'workspace' = workspace
        AND (profile->>'revision')::bigint = revision
        AND profile->>'schema_version' = '1'
    ),
    credential_generation uuid NOT NULL,
    operation_id uuid NOT NULL UNIQUE,
    published_by text NOT NULL,
    published_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace, revision)
);

CREATE TABLE ai_active_profile (
    workspace text PRIMARY KEY CHECK (workspace IN ('sdlc1','sdlc2')),
    revision bigint,
    FOREIGN KEY (workspace, revision) REFERENCES ai_profile_revisions(workspace, revision)
);

-- Published snapshots cannot be rewritten by application DML.
CREATE FUNCTION ai_profile_revision_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'ai_profile_revision_immutable';
END;
$$;
CREATE TRIGGER ai_profile_revision_immutable
    BEFORE UPDATE OR DELETE ON ai_profile_revisions
    FOR EACH ROW EXECUTE FUNCTION ai_profile_revision_immutable();

