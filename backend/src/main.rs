use axum::{Router, http::{self, Method}, routing::get};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

use backend::{
    environment::{ENVIRONMENT, Environment},
    handlers::*,
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_env_var("BACKEND_LOG_LEVEL")
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let env = match Environment::try_new() {
        Ok(env) => env,
        Err(e) => {
            error!("Failed to load environment variables: {e}");
            std::process::exit(1);
        }
    };
    ENVIRONMENT.set(env).expect("Failed to set environment variables");
    let environment = ENVIRONMENT.get().expect("Environment not set");

    let cors = CorsLayer::new()
        .allow_headers([http::header::CONTENT_TYPE])
        .allow_methods([Method::POST, Method::GET])
        .allow_origin(Any);

    let app = Router::new()
        .route("/api/health", get(health_check_handler))
        .layer(cors);

    let bind_address = format!(
        "{}:{}",
        environment.backend_bind_host, environment.backend_bind_port
    );
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("Could not bind listener");

    info!("Server running on http://{}", bind_address);
    axum::serve(listener, app).await.expect("Axum server should never error");
}
