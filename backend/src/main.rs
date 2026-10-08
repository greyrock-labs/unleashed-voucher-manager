use axum::{
    Router,
    http::{self, Method},
    routing::{delete, get, post},
};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, level_filters::LevelFilter, warn};
use tracing_subscriber::EnvFilter;

use backend::{
    environment::{ENVIRONMENT, Environment},
    handlers::*,
    tasks::run_daily_purge,
    unleashed::session::SessionError,
    unleashed_api::{ApiConfig, UNLEASHED_API, UnleashedAPI, startup_retry_delay},
};

#[tokio::main]
async fn main() {
    // =================================
    // Initialize tracing
    // =================================
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_env_var("BACKEND_LOG_LEVEL")
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    // =================================
    // Setup environment variables manager
    // =================================
    let env = match Environment::try_new() {
        Ok(env) => env,
        Err(e) => {
            error!("Failed to load environment variables: {e}");
            std::process::exit(1);
        }
    };
    ENVIRONMENT
        .set(env)
        .expect("Failed to set environment variables");
    let environment = ENVIRONMENT.get().expect("Environment not set");

    // =================================
    // Connect to the Unleashed controller
    // =================================
    let mut attempt = 0;
    loop {
        match UnleashedAPI::try_new(ApiConfig::from_environment(environment)).await {
            Ok(api) => {
                if UNLEASHED_API.set(api).is_err() {
                    panic!("UnleashedAPI already initialized");
                }
                info!("Connected to the Unleashed controller");
                break;
            }
            Err(e) => {
                match &e {
                    SessionError::Auth(_) => error!(
                        "{e}. Check UNLEASHED_USERNAME and UNLEASHED_PASSWORD, and that the \
                         user's role has read-write admin privilege."
                    ),
                    SessionError::Connect(_) => error!(
                        "{e}. Check UNLEASHED_URL, UNLEASHED_HAS_VALID_CERT and that the \
                         controller is reachable."
                    ),
                }
                let delay = startup_retry_delay(attempt);
                warn!("Retrying in {} seconds...", delay.as_secs());
                tokio::time::sleep(delay).await;
                attempt = attempt.saturating_add(1);
            }
        }
    }

    // =================================
    // Start scheduled tasks
    // =================================
    tokio::spawn(run_daily_purge(
        environment.timezone,
        environment.purge_all_expired_vouchers,
    ));

    // =================================
    // Setup Axum server
    // =================================
    let cors = CorsLayer::new()
        .allow_headers([http::header::CONTENT_TYPE])
        .allow_methods([Method::POST, Method::GET, Method::DELETE])
        .allow_origin(Any);

    let app = Router::new()
        .route("/api/health", get(health_check_handler))
        .route("/api/vouchers", get(get_all_vouchers_handler))
        .route("/api/vouchers", post(create_voucher_handler))
        .route("/api/vouchers/details", get(get_voucher_details_handler))
        .route("/api/vouchers/expired", delete(delete_expired_handler))
        .route("/api/vouchers/filtered", get(get_vouchers_filtered_handler))
        .route(
            "/api/vouchers/expired/rolling",
            delete(delete_expired_rolling_handler),
        )
        .route("/api/vouchers/newest", get(get_newest_voucher_handler))
        .route("/api/vouchers/rolling", get(get_rolling_voucher_handler))
        .route(
            "/api/vouchers/rolling",
            post(create_rolling_voucher_handler),
        )
        .route("/api/vouchers/selected", delete(delete_selected_handler))
        .layer(cors);

    let bind_address = format!(
        "{}:{}",
        environment.backend_bind_host, environment.backend_bind_port
    );

    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("Could not bind listener");

    info!("Server running on http://{}", bind_address);

    axum::serve(listener, app)
        .await
        .expect("Axum server should never error");
}
