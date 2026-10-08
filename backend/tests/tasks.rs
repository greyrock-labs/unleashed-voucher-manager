use backend::tasks::next_midnight;
use chrono::{TimeZone, Utc};
use chrono_tz::{America, Tz};

fn local(tz: Tz, y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Tz> {
    tz.with_ymd_and_hms(y, mo, d, h, mi, 0).single().unwrap()
}

#[test]
fn an_ordinary_night() {
    let now = local(America::New_York, 2026, 10, 8, 13, 0);
    assert_eq!(
        next_midnight(now),
        local(America::New_York, 2026, 10, 9, 0, 0)
    );
}

#[test]
fn just_after_midnight_waits_for_the_next_one() {
    let now = local(America::New_York, 2026, 10, 9, 0, 1);
    assert_eq!(
        next_midnight(now),
        local(America::New_York, 2026, 10, 10, 0, 0)
    );
}

#[test]
fn a_skipped_midnight_runs_at_the_first_valid_time() {
    // Chile starts daylight saving at 00:00 on 2026-09-06; clocks jump to 01:00
    let now = local(America::Santiago, 2026, 9, 5, 12, 0);
    let next = next_midnight(now);
    assert_eq!(next, local(America::Santiago, 2026, 9, 6, 1, 0));
    assert!(next > now);
}

#[test]
fn a_repeated_midnight_runs_at_the_first_one() {
    // Cuba ends daylight saving at 01:00 on 2026-11-01, back to 00:00
    let now = local(America::Havana, 2026, 10, 31, 12, 0);
    let next = next_midnight(now);
    assert_eq!(
        next.with_timezone(&Utc),
        Utc.with_ymd_and_hms(2026, 11, 1, 4, 0, 0).unwrap()
    );
}
