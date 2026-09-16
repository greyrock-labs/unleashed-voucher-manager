use axum::response::Json;
use tracing::debug;

pub async fn health_check_handler() -> Json<serde_json::Value> {
    debug!("Received health check request");
    Json(serde_json::json!({ "status": "ok" }))
}
