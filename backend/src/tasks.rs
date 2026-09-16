use chrono::{DateTime, Duration as ChronoDuration, Utc};
use chrono_tz::Tz;
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};

use crate::{
    environment::ENVIRONMENT,
    models::{CreatePassRequest, daily_pass_name, period_start_date},
    unleashed_api::{CreateOutcome, UNLEASHED_API},
};

/// Seconds from `now` until the next occurrence of `roll_hour` local time.
/// Always strictly positive, so a task that wakes exactly on the boundary
/// cannot spin.
pub fn seconds_until_next_roll(now: DateTime<Tz>, roll_hour: u32) -> i64 {
    let tz = now.timezone();
    let today = now.date_naive();

    let candidate = today
        .and_hms_opt(roll_hour, 0, 0)
        .expect("valid roll hour")
        .and_local_timezone(tz)
        .latest()
        .expect("roll time resolvable in timezone");

    let target = if candidate > now {
        candidate
    } else {
        (today + ChronoDuration::days(1))
            .and_hms_opt(roll_hour, 0, 0)
            .expect("valid roll hour")
            .and_local_timezone(tz)
            .latest()
            .expect("roll time resolvable in timezone")
    };

    (target - now).num_seconds().max(1)
}

/// Ensure a pass exists for the period currently in effect.
///
/// The controller rejects duplicate names, so this simply attempts the
/// create and treats `AlreadyExists` as success. There is no
/// read-before-write, which means concurrent replicas cannot both mint a
/// code for the same day.
async fn ensure_daily_pass() {
    let env = ENVIRONMENT.get().expect("Environment not set");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");

    let now = Utc::now().with_timezone(&env.timezone);
    let name = daily_pass_name(period_start_date(now, env.daily_roll_hour));

    let request = CreatePassRequest {
        name: name.clone(),
        duration_hours: env.daily_duration_hours,
        share_number: env.daily_share_number,
    };

    match client.create_pass(&request).await {
        Ok(CreateOutcome::Created(pass)) => {
            info!("Created daily guest pass {} (code {})", pass.name, pass.code)
        }
        Ok(CreateOutcome::AlreadyExists) => {
            info!("Daily guest pass {} already exists", name)
        }
        Err(e) => error!("Failed to create daily guest pass {}: {}", name, e),
    }
}

pub async fn run_daily_rotation() {
    let env = ENVIRONMENT.get().expect("Environment not set");

    // Mint immediately on startup so a fresh deploy is never without a code.
    ensure_daily_pass().await;

    loop {
        let now = Utc::now().with_timezone(&env.timezone);
        let secs = seconds_until_next_roll(now, env.daily_roll_hour);
        info!(
            "Next guest pass roll at {:02}:00 ({}), in {}h {}m",
            env.daily_roll_hour,
            env.timezone,
            secs / 3600,
            (secs % 3600) / 60
        );

        sleep(Duration::from_secs(secs as u64)).await;

        if UNLEASHED_API.get().is_none() {
            warn!("Unleashed client not ready, skipping this roll");
            continue;
        }
        ensure_daily_pass().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Tz;

    #[test]
    fn sleeps_until_todays_roll_when_it_is_still_ahead() {
        let now = Tz::UTC.with_ymd_and_hms(2026, 9, 16, 1, 0, 0).unwrap();
        assert_eq!(seconds_until_next_roll(now, 4), 3 * 3600);
    }

    #[test]
    fn sleeps_until_tomorrows_roll_once_today_has_passed() {
        let now = Tz::UTC.with_ymd_and_hms(2026, 9, 16, 5, 0, 0).unwrap();
        assert_eq!(seconds_until_next_roll(now, 4), 23 * 3600);
    }

    #[test]
    fn never_returns_a_non_positive_delay() {
        let now = Tz::UTC.with_ymd_and_hms(2026, 9, 16, 4, 0, 0).unwrap();
        assert!(seconds_until_next_roll(now, 4) > 0);
    }
}
