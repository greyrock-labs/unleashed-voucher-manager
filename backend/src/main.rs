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
    tasks::{run_daily_rotation, run_pass_cache_refresh},
    unleashed_api::{UNLEASHED_API, UnleashedApi},
};

/// Delay between controller connection attempts at startup.
const CONNECT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Connect to the Unleashed controller, retrying until it answers.
///
/// Deliberately run as a background task rather than before `axum::serve`:
/// the listener has to come up even during a controller outage. Blocking
/// startup on the controller means any restart mid-outage -- a node drain,
/// an image pull, a cluster upgrade -- never serves anything and turns into
/// CrashLoopBackOff, which is precisely what `/api/health` answering
/// 200-with-`degraded` exists to avoid.
async fn connect_to_controller() {
    loop {
        match UnleashedApi::try_new().await {
            Ok(api) => {
                if UNLEASHED_API.set(api).is_err() {
                    warn!("UnleashedApi was already initialised, keeping the existing client");
                }
                info!("Successfully connected to Unleashed controller");
                return;
            }
            Err(e) => {
                error!("Failed to initialize UnleashedApi: {}", e);
                warn!(
                    "Retrying connection in {} seconds...",
                    CONNECT_RETRY_DELAY.as_secs()
                );
                tokio::time::sleep(CONNECT_RETRY_DELAY).await;
            }
        }
    }
}

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
        .route("/api/passes", get(list_passes_handler))
        .route("/api/passes", post(create_pass_handler))
        .route("/api/passes/daily", get(daily_pass_handler))
        .layer(cors);

    let bind_address = format!(
        "{}:{}",
        environment.backend_bind_host, environment.backend_bind_port
    );
    let listener = match tokio::net::TcpListener::bind(&bind_address).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Could not bind listener on {bind_address}: {e}");
            std::process::exit(1);
        }
    };

    // Everything that touches the controller runs behind the listener, so a
    // controller outage degrades this app instead of preventing it from
    // starting. Both tasks cope with `UNLEASHED_API` not being set yet.
    tokio::spawn(connect_to_controller());
    tokio::spawn(run_pass_cache_refresh());
    tokio::spawn(run_daily_rotation());

    info!("Server running on http://{}", bind_address);
    if let Err(e) = axum::serve(listener, app).await {
        error!("Axum server stopped: {e}");
        std::process::exit(1);
    }
}
