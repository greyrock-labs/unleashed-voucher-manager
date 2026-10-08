mod support;

use backend::unleashed::session::{Session, SessionError};
use support::{Mock, PASSWORD, USERNAME};

const LIST: &str = "<ajax-request action='getconf' comp='guest-list'/>";

fn session(mock: &Mock, password: &str) -> Session {
    Session::new(&mock.url, USERNAME, password, true).unwrap()
}

#[tokio::test]
async fn logs_in_and_lists() {
    let mock = Mock::start().await;
    let s = session(&mock, PASSWORD);
    s.login().await.unwrap();
    let body = s.conf(LIST).await.unwrap();
    assert!(body.contains("<resultset>"), "{body}");
    assert_eq!(mock.logins(), 1);
}

#[tokio::test]
async fn logs_in_lazily_on_first_request() {
    let mock = Mock::start().await;
    let s = session(&mock, PASSWORD);
    s.conf(LIST).await.unwrap();
    assert_eq!(mock.logins(), 1);
}

#[tokio::test]
async fn rejects_bad_credentials() {
    let mock = Mock::start().await;
    let s = session(&mock, "wrong");
    assert!(matches!(s.login().await, Err(SessionError::Auth(_))));
}

#[tokio::test]
async fn reads_the_token_from_the_script_on_older_firmware() {
    let mock = Mock::start().await;
    mock.state.lock().unwrap().token_in_script_only = true;
    let s = session(&mock, PASSWORD);
    assert!(s.conf(LIST).await.unwrap().contains("<resultset>"));
}

#[tokio::test]
async fn logs_in_again_when_the_session_expires() {
    let mock = Mock::start().await;
    let s = session(&mock, PASSWORD);
    s.conf(LIST).await.unwrap();
    mock.expire_sessions();
    assert!(s.conf(LIST).await.unwrap().contains("<resultset>"));
    assert_eq!(mock.logins(), 2);
}

#[tokio::test]
async fn concurrent_requests_share_one_relogin() {
    let mock = Mock::start().await;
    let s = std::sync::Arc::new(session(&mock, PASSWORD));
    s.conf(LIST).await.unwrap();
    mock.expire_sessions();
    let calls: Vec<_> = (0..5)
        .map(|_| {
            let s = s.clone();
            tokio::spawn(async move { s.conf(LIST).await })
        })
        .collect();
    for call in calls {
        call.await.unwrap().unwrap();
    }
    assert_eq!(mock.logins(), 2);
}

#[tokio::test]
async fn gives_up_after_one_relogin() {
    let mock = Mock::start().await;
    let s = session(&mock, PASSWORD);
    s.login().await.unwrap();
    mock.state.lock().unwrap().always_redirect = true;
    assert!(matches!(s.conf(LIST).await, Err(SessionError::Auth(_))));
    assert_eq!(mock.logins(), 2);
}

#[tokio::test]
async fn unreachable_controller_is_a_connect_error() {
    let s = Session::new("http://127.0.0.1:9", USERNAME, PASSWORD, true).unwrap();
    assert!(matches!(s.login().await, Err(SessionError::Connect(_))));
}
