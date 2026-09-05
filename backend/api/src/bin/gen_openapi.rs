//! Emit the OpenAPI contract as JSON (openapi/openapi.json).
use utoipa::OpenApi;

fn main() {
    let openapi = admin_panel_api::ApiDoc::openapi();
    let json = serde_json::to_string_pretty(&openapi).expect("serialize openapi");
    print!("{json}");
}
