use axum::{http::StatusCode, response::Json};
use chrono::Utc;
use tracing::{debug, error};

use crate::{
    environment::ENVIRONMENT,
    models::*,
    unleashed_api::{CreateOutcome, UNLEASHED_API},
};

pub const DAILY_NAME_PREFIX: &str = "daily-";

/// Find the pass to display. Returns the pass and whether it is the one the
/// current period expects; a stale fallback keeps the display populated when
/// a roll was missed.
pub fn resolve_daily_pass<'a>(
    passes: &'a [GuestPass],
    expected_name: &str,
) -> Option<(&'a GuestPass, bool)> {
    if let Some(p) = passes.iter().find(|p| p.name == expected_name) {
        return Some((p, true));
    }
    passes
        .iter()
        .filter(|p| p.name.starts_with(DAILY_NAME_PREFIX))
        .max_by_key(|p| p.created_at)
        .map(|p| (p, false))
}

fn expected_daily_name() -> String {
    let env = ENVIRONMENT.get().expect("Environment not set");
    let now = Utc::now().with_timezone(&env.timezone);
    daily_pass_name(period_start_date(now, env.daily_roll_hour))
}

fn to_status(e: &UnleashedError) -> StatusCode {
    match e {
        UnleashedError::Auth | UnleashedError::SessionLapsed => StatusCode::BAD_GATEWAY,
        UnleashedError::Controller { .. } => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn list_passes_handler() -> Result<Json<Vec<GuestPass>>, StatusCode> {
    debug!("Received request to list guest passes");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");
    match client.list_passes().await {
        Ok(passes) => Ok(Json(passes)),
        Err(e) => {
            error!("Failed to list guest passes: {}", e);
            Err(to_status(&e))
        }
    }
}

pub async fn create_pass_handler(
    Json(request): Json<CreatePassRequest>,
) -> Result<Json<GuestPass>, StatusCode> {
    debug!("Received request to create a guest pass");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");
    match client.create_pass(&request).await {
        Ok(CreateOutcome::Created(pass)) => Ok(Json(*pass)),
        Ok(CreateOutcome::AlreadyExists) => Err(StatusCode::CONFLICT),
        Err(e) => {
            error!("Failed to create guest pass: {}", e);
            Err(to_status(&e))
        }
    }
}

pub async fn daily_pass_handler() -> Result<Json<GuestPass>, StatusCode> {
    debug!("Received request for today's guest pass");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");
    let passes = client.list_passes().await.map_err(|e| {
        error!("Failed to list guest passes: {}", e);
        to_status(&e)
    })?;

    match resolve_daily_pass(&passes, &expected_daily_name()) {
        Some((pass, _current)) => Ok(Json(pass.clone())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn health_check_handler() -> Result<Json<HealthCheckResponse>, StatusCode> {
    debug!("Received health check request");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");
    let daily_pass_current = match client.list_passes().await {
        Ok(passes) => resolve_daily_pass(&passes, &expected_daily_name())
            .map(|(_, current)| current)
            .unwrap_or(false),
        Err(_) => false,
    };

    Ok(Json(HealthCheckResponse {
        status: "ok".to_string(),
        daily_pass_current,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use self::backend_test_support::pass;

    mod backend_test_support {
        use crate::models::GuestPass;
        pub fn pass(name: &str, created_at: i64) -> GuestPass {
            GuestPass {
                id: name.to_string(),
                name: name.to_string(),
                code: "000000".to_string(),
                ssid: "Test".to_string(),
                created_at,
                activated_at: None,
                expires_at: created_at + 604800,
                valid_time_secs: 86400,
                used: false,
                share_number: 0,
                client_macs: vec![],
                remarks: String::new(),
            }
        }
    }

    #[test]
    fn prefers_the_pass_for_the_current_period() {
        let passes = vec![pass("daily-2026-09-15", 100), pass("daily-2026-09-16", 200)];
        let (found, current) = resolve_daily_pass(&passes, "daily-2026-09-16").unwrap();
        assert_eq!(found.name, "daily-2026-09-16");
        assert!(current);
    }

    /// If the roll never happened, show the newest daily pass rather than
    /// nothing, and report it as not current.
    #[test]
    fn falls_back_to_the_newest_daily_pass_when_today_is_missing() {
        let passes = vec![pass("daily-2026-09-14", 100), pass("daily-2026-09-15", 200)];
        let (found, current) = resolve_daily_pass(&passes, "daily-2026-09-16").unwrap();
        assert_eq!(found.name, "daily-2026-09-15");
        assert!(!current);
    }

    #[test]
    fn ignores_passes_that_are_not_daily() {
        let passes = vec![pass("work-laptop", 999)];
        assert!(resolve_daily_pass(&passes, "daily-2026-09-16").is_none());
    }
}
