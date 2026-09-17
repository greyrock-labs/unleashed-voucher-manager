use std::sync::{
    RwLock,
    atomic::{AtomicBool, Ordering},
};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use chrono::Utc;
use tracing::{debug, error, warn};

use crate::{
    environment::ENVIRONMENT,
    models::*,
    unleashed_api::{CreateOutcome, UNLEASHED_API, UnleashedApi},
};

pub const DAILY_NAME_PREFIX: &str = "daily-";

/// Response header on `/api/passes/daily` saying whether the pass served is
/// the one the current period expects. `false` means either that today's
/// roll has not landed yet, or that this came from the in-memory cache
/// because the controller is unreachable.
pub const DAILY_PASS_CURRENT_HEADER: &str = "x-daily-pass-current";

/// The last daily pass successfully resolved from the controller.
///
/// A controller blip must not blank the guest display: the code printed on
/// the wall stays valid for its full duration regardless of whether this
/// app can currently talk to the controller, so a failed refresh never
/// clears this.
static DAILY_SNAPSHOT: RwLock<Option<DailySnapshot>> = RwLock::new(None);

/// Whether the last controller round-trip succeeded. Read by `/api/health`
/// so the probe never waits on the controller itself -- see
/// `health_check_handler`.
static CONTROLLER_REACHABLE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone)]
pub struct DailySnapshot {
    pub pass: GuestPass,
    /// True when `pass` is the one the period in effect at fetch time
    /// expected, rather than a stale fallback.
    pub current: bool,
}

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

/// The controller client, or 503 if the background connect task has not
/// published it yet.
///
/// The listener comes up before the controller connection does (see
/// `main`), so every handler has to cope with the client being absent. The
/// old `UNLEASHED_API.get().expect(...)` would abort the whole process
/// under `panic = "abort"`, taking the frontend down with it.
fn client() -> Result<&'static UnleashedApi, StatusCode> {
    UNLEASHED_API.get().ok_or_else(|| {
        warn!("Controller client is not available yet, answering 503");
        StatusCode::SERVICE_UNAVAILABLE
    })
}

pub fn controller_reachable() -> bool {
    CONTROLLER_REACHABLE.load(Ordering::Relaxed)
}

