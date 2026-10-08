use backend::unleashed::{
    guest::GuestPass,
    mapping::{current_rolling, has_live_rolling_for_ip, rolling_name, to_voucher},
};
use chrono::TimeZone;
use chrono_tz::Tz;

const NOW: i64 = 1_791_478_540;

fn pass(id: &str, name: &str) -> GuestPass {
    GuestPass {
        id: id.into(),
        name: name.into(),
        key: "123456".into(),
        remarks: "".into(),
        share_number: 0,
        valid_time: 3600,
        create_time: NOW,
        start_time: Some(NOW),
        expire_time: NOW + 3600,
        used: false,
        client_macs: vec![],
    }
}

#[test]
fn maps_an_unused_pass() {
    let v = to_voucher(&pass("4", "Guest-4"), Tz::UTC, NOW);
    assert_eq!(v.id, "4");
    assert_eq!(v.code, "123456");
    assert_eq!(v.created_at, "2026-10-08 16:55:40");
    assert_eq!(v.expires_at.as_deref(), Some("2026-10-08 17:55:40"));
    assert_eq!(v.activated_at, None, "start-time alone does not mean used");
    assert_eq!(v.authorized_guest_limit, None, "0 is unlimited");
    assert_eq!(v.authorized_guest_count, 0);
    assert_eq!(v.time_limit_minutes, 60);
    assert!(!v.expired);
}

#[test]
fn maps_a_used_pass() {
    let p = GuestPass {
        share_number: 3,
        client_macs: vec!["00:00:00:00:00:01".into(), "00:00:00:00:00:02".into()],
        ..pass("1", "guest-1")
    };
    let v = to_voucher(&p, Tz::UTC, NOW);
    assert_eq!(v.activated_at.as_deref(), Some("2026-10-08 16:55:40"));
    assert_eq!(v.authorized_guest_limit, Some(3));
    assert_eq!(v.authorized_guest_count, 2);
}

#[test]
fn used_flag_without_clients_counts_as_used() {
    let p = GuestPass {
        used: true,
        ..pass("1", "a")
    };
    assert!(to_voucher(&p, Tz::UTC, NOW).activated_at.is_some());
}

#[test]
fn formats_in_the_configured_timezone() {
    let v = to_voucher(&pass("1", "a"), chrono_tz::America::New_York, NOW);
    assert_eq!(v.created_at, "2026-10-08 12:55:40");
}

#[test]
fn expiry_is_inclusive() {
    let p = pass("1", "a");
    assert!(!to_voucher(&p, Tz::UTC, p.expire_time - 1).expired);
    assert!(to_voucher(&p, Tz::UTC, p.expire_time).expired);
}

#[test]
fn names_rolling_passes_without_whitespace() {
    let created = Tz::UTC.timestamp_opt(NOW, 0).unwrap();
    let name = rolling_name(created, "192.0.2.7");
    assert_eq!(name, "[ROLLING]-20261008165540-192.0.2.7");
    assert!(!name.contains(char::is_whitespace));
}

#[test]
fn picks_the_newest_unused_unexpired_rolling_pass() {
    let passes = vec![
        GuestPass {
            create_time: NOW - 10,
            ..pass("1", "[ROLLING]-a-192.0.2.1")
        },
        GuestPass {
            create_time: NOW - 5,
            ..pass("2", "[ROLLING]-b-192.0.2.2")
        },
        GuestPass {
            used: true,
            ..pass("3", "[ROLLING]-c-192.0.2.3")
        },
        GuestPass {
            expire_time: NOW,
            ..pass("4", "[ROLLING]-d-192.0.2.4")
        },
        pass("5", "not rolling"),
    ];
    assert_eq!(current_rolling(&passes, NOW).unwrap().id, "2");
}

#[test]
fn no_rolling_pass_when_all_used_or_expired() {
    let passes = vec![
        GuestPass {
            used: true,
            ..pass("1", "[ROLLING]-a-192.0.2.1")
        },
        GuestPass {
            expire_time: NOW - 1,
            ..pass("2", "[ROLLING]-b-192.0.2.2")
        },
    ];
    assert!(current_rolling(&passes, NOW).is_none());
}

#[test]
fn matches_rolling_passes_by_whole_ip() {
    let passes = vec![pass("1", "[ROLLING]-20261008133540-11.2.3.4")];
    assert!(has_live_rolling_for_ip(&passes, "11.2.3.4", NOW));
    assert!(!has_live_rolling_for_ip(&passes, "1.2.3.4", NOW));
}

#[test]
fn expired_rolling_passes_do_not_block_an_ip() {
    let passes = vec![GuestPass {
        expire_time: NOW - 1,
        ..pass("1", "[ROLLING]-20261008133540-192.0.2.1")
    }];
    assert!(!has_live_rolling_for_ip(&passes, "192.0.2.1", NOW));
}
