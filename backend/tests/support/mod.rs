//! An in-process stand-in for the Unleashed admin pages, modelled on what a
//! real controller was observed to do.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Form, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};

pub const USERNAME: &str = "admin";
pub const PASSWORD: &str = "secret";

#[derive(Debug, Clone)]
pub struct MockPass {
    pub id: u32,
    pub name: String,
    pub key: String,
    pub remarks: String,
    pub share_number: u64,
    pub valid_time: u64,
    pub create_time: i64,
    pub expire_time: i64,
    pub client_macs: Vec<String>,
}

#[derive(Default)]
pub struct MockState {
    /// Session cookie -> (CSRF token, dashboard visited).
    sessions: HashMap<String, (String, bool)>,
    pub logins: u32,
    /// Answer every AJAX call with a redirect to the login page.
    pub always_redirect: bool,
    /// Send the CSRF token in a header (current firmware) or only from
    /// `_csrfTokenVar.jsp` (older firmware).
    pub token_in_script_only: bool,
    /// Answer every create with OK and create nothing.
    pub ignore_creates: bool,
    /// Where `GET /` sends the client to log in, instead of this mock.
    pub login_location: Option<String>,
    /// Answer a successful login with a redirect that has no `Location`.
    pub login_redirect_without_location: bool,
    /// Delay every create, so concurrent creates overlap.
    pub create_delay_ms: u64,
    pub passes: Vec<MockPass>,
    next_id: u32,
    /// Every `mon_createguest.jsp` form received.
    pub create_forms: Vec<HashMap<String, String>>,
}

#[derive(Clone)]
pub struct Mock {
    pub url: String,
    pub state: Arc<Mutex<MockState>>,
}

impl Mock {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(MockState {
            next_id: 1,
            ..Default::default()
        }));
        let app = Router::new()
            .route("/", get(root))
            .route("/admin/login.jsp", get(login))
            .route("/admin/dashboard.jsp", get(dashboard))
            .route("/admin/_csrfTokenVar.jsp", get(csrf_script))
            .route("/admin/_conf.jsp", post(conf))
            .route("/admin/mon_createguest.jsp", post(create_guest))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, state }
    }

    /// Forget every session, as a controller reboot does.
    pub fn expire_sessions(&self) {
        self.state.lock().unwrap().sessions.clear();
    }

    pub fn logins(&self) -> u32 {
        self.state.lock().unwrap().logins
    }

    pub fn add_pass(&self, name: &str, used: bool, expires_in: i64) -> u32 {
        let mut s = self.state.lock().unwrap();
        let id = s.next_id;
        s.next_id += 1;
        let now = now();
        s.passes.push(MockPass {
            id,
            name: name.to_string(),
            key: format!("{:06}", 100_000 + id),
            remarks: String::new(),
            share_number: 0,
            valid_time: 3600,
            create_time: now - 60,
            expire_time: now + expires_in,
            client_macs: if used {
                vec!["00:00:00:00:00:01".to_string()]
            } else {
                vec![]
            },
        });
        id
    }

    pub fn pass_ids(&self) -> Vec<u32> {
        self.state
            .lock()
            .unwrap()
            .passes
            .iter()
            .map(|p| p.id)
            .collect()
    }
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn redirect(to: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, to.to_string())]).into_response()
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| c.trim().strip_prefix("-ejs-session-=").map(str::to_string))
}

/// True when the request carries an activated session and its CSRF token.
fn authorised(state: &MockState, headers: &HeaderMap) -> bool {
    let Some(cookie) = session_cookie(headers) else {
        return false;
    };
    let token = headers.get("X-CSRF-Token").and_then(|v| v.to_str().ok());
    matches!(state.sessions.get(&cookie), Some((t, true)) if Some(t.as_str()) == token)
}

async fn root(State(state): State<Arc<Mutex<MockState>>>) -> Response {
    let location = state.lock().unwrap().login_location.clone();
    redirect(location.as_deref().unwrap_or("/admin/login.jsp"))
}

async fn login(
    State(state): State<Arc<Mutex<MockState>>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let mut s = state.lock().unwrap();
    if params.get("username").map(String::as_str) != Some(USERNAME)
        || params.get("password").map(String::as_str) != Some(PASSWORD)
    {
        return (StatusCode::OK, "<html>login page</html>").into_response();
    }
    s.logins += 1;
    let cookie = format!("s{}", s.logins);
    let token = format!("t{}", s.logins);
    s.sessions.insert(cookie.clone(), (token.clone(), false));
    let mut response = if s.login_redirect_without_location {
        StatusCode::FOUND.into_response()
    } else {
        redirect("/admin/dashboard.jsp")
    };
    let headers = response.headers_mut();
    headers.insert(
        header::SET_COOKIE,
        format!("-ejs-session-={cookie}; Path=/").parse().unwrap(),
    );
    if !s.token_in_script_only {
        headers.insert("HTTP_X_CSRF_TOKEN", token.parse().unwrap());
    }
    response
}

async fn dashboard(State(state): State<Arc<Mutex<MockState>>>, headers: HeaderMap) -> Response {
    let mut s = state.lock().unwrap();
    if let Some(cookie) = session_cookie(&headers)
        && let Some(session) = s.sessions.get_mut(&cookie)
    {
        session.1 = true;
    }
    (StatusCode::OK, "<html>dashboard</html>").into_response()
}

