//! An authenticated session with the controller's admin pages.

use std::time::Duration;

use reqwest::{Client, StatusCode, Url, header::LOCATION, redirect::Policy};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

const CSRF_RESPONSE_HEADER: &str = "HTTP_X_CSRF_TOKEN";
const CSRF_REQUEST_HEADER: &str = "X-CSRF-Token";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// The controller could not be reached or answered something unexpected.
    Connect(String),
    /// The controller refused the credentials.
    Auth(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "controller connection failed: {e}"),
            Self::Auth(e) => write!(f, "controller authentication failed: {e}"),
        }
    }
}

/// Logs in to the controller and keeps the session alive.
///
/// Every request runs under one lock, so a re-login after the session lapses
/// happens once and the requests waiting behind it reuse the new session.
pub struct Session {
    client: Client,
    root: Url,
    username: String,
    password: String,
    /// The CSRF token of the current session; `None` before the first login.
    csrf: Mutex<Option<String>>,
}

impl Session {
    pub fn new(
        url: &str,
        username: &str,
        password: &str,
        verify_tls: bool,
    ) -> Result<Self, SessionError> {
        let root = Url::parse(url).map_err(|e| SessionError::Connect(format!("bad URL: {e}")))?;
        let client = Client::builder()
            .cookie_store(true)
            .redirect(Policy::none())
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .tls_danger_accept_invalid_certs(!verify_tls)
            .build()
            .map_err(|e| SessionError::Connect(format!("HTTP client: {e}")))?;
        Ok(Self {
            client,
            root,
            username: username.to_string(),
            password: password.to_string(),
            csrf: Mutex::new(None),
        })
    }

    /// Log in now, so a bad URL or bad credentials surface at startup.
    pub async fn login(&self) -> Result<(), SessionError> {
        let mut csrf = self.csrf.lock().await;
        *csrf = Some(self.do_login().await?);
        Ok(())
    }

    /// POST an `<ajax-request>` to `/admin/_conf.jsp`.
    pub async fn conf(&self, xml: &str) -> Result<String, SessionError> {
        self.post("_conf.jsp", Body::Xml(xml.to_string())).await
    }

    /// POST a urlencoded form to an admin page such as `mon_createguest.jsp`.
    pub async fn form(
        &self,
        page: &str,
        fields: &[(&'static str, String)],
    ) -> Result<String, SessionError> {
        self.post(page, Body::Form(fields.to_vec())).await
    }

    async fn post(&self, page: &str, body: Body) -> Result<String, SessionError> {
        let mut csrf = self.csrf.lock().await;
        if csrf.is_none() {
            *csrf = Some(self.do_login().await?);
        }
        for attempt in 0..2 {
            let token = csrf.as_deref().unwrap_or_default();
            let url = self.admin_url(page)?;
            let request = self.client.post(url).header(CSRF_REQUEST_HEADER, token);
            let request = match &body {
                Body::Xml(xml) => request
                    .header(reqwest::header::CONTENT_TYPE, "text/xml")
                    .body(xml.clone()),
                Body::Form(fields) => request.form(fields),
            };
            let response = request.send().await.map_err(connect_error)?;
            if response.status().is_redirection() {
                if attempt == 1 {
                    break;
                }
                info!("Controller session expired, logging in again");
                *csrf = Some(self.do_login().await?);
                continue;
            }
            if !response.status().is_success() {
                return Err(SessionError::Connect(format!(
                    "{page} answered {}",
                    response.status()
                )));
            }
            return response.text().await.map_err(connect_error);
        }
        *csrf = None;
        Err(SessionError::Auth(format!(
            "{page} still redirects to the login page after logging in again"
        )))
    }

    async fn do_login(&self) -> Result<String, SessionError> {
        let login_url = self.find_login_page().await?;
        debug!("Logging in at {login_url}");

        let response = self
            .client
            .get(login_url.clone())
            .query(&[
                ("username", self.username.as_str()),
                ("password", self.password.as_str()),
                ("ok", "Log In"),
            ])
            .send()
            .await
            .map_err(connect_error)?;
        if response.status() == StatusCode::OK {
            return Err(SessionError::Auth(
                "username or password rejected".to_string(),
            ));
        }
        if !response.status().is_redirection() {
            return Err(SessionError::Connect(format!(
                "login answered {}",
                response.status()
            )));
        }
        let header_token = response
            .headers()
            .get(CSRF_RESPONSE_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let landing = redirect_target(&response)?;

        // The session only works for AJAX calls once the landing page
        // (dashboard.jsp) has been loaded.
        self.client
            .get(landing)
            .send()
            .await
            .map_err(connect_error)?;

        match header_token {
            Some(token) => Ok(token),
            None => self.scrape_csrf_token().await,
        }
    }

    /// `GET /` redirects to the login page, sometimes via a second hop when
    /// the address belongs to a member AP rather than the master.
    async fn find_login_page(&self) -> Result<Url, SessionError> {
        let mut url = self
            .root
            .join("/")
            .map_err(|e| SessionError::Connect(e.to_string()))?;
        for _ in 0..2 {
            let response = self.client.get(url).send().await.map_err(connect_error)?;
            let target = redirect_target(&response)?;
            if target.path() != "/" {
                return Ok(target);
            }
            url = target;
        }
        Err(SessionError::Connect(
            "could not find the login page".to_string(),
        ))
    }

    /// Older firmware puts the token in a script instead of a header.
    async fn scrape_csrf_token(&self) -> Result<String, SessionError> {
        let text = self
            .client
            .get(self.admin_url("_csrfTokenVar.jsp")?)
            .send()
            .await
            .map_err(connect_error)?
            .text()
            .await
            .map_err(connect_error)?;
        text.split('\'')
            .nth(1)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .ok_or_else(|| SessionError::Connect("no CSRF token in the login response".to_string()))
    }

    fn admin_url(&self, page: &str) -> Result<Url, SessionError> {
        self.root
            .join(&format!("/admin/{page}"))
            .map_err(|e| SessionError::Connect(e.to_string()))
    }
}

enum Body {
    Xml(String),
    Form(Vec<(&'static str, String)>),
}

fn redirect_target(response: &reqwest::Response) -> Result<Url, SessionError> {
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            SessionError::Connect(format!(
                "expected a redirect from {}, got {}",
                without_query(response.url()),
                response.status()
            ))
        })?;
    response
        .url()
        .join(location)
        .map_err(|e| SessionError::Connect(format!("bad redirect {location:?}: {e}")))
}

/// The login request carries the password in its query, so request URLs
/// never reach logs or error messages.
fn connect_error(e: reqwest::Error) -> SessionError {
    let e = e.without_url();
    warn!("Controller request failed: {e}");
    SessionError::Connect(e.to_string())
}

fn without_query(url: &Url) -> String {
    let mut url = url.clone();
    url.set_query(None);
    url.to_string()
}
