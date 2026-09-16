use chrono::{DateTime, Duration as ChronoDuration, LocalResult, NaiveDate, Utc};
use chrono_tz::Tz;
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};

use crate::{
    environment::ENVIRONMENT,
    models::{CreatePassRequest, UnleashedError, daily_pass_name, period_start_date},
    unleashed_api::{CreateOutcome, UNLEASHED_API},
};

/// Number of times the startup mint retries before deferring to the normal
/// schedule.
const STARTUP_RETRY_ATTEMPTS: u32 = 5;
/// Delay between startup mint retries.
const STARTUP_RETRY_DELAY: Duration = Duration::from_secs(60);

/// Resolve `date` at `hour:00:00` local time in `tz` to a concrete instant.
///
/// A repeated local time (the autumn "fall back" hour) uses the later of
/// the two offsets, matching `.latest()`. A local time that does not exist
/// (a spring-forward gap, e.g. `America/New_York` skipping 02:00-03:00) has
/// no well-defined instant, so this advances hour by hour -- bounded to a
/// few attempts -- until it lands past the gap, the same way a clock that
/// springs forward simply skips the missing hour.
fn resolve_local_hour(date: NaiveDate, hour: u32, tz: Tz) -> DateTime<Tz> {
    let base = date.and_hms_opt(hour, 0, 0).expect("valid roll hour");

    for step in 0..3 {
        let naive = base + ChronoDuration::hours(step);
        match naive.and_local_timezone(tz) {
            LocalResult::Single(dt) => return dt,
            LocalResult::Ambiguous(_earlier, later) => return later,
            LocalResult::None => continue,
        }
    }

    panic!("local time near {base} does not resolve in {tz} even after advancing past a DST gap");
}

/// Seconds from `now` until the next occurrence of `roll_hour` local time.
/// Always strictly positive, so a task that wakes exactly on the boundary
/// cannot spin.
pub fn seconds_until_next_roll(now: DateTime<Tz>, roll_hour: u32) -> i64 {
    let tz = now.timezone();
    let today = now.date_naive();

    let candidate = resolve_local_hour(today, roll_hour, tz);

    let target = if candidate > now {
        candidate
    } else {
        resolve_local_hour(today + ChronoDuration::days(1), roll_hour, tz)
    };

    (target - now).num_seconds().max(1)
}

/// Ensure a pass exists for the period currently in effect.
///
/// The controller rejects duplicate names, so this simply attempts the
/// create and treats `AlreadyExists` as success. There is no
/// read-before-write, which means concurrent replicas cannot both mint a
/// code for the same day.
async fn ensure_daily_pass() -> Result<(), UnleashedError> {
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
            info!("Created daily guest pass {} (code {})", pass.name, pass.code);
            Ok(())
        }
        Ok(CreateOutcome::AlreadyExists) => {
            info!("Daily guest pass {} already exists", name);
            Ok(())
        }
        Err(e) => {
            error!("Failed to create daily guest pass {}: {}", name, e);
            Err(e)
        }
    }
}

pub async fn run_daily_rotation() {
    let env = ENVIRONMENT.get().expect("Environment not set");

    // Mint immediately on startup so a fresh deploy is never without a
    // code. Retry a few times with a short backoff in case the controller
    // is mid-outage right at startup -- without this, a failed mint here
    // would silently wait for the next scheduled roll, up to 24h away.
    for attempt in 1..=STARTUP_RETRY_ATTEMPTS {
        if ensure_daily_pass().await.is_ok() {
            break;
        }
        if attempt < STARTUP_RETRY_ATTEMPTS {
            warn!(
                "Startup guest pass mint failed (attempt {}/{}), retrying in {}s",
                attempt,
                STARTUP_RETRY_ATTEMPTS,
                STARTUP_RETRY_DELAY.as_secs()
            );
            sleep(STARTUP_RETRY_DELAY).await;
        } else {
            warn!(
                "Startup guest pass mint failed after {} attempts, deferring to the normal schedule",
                STARTUP_RETRY_ATTEMPTS
            );
        }
    }

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

        // Scheduled rolls just log and move on; a failure here is picked
        // up again at the next roll, ~24h later.
        let _ = ensure_daily_pass().await;
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

        // A moment a few hundred milliseconds before the boundary
        // truncates to 0 whole seconds via `num_seconds()` -- this
        // actually exercises the `.max(1)` clamp, unlike landing exactly
        // on the boundary above.
        let almost_there = Tz::UTC.with_ymd_and_hms(2026, 9, 16, 3, 59, 59).unwrap()
            + ChronoDuration::milliseconds(800);
        assert!(seconds_until_next_roll(almost_there, 4) > 0);
    }

    #[test]
    fn does_not_panic_in_a_spring_forward_gap() {
        // 2027-03-14 is the second Sunday of March, the day
        // America/New_York's clocks jump from 02:00 EST straight to
        // 03:00 EDT. Verified directly against chrono-tz:
        // `America::New_York.from_local_datetime(2027-03-14 02:00:00)`
        // returns `LocalResult::None`, and 03:00:00 returns
        // `Single(2027-03-14T03:00:00 EDT)` -- confirming 2027-03-14 is a
        // real spring-forward transition day and 02:00 sits in the gap.
        let now = chrono_tz::America::New_York
            .with_ymd_and_hms(2027, 3, 14, 1, 0, 0)
            .unwrap();
        assert!(seconds_until_next_roll(now, 2) > 0);
    }
}
