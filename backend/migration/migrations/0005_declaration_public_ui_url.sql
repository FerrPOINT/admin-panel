-- 0005: public UI URL for service declarations (ADR-0007).
-- Optional per-declaration URL shown to users by the service switcher;
-- NULL keeps the previous behavior (ui_url = integration_base_url).
ALTER TABLE service_declarations
    ADD COLUMN IF NOT EXISTS public_ui_url TEXT NULL;
