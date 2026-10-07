-- Remember saved context budgets independently for each provider/model.
CREATE TABLE ai_model_contexts (
    workspace text NOT NULL,
    provider text NOT NULL,
    model text NOT NULL CHECK (length(model) BETWEEN 1 AND 256 AND model = btrim(model)),
    context_window_tokens bigint NOT NULL DEFAULT 256000 CHECK (context_window_tokens BETWEEN 64000 AND 4294967295),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace,provider,model),
    FOREIGN KEY (workspace,provider) REFERENCES ai_provider_settings(workspace,provider)
);
INSERT INTO ai_model_contexts(workspace,provider,model,context_window_tokens)
    SELECT workspace,provider,model,context_window_tokens FROM ai_provider_settings;
