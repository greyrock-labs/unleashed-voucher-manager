use std::sync::atomic::{AtomicUsize, Ordering};

use backend::environment::Environment;
use backend::models::UnleashedError;
use backend::unleashed_api::{extract_csrf_token, UnleashedApi};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

#[test]
fn extracts_the_csrf_token_from_inline_script() {
    let html = r#"
        <script>
          var company = 'Ruckus Wireless';
        </script>
        <script>
var csfrToken = 'sBMp7znAEq';
</script>"#;
    assert_eq!(extract_csrf_token(html).as_deref(), Some("sBMp7znAEq"));
}

#[test]
fn returns_none_when_the_token_is_absent() {
    assert_eq!(extract_csrf_token("<html><body>nope</body></html>"), None);
}

// -- Mock-HTTP session tests -------------------------------------------------
//
// `ENVIRONMENT` is a process-global `OnceLock` and can only be set once, so
// it cannot give each test its own mock server. Instead these tests build an
// `Environment` directly (all its fields are `pub`) and leak it to get the
// `'static` reference `UnleashedApi` requires, then construct the API via
// `UnleashedApi::from_environment` rather than `try_new`/the global
// `ENVIRONMENT`/`UNLEASHED_API`. That keeps every test's mock server fully
// isolated from the others.

const LOGIN_PATH: &str = "/user/user_login_guestpass.jsp";
const CMD_PATH: &str = "/user/_cmdstat.jsp";
const CSRF_TOKEN: &str = "TESTTOKEN123";
const VALID_XML: &str = r#"<ajax-response><xmsg type="0" msg=""/></ajax-response>"#;
const LOGIN_PAGE: &str = "<!DOCTYPE html><html><body>session expired, please log in</body></html>";

fn login_html() -> String {
    format!(
        r#"<html><script>var csfrToken = '{CSRF_TOKEN}';</script></html>"#
    )
}

fn test_environment(base_url: String) -> &'static Environment {
    Box::leak(Box::new(Environment {
        unleashed_url: base_url,
        unleashed_username: "guest-admin".to_string(),
        unleashed_password: "hunter2".to_string(),
        unleashed_ssid: "Grey Rock Guest".to_string(),
        unleashed_has_valid_cert: true,
        backend_bind_host: "127.0.0.1".to_string(),
        backend_bind_port: 8080,
        timezone: chrono_tz::Tz::UTC,
        daily_roll_hour: 4,
        daily_duration_hours: 24,
        daily_share_number: 0,
    }))
}

#[tokio::test]
async fn logs_in_and_calls_with_the_scraped_csrf_token() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string(login_html()))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(CMD_PATH))
        .and(header("X-CSRF-Token", CSRF_TOKEN))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_XML))
        .expect(1)
        .mount(&server)
        .await;

    let environment = test_environment(server.uri());
    let api = UnleashedApi::from_environment(environment)
        .await
        .expect("login against the mock server should succeed");

    let body = api
        .call("getstat", "system", "<guest-list/>")
        .await
        .expect("call should succeed on the first try");
    assert_eq!(body, VALID_XML);

    // Confirms both the request counts above AND that the CSRF header
    // carried the token scraped from the login page.
    server.verify().await;
}

#[tokio::test]
async fn re_authenticates_once_when_the_session_has_lapsed() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .expect(2) // initial login during construction + one re-login
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string(login_html()))
        .expect(2)
        .mount(&server)
        .await;

    // First _cmdstat.jsp call reports a lapsed session; the second (after
    // re-login) succeeds.
    let cmd_calls = AtomicUsize::new(0);
    Mock::given(method("POST"))
        .and(path(CMD_PATH))
        .respond_with(move |_req: &Request| {
            if cmd_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(200).set_body_string(LOGIN_PAGE)
            } else {
                ResponseTemplate::new(200).set_body_string(VALID_XML)
            }
        })
        .expect(2) // exactly one retry, not a loop
        .mount(&server)
        .await;

    let environment = test_environment(server.uri());
    let api = UnleashedApi::from_environment(environment)
        .await
        .expect("initial login against the mock server should succeed");

    let body = api
        .call("getstat", "system", "<guest-list/>")
        .await
        .expect("call should succeed after re-authenticating once");
    assert_eq!(body, VALID_XML);

    server.verify().await;
}

#[tokio::test]
async fn gives_up_after_a_single_failed_retry() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .expect(2) // initial login + the one re-login attempt
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string(login_html()))
        .expect(2)
        .mount(&server)
        .await;

    // Every _cmdstat.jsp call reports a lapsed session -- login never
    // actually recovers the session.
    Mock::given(method("POST"))
        .and(path(CMD_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string(LOGIN_PAGE))
        .expect(2) // the initial attempt plus exactly one retry -- no loop
        .mount(&server)
        .await;

    let environment = test_environment(server.uri());
    let api = UnleashedApi::from_environment(environment)
        .await
        .expect("initial login against the mock server should succeed");

    let result = api.call("getstat", "system", "<guest-list/>").await;
    assert!(
        matches!(result, Err(UnleashedError::SessionLapsed)),
        "expected SessionLapsed, got {result:?}"
    );

    // If `call` had retried more than once, this mock's `expect(2)` would
    // fail here.
    server.verify().await;
}
