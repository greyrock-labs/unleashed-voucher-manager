use std::{sync::OnceLock, time::Duration};

use reqwest::{Client, ClientBuilder};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::{
    environment::{ENVIRONMENT, Environment},
    models::*,
};

pub static UNLEASHED_API: OnceLock<UnleashedApi> = OnceLock::new();

const LOGIN_PATH: &str = "/user/user_login_guestpass.jsp";
const CMD_PATH: &str = "/user/_cmdstat.jsp";
const REFERER_PATH: &str = "/user/guestinfo.jsp";
const CSRF_MARKER: &str = "var csfrToken = '";

#[derive(Debug)]
pub struct UnleashedApi {
    client: Client,
    environment: &'static Environment,
    csrf: RwLock<Option<String>>,
}

/// Pull the CSRF token out of the inline script the controller emits on
/// every authenticated page. Scraping is unavoidable -- the token is not
/// exposed in a header or cookie.
pub fn extract_csrf_token(html: &str) -> Option<String> {
    let start = html.find(CSRF_MARKER)? + CSRF_MARKER.len();
    let rest = &html[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// The controller answers an expired session with a full HTML login page
/// instead of an XML envelope.
fn is_login_page(body: &str) -> bool {
    body.trim_start().starts_with("<!DOCTYPE html>")
}

impl UnleashedApi {
    pub async fn try_new() -> Result<Self, String> {
        let environment: &Environment = ENVIRONMENT.get().expect("Environment not set");

        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .cookie_store(true)
            .tls_danger_accept_invalid_certs(!environment.unleashed_has_valid_cert)
            .tls_backend_rustls()
            .build()
            .map_err(|e| format!("Failed to build Unleashed reqwest client: {e}"))?;

        let api = Self {
            client,
            environment,
            csrf: RwLock::new(None),
        };
        api.login().await.map_err(|e| e.to_string())?;
        info!("Successfully authenticated to Unleashed controller");
        Ok(api)
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}{}",
            self.environment.unleashed_url.trim_end_matches('/'),
            path
        )
    }

    /// Prime the session cookie, post credentials, and capture the CSRF token.
    pub async fn login(&self) -> Result<(), UnleashedError> {
        // The GET establishes the -ejs-session- cookie the POST needs.
        self.client.get(self.url(LOGIN_PATH)).send().await?;

        let body = self
            .client
            .post(self.url(LOGIN_PATH))
            .form(&[
                ("username", self.environment.unleashed_username.as_str()),
                ("password", self.environment.unleashed_password.as_str()),
                ("ok", "Log in"),
                ("email", ""),
                ("user", ""),
                ("ssid", ""),
            ])
            .send()
            .await?
            .text()
            .await?;

        match extract_csrf_token(&body) {
            Some(token) => {
                *self.csrf.write().await = Some(token);
                Ok(())
            }
            None => Err(UnleashedError::Auth),
        }
    }

    async fn post_ajax(&self, xml: &str) -> Result<String, UnleashedError> {
        let token = self.csrf.read().await.clone().unwrap_or_default();
        Ok(self
            .client
            .post(self.url(CMD_PATH))
            .header("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8")
            .header("X-CSRF-Token", token)
            .header("Referer", self.url(REFERER_PATH))
            .body(xml.to_string())
            .send()
            .await?
            .text()
            .await?)
    }

    /// Issue one AJAX request, re-authenticating once if the session lapsed.
    pub async fn call(
        &self,
        action: &str,
        comp: &str,
        inner: &str,
    ) -> Result<String, UnleashedError> {
        let xml = format!(
            "<ajax-request action='{action}' updater='{comp}.{}.{}' comp='{comp}'>{inner}</ajax-request>",
            chrono::Utc::now().timestamp_millis(),
            std::process::id() % 9000 + 1000,
        );

        let body = self.post_ajax(&xml).await?;
        if !is_login_page(&body) {
            return Ok(body);
        }

        warn!("Unleashed session lapsed, re-authenticating");
        self.login().await?;
        let body = self.post_ajax(&xml).await?;
        if is_login_page(&body) {
            return Err(UnleashedError::SessionLapsed);
        }
        debug!("Re-authenticated and retried successfully");
        Ok(body)
    }
}
