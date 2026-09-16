// Regression: the health worker must derive the probe path from the
// capability catalog (docs/API.md §6: health.read → GET /health), not hardcode
// `/health/live` — services without that route were marked unreachable.
#[test]
fn health_worker_probe_path_is_catalog_driven() {
    let src = include_str!("../src/health_worker.rs");
    assert!(
        !src.contains("/health/live"),
        "health_worker must not hardcode /health/live; use the catalog fixed_path"
    );
    assert!(
        src.contains("fixed_path"),
        "worker should use catalog fixed_path"
    );
}
