use axum::{
    Router,
    http::{self, Method},
    routing::{get, post},
};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, level_filters::LevelFilter, warn};
use tracing_subscriber::EnvFilter;

use backend::{
    environment::{ENVIRONMENT, Environment},
    handlers::*,
    unleashed_api::{UNLEASHED_API, UnleashedApi},
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

    loop {
        match UnleashedApi::try_new().await {
            Ok(api) => {
                UNLEASHED_API.set(api).expect("Failed to set UnleashedApi");
                info!("Successfully connected to Unleashed controller");
                break;
            }
            Err(e) => {
                error!("Failed to initialize UnleashedApi: {}", e);
                warn!("Retrying connection in 5 seconds...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    // The daily rotation task is spawned in Task 6, which supplies it.
    // Wiring the spawn here would leave the crate uncompilable until then.

    let cors = CorsLayer::new()
        .allow_headers([http::header::CONTENT_TYPE])
        .allow_methods([Method::POST, Method::GET])
        .allow_origin(Any);

    let app = Router::new()
        .route("/api/health", get(health_check_handler))
        .route("/api/passes", get(list_passes_handler))
        .route("/api/passes", post(create_pass_handler))
        .route("/api/passes/daily", get(daily_pass_handler))
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
