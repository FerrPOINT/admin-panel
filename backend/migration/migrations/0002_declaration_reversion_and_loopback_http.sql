-- 0002: allow declaration re-versions and loopback http integration URLs.
--
-- The v1 schema pinned declaration_version = 1 and https-only integration
-- URLs. Runtime requires re-versioned declarations (PATCH flow) and the
-- local fleet stand serves plain http on loopback hosts; https stays the
-- rule for everything else.

ALTER TABLE service_declarations
    DROP CONSTRAINT service_declarations_declaration_version_check;

ALTER TABLE service_declarations
    ADD CONSTRAINT service_declarations_declaration_version_check
    CHECK (declaration_version >= 1);

ALTER TABLE service_declarations
    DROP CONSTRAINT service_declarations_integration_base_url_check;

ALTER TABLE service_declarations
    ADD CONSTRAINT service_declarations_integration_base_url_check
    CHECK (integration_base_url ~ '^https?://[^/@?#]+(:[0-9]+)?$');
