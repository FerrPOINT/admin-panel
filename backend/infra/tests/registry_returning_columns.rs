// Unit regression: every RegistryEntryRow-backed UPDATE ... RETURNING list
// must include the health columns added by migration 0003, otherwise
// sqlx FromRow mapping fails at runtime ("no column found for name:
// health_status") on approve/status paths.
use admin_panel_infra::registry::RegistryStore;

#[test]
fn update_returning_lists_include_health_columns() {
    let src = include_str!("../src/registry.rs");
    for required in ["health_status, health_checked_at, health_detail"] {
        let returning_blocks: Vec<&str> = src
            .split("RETURNING")
            .skip(1)
            .map(|chunk| {
                let end = chunk.find(';').unwrap_or(chunk.len());
                &chunk[..end]
            })
            .collect();
        assert!(!returning_blocks.is_empty());
        for block in returning_blocks {
            let looks_like_registry_entry =
                block.contains("service_key") && block.contains("version");
            if !looks_like_registry_entry {
                continue;
            }
            assert!(
                block.contains(required),
                "RETURNING block missing health columns:\n{block}"
            );
        }
    }
    // The store must still exist so the include target compiles.
    let _ = RegistryStore::new;
}
