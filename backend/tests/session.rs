mod support;

use backend::unleashed::session::{Session, SessionError};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};

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

/// Collects everything logged while `f` runs.
async fn logged<F: std::future::Future>(f: F) -> (F::Output, String) {
    #[derive(Clone)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);
    impl Write for Buffer {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buffer = Buffer(Arc::new(Mutex::new(Vec::new())));
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let output = f.await;
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    (output, text)
}

#[tokio::test]
async fn a_failed_login_request_does_not_leak_the_password() {
    let mock = Mock::start().await;
    // The login page is on a port nothing listens on, so the login GET,
    // which carries the password in its query, fails to connect.
    mock.state.lock().unwrap().login_location =
        Some("http://127.0.0.1:9/admin/login.jsp".to_string());
    let s = session(&mock, PASSWORD);
    let (result, logs) = logged(s.login()).await;
    let error = result.unwrap_err().to_string();
    assert!(!error.contains(PASSWORD), "error leaks the password: {error}");
    assert!(!logs.contains(PASSWORD), "logs leak the password: {logs}");
}

#[tokio::test]
async fn a_bad_login_redirect_does_not_leak_the_password() {
    let mock = Mock::start().await;
    mock.state.lock().unwrap().login_redirect_without_location = true;
    let s = session(&mock, PASSWORD);
    let (result, logs) = logged(s.login()).await;
    let error = result.unwrap_err().to_string();
    assert!(!error.contains(PASSWORD), "error leaks the password: {error}");
    assert!(!logs.contains(PASSWORD), "logs leak the password: {logs}");
}