async fn csrf_script(State(state): State<Arc<Mutex<MockState>>>, headers: HeaderMap) -> Response {
    let s = state.lock().unwrap();
    match session_cookie(&headers).and_then(|c| s.sessions.get(&c).cloned()) {
        Some((token, _)) => format!("<script>var csfrToken = '{token}';</script>").into_response(),
        None => redirect("/admin/login.jsp"),
    }
}

async fn conf(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let mut s = state.lock().unwrap();
    if s.always_redirect || !authorised(&s, &headers) {
        return redirect("/admin/login.jsp");
    }
    if body.contains("action='getconf'") && body.contains("comp='guest-list'") {
        let now = now();
        s.passes.retain(|p| p.expire_time > now);
        let guests: String = s.passes.iter().map(render_pass).collect();
        return format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><!DOCTYPE ajax-response><ajax-response>\
             <response type=\"object\" id=\"guest-list.1\"><resultset>{guests}</resultset>\
             </response></ajax-response>"
        )
        .into_response();
    }
    if body.contains("action='delobj'") && body.contains("comp='guest-list'") {
        let ids: Vec<u32> = body
            .split("<guest id='")
            .skip(1)
            .filter_map(|rest| rest.split('\'').next()?.parse().ok())
            .collect();
        s.passes.retain(|p| !ids.contains(&p.id));
        return "<?xml version=\"1.0\" encoding=\"utf-8\"?><ajax-response>\
                <response type=\"object\" id=\"guest-list.1\" /></ajax-response>"
            .into_response();
    }
    (StatusCode::OK, "").into_response()
}

fn render_pass(p: &MockPass) -> String {
    let used = if p.client_macs.is_empty() {
        ""
    } else {
        " used=\"true\""
    };
    let clients: String = p
        .client_macs
        .iter()
        .map(|m| format!("<client mac=\"{m}\" />"))
        .collect();
    format!(
        "<guest shared-guestpass=\"true\" share-number=\"{}\" countdown-by-issued=\"true\" \
         create-time=\"{}\" valid-time=\"{}\" start-time=\"{}\" expire-time=\"{}\" \
         remarks=\"{}\" name=\"{}\" x-key=\"{}\" id=\"{}\" full-name=\"{}\" wlan=\"Guest\" \
         key=\"{}\"{used}>{clients}</guest>",
        p.share_number,
        p.create_time,
        p.valid_time,
        p.create_time,
        p.expire_time,
        p.remarks,
        p.name,
        p.key,
        p.id,
        p.name,
        p.key,
    )
}

fn create_response(result: &str, message: &str, key: &str, name: &str) -> String {
    format!(
        "\n{{\"result\":\"{result}\",\n \"errorMsg\":\"{message}\",\n\"key\":\"{key}\",\n\
         \"fullname\":\"{name}\",\n\"ids\":\"\"}}\n"
    )
}

async fn create_guest(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let delay = state.lock().unwrap().create_delay_ms;
    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    let mut s = state.lock().unwrap();
    if s.always_redirect || !authorised(&s, &headers) {
        return redirect("/admin/login.jsp");
    }
    s.create_forms.push(form.clone());
    let field = |k: &str| form.get(k).cloned().unwrap_or_default();
    let name = field("fullname");

    // Observed on a real controller: a name with whitespace answers OK with
    // the previous pass's details and creates nothing.
    if s.ignore_creates || name.chars().any(char::is_whitespace) {
        let last = s.passes.last().cloned();
        let (key, last_name) = last.map(|p| (p.key, p.name)).unwrap_or_default();
        return create_response("OK", "~~", &key, &last_name).into_response();
    }

    let unit = match field("duration-unit").as_str() {
        "hour" => 3600,
        "day" => 86_400,
        "week" => 604_800,
        _ => 0,
    };
    let valid_time = field("duration").parse::<u64>().unwrap_or(0) * unit;
    let share_number = field("limitnumber").parse().unwrap_or(0);
    let now = now();
    let make = |s: &mut MockState, name: String, key: String, remarks: String| {
        let id = s.next_id;
        s.next_id += 1;
        let name = if name.is_empty() {
            format!("Guest-{id}")
        } else {
            name
        };
        let key = if key.is_empty() {
            format!("{:06}", 100_000 + id)
        } else {
            key
        };
        s.passes.push(MockPass {
            id,
            name,
            key,
            remarks,
            share_number,
            valid_time,
            create_time: now,
            expire_time: now + valid_time as i64,
            client_macs: vec![],
        });
    };

    if field("gentype") == "multiple" {
        let count: u32 = field("createToNum").parse().unwrap_or(0);
        for _ in 0..count {
            make(
                &mut s,
                String::new(),
                String::new(),
                "Batch generation".to_string(),
            );
        }
        return create_response("OK", "~~", "", "null").into_response();
    }

    let key = field("key").to_uppercase();
    if !key.is_empty() && s.passes.iter().any(|p| p.key == key) {
        return create_response(
            "KEY_DUPLICATED",
            "The key key already exists. Please enter a different key.",
            &key,
            &name,
        )
        .into_response();
    }
    make(&mut s, name.clone(), key.clone(), field("remarks"));
    create_response("DONE", "~~", &key, &name).into_response()
}
