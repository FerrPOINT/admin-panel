-- An immutable profile is a candidate until runtime registration is acknowledged.
ALTER TABLE ai_profile_revisions ADD CONSTRAINT ai_profile_operation_binding UNIQUE(workspace,revision,operation_id);
CREATE TABLE ai_publication_outbox (
    operation_id uuid PRIMARY KEY REFERENCES ai_profile_revisions(operation_id),
    workspace text NOT NULL,
    revision bigint NOT NULL,
    expected_revision bigint NOT NULL CHECK (expected_revision >= 0),
    expected_draft_revision bigint NOT NULL CHECK (expected_draft_revision > 0),
    evidence jsonb NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    adapter_version text NOT NULL CHECK (length(adapter_version) BETWEEN 1 AND 256),
    accounting_policy text NOT NULL CHECK (length(accounting_policy) BETWEEN 1 AND 256),
    state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','published','rejected')),
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    FOREIGN KEY (workspace,revision,operation_id) REFERENCES ai_profile_revisions(workspace,revision,operation_id),
    CHECK ((state = 'pending') = (completed_at IS NULL))
);
CREATE UNIQUE INDEX ai_publication_one_pending ON ai_publication_outbox(workspace) WHERE state = 'pending';
CREATE FUNCTION ai_publication_payload_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN RAISE EXCEPTION 'ai_publication_payload_immutable'; END IF;
    IF (to_jsonb(NEW) - ARRAY['state','completed_at']) IS DISTINCT FROM (to_jsonb(OLD) - ARRAY['state','completed_at'])
       OR OLD.state <> 'pending' OR NEW.state = 'pending' THEN
        RAISE EXCEPTION 'ai_publication_payload_immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER ai_publication_payload_immutable BEFORE UPDATE OR DELETE ON ai_publication_outbox
    FOR EACH ROW EXECUTE FUNCTION ai_publication_payload_immutable();
