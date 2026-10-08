//! Emit the OpenAPI contract as JSON (openapi/openapi.json).
use utoipa::OpenApi;

fn main() {
    let mut openapi = admin_panel_api::ApiDoc::openapi();
    // Keep the established published contract free of empty Cargo license metadata.
    if openapi
        .info
        .license
        .as_ref()
        .is_some_and(|license| license.name.is_empty())
    {
        openapi.info.license = None;
    }
    let json = serde_json::to_string_pretty(&openapi).expect("serialize openapi");
    print!("{json}");
}