fn snapshot() -> Option<DailySnapshot> {
    // A poisoned lock means some other task panicked while holding it; the
    // cached pass itself is still fine, and panicking again here would
    // abort the process.
    DAILY_SNAPSHOT
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Refresh the cached daily pass and the controller-reachability flag.
///
/// A failure marks the controller unreachable but deliberately leaves any
/// existing snapshot in place -- the whole point of the cache is to keep
/// serving the last known-good code through an outage.
pub async fn refresh_daily_cache() {
    let Some(client) = UNLEASHED_API.get() else {
        CONTROLLER_REACHABLE.store(false, Ordering::Relaxed);
        return;
    };

    match client.list_passes().await {
        Ok(passes) => {
            CONTROLLER_REACHABLE.store(true, Ordering::Relaxed);
            if let Some((pass, current)) = resolve_daily_pass(&passes, &expected_daily_name()) {
                *DAILY_SNAPSHOT.write().unwrap_or_else(|e| e.into_inner()) = Some(DailySnapshot {
                    pass: pass.clone(),
                    current,
                });
            }
        }
        Err(e) => {
            warn!("Could not refresh the daily guest pass from the controller: {e}");
            CONTROLLER_REACHABLE.store(false, Ordering::Relaxed);
        }
    }
}

pub async fn list_passes_handler() -> Result<Json<Vec<GuestPass>>, StatusCode> {
    debug!("Received request to list guest passes");
    let client = client()?;
    match client.list_passes().await {
        Ok(passes) => {
            CONTROLLER_REACHABLE.store(true, Ordering::Relaxed);
            Ok(Json(passes))
        }
        Err(e) => {
            error!("Failed to list guest passes: {}", e);
            CONTROLLER_REACHABLE.store(false, Ordering::Relaxed);
            Err(to_status(&e))
        }
    }
}

pub async fn create_pass_handler(
    Json(request): Json<CreatePassRequest>,
) -> Result<Json<GuestPass>, StatusCode> {
    debug!("Received request to create a guest pass");
    let client = client()?;
    match client.create_pass(&request).await {
        Ok(CreateOutcome::Created(pass)) => Ok(Json(*pass)),
        // The controller rejected the name as a duplicate. 409 rather than a
        // generic failure so the UI can say so specifically -- passes can
        // never be deleted, so this is a permanent condition for that name.
        Ok(CreateOutcome::AlreadyExists) => Err(StatusCode::CONFLICT),
        Err(e) => {
            error!("Failed to create guest pass: {}", e);
            Err(to_status(&e))
        }
    }
}

fn daily_response(pass: &GuestPass, current: bool) -> Response {
    (
        [(
            DAILY_PASS_CURRENT_HEADER,
            if current { "true" } else { "false" },
        )],
        Json(pass.clone()),
    )
        .into_response()
}

pub async fn daily_pass_handler() -> Result<Response, StatusCode> {
    debug!("Received request for today's guest pass");

    // Only go to the controller when it is believed to be up, or when the
    // cache has nothing to fall back on. During a known outage the call
    // would just block for the full 30s client timeout before failing, and
    // the cache already holds the code printed on the wall.
    if controller_reachable() || snapshot().is_none() {
        refresh_daily_cache().await;
    }

    let reachable = controller_reachable();
    match snapshot() {
        Some(snap) => {
            let current = snap.current && reachable;
            if !reachable {
                warn!(
                    "Serving the cached guest pass {} -- the controller is unreachable",
                    snap.pass.name
                );
            }
            Ok(daily_response(&snap.pass, current))
        }
        // Nothing cached and the controller answered: there genuinely is no
        // daily pass yet.
        None if reachable => Err(StatusCode::NOT_FOUND),
        // Nothing cached and no controller: we simply do not know yet.
        None => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// Build the health response from cached state alone.
///
/// Factored out of the handler so the reportable states can be unit tested
/// directly without standing up a mock controller.
fn health_from_cache(reachable: bool, snapshot: Option<&DailySnapshot>) -> HealthCheckResponse {
    HealthCheckResponse {
        status: if reachable { "ok" } else { "degraded" }.to_string(),
        daily_pass_current: reachable && snapshot.is_some_and(|s| s.current),
        controller_reachable: reachable,
    }
}

// This handler deliberately always returns HTTP 200, even when the
// Unleashed controller is completely unreachable. A restart cannot fix an
// unreachable upstream, so failing liveness on it would turn a controller
// outage into a crash-loop; failing readiness would pull this pod out of
// service and take the diagnostic page down with it too, making the outage
// total instead of degraded. So this endpoint keeps answering, and instead
// reports `status: "degraded"` / `controllerReachable: false` in the body so
// the truth is visible to anyone who reads the response rather than just the
// status code. Do not "fix" this by returning a non-2xx on failure.
//
// It also answers purely from cached state and never awaits the controller.
// The reqwest client's timeout is 30s and a call may include a re-login, far
// longer than any sane probe `timeoutSeconds` -- so hitting the controller
// here would fail the probe on timeout during an outage and produce exactly
// the crash-loop the paragraph above exists to prevent. The background
// refresh task keeps the cache current.
pub async fn health_check_handler() -> Json<HealthCheckResponse> {
    debug!("Received health check request");
    Json(health_from_cache(
        controller_reachable(),
        snapshot().as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use self::backend_test_support::{pass, snapshot_of};
    use super::*;

    mod backend_test_support {
        use super::DailySnapshot;
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

        pub fn snapshot_of(name: &str, current: bool) -> DailySnapshot {
            DailySnapshot {
                pass: pass(name, 100),
                current,
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

    #[test]
    fn health_reports_ok_when_todays_pass_is_current() {
        let health = health_from_cache(true, Some(&snapshot_of("daily-2026-09-16", true)));
        assert_eq!(health.status, "ok");
        assert!(health.daily_pass_current);
        assert!(health.controller_reachable);
    }

    #[test]
    fn health_reports_ok_but_stale_when_todays_pass_is_missing() {
        let health = health_from_cache(true, Some(&snapshot_of("daily-2026-09-15", false)));
        assert_eq!(health.status, "ok");
        assert!(!health.daily_pass_current);
        assert!(health.controller_reachable);
    }

    /// The controller outage case: still 200'd by the handler, but the body
    /// must say so -- this is the whole point of the fix.
    #[test]
    fn health_reports_degraded_when_controller_is_unreachable() {
        let health = health_from_cache(false, None);
        assert_eq!(health.status, "degraded");
        assert!(!health.daily_pass_current);
        assert!(!health.controller_reachable);
    }

    /// A cached pass keeps the display alive through an outage, but it must
    /// not be reported as current -- nothing has confirmed it since the
    /// controller went away.
    #[test]
    fn health_does_not_call_a_cached_pass_current_while_unreachable() {
        let health = health_from_cache(false, Some(&snapshot_of("daily-2026-09-16", true)));
        assert_eq!(health.status, "degraded");
        assert!(!health.daily_pass_current);
        assert!(!health.controller_reachable);
    }

    /// Startup, before the background connect task has published a client:
    /// 200 with `controllerReachable: false`, never a 5xx that would fail a
    /// probe and crash-loop the pod.
    #[test]
    fn health_is_degraded_but_ok_before_the_client_is_ready() {
        let health = health_from_cache(false, None);
        assert_eq!(health.status, "degraded");
        assert!(!health.controller_reachable);
    }
}
