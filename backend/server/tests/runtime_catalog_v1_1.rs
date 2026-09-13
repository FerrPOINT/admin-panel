// Catalog v1.1: the public runtime catalog must expose per-service health
// (from the background health worker) and a ui_url distinction so that
// services without a UI (java-agent) are not dead links in switchers.
#[test]
fn runtime_catalog_source_exposes_health_and_ui_url() {
    let src = include_str!("../../api/src/lib.rs");

    // health_status from the registry entry is surfaced as `health`.
    assert!(
        src.contains("\"health\": entry"),
        "runtime_services must map entry.health_status into the public catalog"
    );
    // Unknown health (never probed) must serialize as the literal "unknown",
    // not null — switchers render an amber dot for it.
    assert!(
        src.contains("unwrap_or_else(|| \"unknown\".to_string())"),
        "absent health_status must degrade to \"unknown\""
    );
    // UI vs API distinction: capability ui.render marks services with a UI;
    // ui_url is null for API-only services so switchers can skip them.
    assert!(
        src.contains("\"ui_url\""),
        "runtime catalog entries must carry a ui_url field"
    );
}

#[test]
fn ui_url_logic_is_capability_driven() {
    let src = include_str!("../../api/src/lib.rs");
    // The ui_url must come from the integration_base_url when the service
    // declares a UI capability, and be null otherwise — no hardcoded ports.
    assert!(
        src.contains("ui.render"),
        "ui_url presence must be derived from the ui.render capability"
    );
    assert!(
        !src.contains("\"7761\"") && !src.contains("\"7751\""),
        "no hardcoded per-service ports in the catalog builder"
    );
}
