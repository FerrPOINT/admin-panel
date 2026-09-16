-- Catalog v1.1: UI marker capability. Services whose active declaration
-- carries `ui.render` expose a user-facing frontend; the runtime catalog
-- publishes ui_url for them and null for API-only services (java-agent).

INSERT INTO capability_catalog (key, description, fixed_method, fixed_path) VALUES
    ('ui.render', 'Service exposes a user-facing UI at the integration base URL', 'GET', '/')
ON CONFLICT (key) DO NOTHING;

-- Data migration: mark the fleet services that ship a frontend UI today.
-- New services declare `ui.render` in their own declaration going forward.
UPDATE service_declarations d
SET capabilities = d.capabilities || '["ui.render"]'::jsonb
FROM service_registry_entries e
WHERE d.id = e.active_declaration_id
  AND e.service_key IN ('admin-panel', 'ci-cd', 'fleet-control', 'project-workflow', 'task-tracker', 'wiki')
  AND NOT (d.capabilities ? 'ui.render');
