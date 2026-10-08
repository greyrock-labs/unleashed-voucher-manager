use backend::{environment::Environment, unleashed_api::ApiConfig};
use chrono_tz::Tz;

const SECRET: &str = "hunter2-do-not-print";

#[test]
fn api_config_debug_hides_the_password() {
    let config = ApiConfig {
        url: "https://unleashed.example.com".into(),
        username: "admin".into(),
        password: SECRET.into(),
        ssid: "Guest".into(),
        verify_tls: true,
        timezone: Tz::UTC,
        rolling_voucher_duration_minutes: 480,
    };
    let printed = format!("{config:?}");
    assert!(!printed.contains(SECRET), "{printed}");
    assert!(printed.contains("<redacted>"), "{printed}");
    assert!(
        printed.contains("admin"),
        "other fields still print: {printed}"
    );
}

#[test]
fn environment_debug_hides_the_password() {
    let env = Environment {
        unleashed_url: "https://unleashed.example.com".into(),
        unleashed_username: "admin".into(),
        unleashed_password: SECRET.into(),
        unleashed_ssid: "Guest".into(),
        unleashed_has_valid_cert: true,
        backend_bind_host: "127.0.0.1".into(),
        backend_bind_port: 8080,
        purge_all_expired_vouchers: false,
        rolling_voucher_duration_minutes: 480,
        timezone: Tz::UTC,
    };
    let printed = format!("{env:?}");
    assert!(!printed.contains(SECRET), "{printed}");
    assert!(printed.contains("<redacted>"), "{printed}");
}
