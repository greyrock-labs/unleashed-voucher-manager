use backend::environment::Environment;
use backend::models::{controller_error, parse_guest_list, CreatePassRequest, DUPLICATE_NAME_MSG};
use backend::unleashed_api::{CreateOutcome, UnleashedApi};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const GUEST_LIST: &str = include_str!("fixtures/guest_list.xml");
const ERR_DUPLICATE: &str = include_str!("fixtures/error_duplicate.xml");
const ERR_PRIVILEGE: &str = include_str!("fixtures/error_privilege.xml");
const GENERATE_KEY: &str = include_str!("fixtures/generate_key.xml");

/// A duplicate name means the pass already exists and nothing was created.
/// The daily task depends on telling this apart from a real failure.
#[test]
fn duplicate_is_distinguishable_from_other_controller_errors() {
    assert_eq!(controller_error(ERR_DUPLICATE).as_deref(), Some(DUPLICATE_NAME_MSG));
    assert_ne!(controller_error(ERR_PRIVILEGE).as_deref(), Some(DUPLICATE_NAME_MSG));
}

#[test]
fn a_named_pass_can_be_found_in_a_parsed_list() {
    let passes = parse_guest_list(GUEST_LIST).unwrap();
    let found = passes.iter().find(|p| p.name == "daily-2026-09-16");
    assert!(found.is_some());
    assert_eq!(found.unwrap().code, "399711");
}

// -- Mock-HTTP create-guest tests --------------------------------------------
//
// Same isolation trick as `tests/session.rs`: build an `Environment`
// directly and leak it for the `'static` reference `UnleashedApi` needs,
// rather than going through the process-global `ENVIRONMENT` OnceLock.

const LOGIN_PATH: &str = "/user/user_login_guestpass.jsp";
const CMD_PATH: &str = "/user/_cmdstat.jsp";
const CSRF_TOKEN: &str = "TESTTOKEN123";
const SSID: &str = "Grey Rock Guest";

fn login_html() -> String {
    format!(r#"<html><script>var csfrToken = '{CSRF_TOKEN}';</script></html>"#)
}

fn test_environment(base_url: String) -> &'static Environment {
    Box::leak(Box::new(Environment {
        unleashed_url: base_url,
        unleashed_username: "guest-admin".to_string(),
        unleashed_password: "hunter2".to_string(),
        unleashed_ssid: SSID.to_string(),
        unleashed_has_valid_cert: true,
        backend_bind_host: "127.0.0.1".to_string(),
        backend_bind_port: 8080,
        timezone: chrono_tz::Tz::UTC,
        daily_roll_hour: 4,
        daily_duration_hours: 24,
        daily_share_number: 0,
    }))
}

async fn mount_login(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string(login_html()))
        .mount(server)
        .await;
}

/// Every create is preceded by a `generate-guest-key` docmd.
async fn mount_generate_key(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path(CMD_PATH))
        .and(body_string_contains("generate-guest-key"))
        .respond_with(ResponseTemplate::new(200).set_body_string(GENERATE_KEY))
        .expect(1)
        .mount(server)
        .await;
}

/// The controller constraints that the whole product depends on are enforced
/// only by string interpolation in `create_pass`, so assert the bytes that
/// actually go on the wire:
///
/// - `duration-unit='hour'`: the controller silently treats 'day' as hours,
///   so a `duration='1'` with unit 'day' would mint a ONE HOUR pass.
/// - `share-number='0'`: 0 means UNLIMITED devices and is the daily default;
///   1 would admit a single device and lock out every other guest.
/// - the name is sanitised to the controller's charset before being sent.
#[tokio::test]
async fn create_guest_sends_whole_hours_unlimited_sharing_and_a_sanitised_name() {
    let server = MockServer::start().await;
    mount_login(&server).await;
    mount_generate_key(&server).await;

    // Each `.and(...)` is part of the match, so `expect(1)` + `verify()`
    // fails the test if ANY of these attributes is wrong or missing.
    Mock::given(method("POST"))
        .and(path(CMD_PATH))
        .and(body_string_contains("cmd='create-guest'"))
        .and(body_string_contains("duration='4'"))
        .and(body_string_contains("duration-unit='hour'"))
        .and(body_string_contains("share-number='0'"))
        // "Quick Pass (4 Hours)" -- spaces become '-' and the parentheses,
        // which the controller rejects, are dropped.
        .and(body_string_contains("name='Quick-Pass-4-Hours'"))
        .and(body_string_contains("x-key='399711'"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"<ajax-response><xmsg type="0"/></ajax-response>"#),
        )
        .expect(1)
        .mount(&server)
        .await;

    // create_pass re-lists afterwards to pick up the controller-assigned id.
    Mock::given(method("POST"))
        .and(path(CMD_PATH))
        .and(body_string_contains("guest-list"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<ajax-response><response><guest-list><guest id="7" name="Quick-Pass-4-Hours" x-key="123456" ssid="{SSID}" share-number="0" create-time="1789589893" valid-time="14400" start-time="" expire-time="1790194693" remarks=""/></guest-list></response></ajax-response>"#
        )))
        .expect(1)
        .mount(&server)
        .await;

    let api = UnleashedApi::from_environment(test_environment(server.uri()))
        .await
        .expect("login against the mock server should succeed");

    let outcome = api
        .create_pass(&CreatePassRequest {
            name: "Quick Pass (4 Hours)".to_string(),
            duration_hours: 4,
            share_number: 0,
        })
        .await
        .expect("create should succeed");

    match outcome {
        CreateOutcome::Created(pass) => {
            assert_eq!(pass.name, "Quick-Pass-4-Hours");
            assert_eq!(pass.id, "7");
            assert_eq!(pass.share_number, 0);
        }
        other => panic!("expected Created, got {other:?}"),
    }

    // Asserts the request counts above AND, through the matchers, the exact
    // wire format of the create-guest command.
    server.verify().await;
}

/// `E_DuplicatedValue` means the pass already exists and nothing was
/// created. The daily rotation relies on this being reported as success
/// rather than an error -- it is what makes the rotation idempotent without
/// a read-before-write race, and passes can never be deleted to recover.
#[tokio::test]
async fn a_duplicated_name_maps_to_already_exists_rather_than_an_error() {
    let server = MockServer::start().await;
    mount_login(&server).await;
    mount_generate_key(&server).await;

    Mock::given(method("POST"))
        .and(path(CMD_PATH))
        .and(body_string_contains("cmd='create-guest'"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ERR_DUPLICATE))
        .expect(1)
        .mount(&server)
        .await;

    let api = UnleashedApi::from_environment(test_environment(server.uri()))
        .await
        .expect("login against the mock server should succeed");

    let outcome = api
        .create_pass(&CreatePassRequest {
            name: "daily-2026-09-16".to_string(),
            duration_hours: 24,
            share_number: 0,
        })
        .await
        .expect("E_DuplicatedValue must not surface as an error");

    assert!(
        matches!(outcome, CreateOutcome::AlreadyExists),
        "expected AlreadyExists, got {outcome:?}"
    );

    // No guest-list mock is mounted: reaching the re-list would mean the
    // duplicate was treated as a successful create.
    server.verify().await;
}
