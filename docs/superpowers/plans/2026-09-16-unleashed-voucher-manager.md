# Unleashed Voucher Manager Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the vendored UniFi Voucher Manager into a Ruckus Unleashed guest-pass manager that mints one 24-hour pass per day and shows it on a guest-facing display page.

**Architecture:** Next.js frontend proxying to an Axum backend in one container. `backend/src/unleashed_api.rs` replaces `unifi_api.rs`, speaking the Unleashed AJAX/XML protocol over plain HTTPS with a session cookie and a scraped CSRF token. A daily task creates `daily-YYYY-MM-DD` and relies on the controller's own name-uniqueness check for idempotency. Nothing is ever deleted.

**Tech Stack:** Rust (edition 2024, axum 0.8, reqwest 0.13, quick-xml, chrono-tz), TypeScript/Next.js 16, Docker Buildx Bake, Forgejo Actions, Helm.

**Spec:** `docs/superpowers/specs/2026-09-16-unleashed-voucher-manager-design.md`

## Global Constraints

These apply to every task. Values are copied verbatim from the spec and were each verified against the live controller.

- **Endpoint:** all calls POST to `/user/_cmdstat.jsp`. Login posts to `/user/user_login_guestpass.jsp`.
- **Never implement delete.** The Guest Pass Manager role returns `AD_PrivilegeInsufficient` for every delete route. No delete method, no stub, no feature flag.
- **Durations are always whole hours**, sent as `duration-unit='hour'`. `duration-unit='day'` is silently treated as hours by the controller and would produce a 1-hour pass where 24 were intended.
- **`share-number='0'` means unlimited devices.** The daily pass defaults to `0`. A value of `1` admits exactly one device and locks out every other guest.
- **Pass names:** 1–64 chars, no whitespace, and none of `! # $ & ( ) < > " ' \ | ; ` + backtick + comma.
- **Duplicate names are rejected** by the controller with `E_DuplicatedValue` and nothing is created. This is the idempotency mechanism — treat it as success, never check-then-create.
- **`valid-time` ≠ `expire-time`.** `valid-time` is access granted on first use (set by `duration`). `expire-time` on an unused pass is the fixed 7-day deadline to first use; on a used pass it is `start-time + valid-time`.
- **Silently ignored on create** (never rely on them): `countdown-by-issued`, `expire-time`, `valid-time`, `shared`.
- **TLS:** verify certificates by default; `UNLEASHED_HAS_VALID_CERT=false` disables verification.
- **Commit after every task.** Conventional commit prefixes (`feat:`, `refactor:`, `test:`, `chore:`).

---

### Task 1: Environment configuration and UniFi removal

Replace UniFi configuration with Unleashed configuration, and strip the UniFi
backend so the crate still compiles. This lands first because every later
backend task reads `Environment` -- and because renaming those fields breaks
`unifi_api.rs`, which would leave the crate uncompilable (and therefore
untestable) for the next five tasks.

**Files:**
- Modify: `backend/src/environment.rs` (full rewrite of the struct and `try_new`)
- Delete: `backend/src/unifi_api.rs`
- Modify: `backend/src/lib.rs`, `backend/src/handlers.rs`, `backend/src/tasks.rs`, `backend/src/main.rs` (reduce to a compiling skeleton)
- Test: `backend/src/environment.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: `Environment { unleashed_url: String, unleashed_username: String, unleashed_password: String, unleashed_ssid: String, unleashed_has_valid_cert: bool, backend_bind_host: String, backend_bind_port: u16, timezone: Tz, daily_roll_hour: u32, daily_duration_hours: u32, daily_share_number: u32 }`, the static `ENVIRONMENT: OnceLock<Environment>`, and `Environment::try_new() -> Result<Self, String>`.

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `backend/src/environment.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bool_accepts_common_spellings() {
        assert_eq!(Environment::parse_bool("true"), Ok(true));
        assert_eq!(Environment::parse_bool("YES"), Ok(true));
        assert_eq!(Environment::parse_bool(" 1 "), Ok(true));
        assert_eq!(Environment::parse_bool("false"), Ok(false));
        assert_eq!(Environment::parse_bool("no"), Ok(false));
        assert!(Environment::parse_bool("maybe").is_err());
    }

    #[test]
    fn roll_hour_must_be_a_valid_hour() {
        assert_eq!(Environment::parse_roll_hour("0"), Ok(0));
        assert_eq!(Environment::parse_roll_hour("23"), Ok(23));
        assert!(Environment::parse_roll_hour("24").is_err());
        assert!(Environment::parse_roll_hour("-1").is_err());
    }

    #[test]
    fn duration_hours_must_be_at_least_one() {
        assert_eq!(Environment::parse_duration_hours("24"), Ok(24));
        assert_eq!(Environment::parse_duration_hours("1"), Ok(1));
        assert!(Environment::parse_duration_hours("0").is_err());
    }

    #[test]
    fn url_must_carry_a_scheme() {
        assert!(Environment::validate_url("unleashed.example.com").is_err());
        assert!(Environment::validate_url("https://unleashed.example.com").is_ok());
    }

    /// share-number 0 means unlimited devices and is the daily default.
    /// A value of 1 would admit a single device and lock out every other guest.
    #[test]
    fn share_number_zero_is_valid_and_means_unlimited() {
        assert_eq!(Environment::parse_share_number("0"), Ok(0));
        assert_eq!(Environment::parse_share_number("5"), Ok(5));
        assert!(Environment::parse_share_number("-1").is_err());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test environment`
Expected: FAIL — `parse_roll_hour`, `parse_duration_hours`, `validate_url`, `parse_share_number` do not exist.

- [ ] **Step 3: Rewrite the module**

Replace the whole of `backend/src/environment.rs` above the test module:

```rust
use std::{env, sync::OnceLock};

use chrono_tz::Tz;
use tracing::{error, info};

const DEFAULT_BACKEND_BIND_HOST: &str = "127.0.0.1";
const DEFAULT_BACKEND_BIND_PORT: u16 = 8080;
const DEFAULT_DAILY_ROLL_HOUR: u32 = 4;
const DEFAULT_DAILY_DURATION_HOURS: u32 = 24;
/// 0 means unlimited devices may share the pass.
const DEFAULT_DAILY_SHARE_NUMBER: u32 = 0;

pub static ENVIRONMENT: OnceLock<Environment> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct Environment {
    pub unleashed_url: String,
    pub unleashed_username: String,
    pub unleashed_password: String,
    pub unleashed_ssid: String,
    pub unleashed_has_valid_cert: bool,
    pub backend_bind_host: String,
    pub backend_bind_port: u16,
    pub timezone: Tz,
    pub daily_roll_hour: u32,
    pub daily_duration_hours: u32,
    pub daily_share_number: u32,
}

impl Environment {
    pub fn try_new() -> Result<Self, String> {
        #[cfg(feature = "dotenv")]
        dotenvy::dotenv().map_err(|e| format!("Failed to load .env file: {e}"))?;

        let unleashed_url =
            env::var("UNLEASHED_URL").map_err(|e| format!("UNLEASHED_URL: {e}"))?;
        Self::validate_url(&unleashed_url)?;

        let unleashed_username =
            env::var("UNLEASHED_USERNAME").map_err(|e| format!("UNLEASHED_USERNAME: {e}"))?;
        let unleashed_password =
            env::var("UNLEASHED_PASSWORD").map_err(|e| format!("UNLEASHED_PASSWORD: {e}"))?;
        let unleashed_ssid =
            env::var("UNLEASHED_SSID").map_err(|e| format!("UNLEASHED_SSID: {e}"))?;

        let unleashed_has_valid_cert = match env::var("UNLEASHED_HAS_VALID_CERT") {
            Ok(v) => Self::parse_bool(&v)
                .map_err(|e| format!("Invalid UNLEASHED_HAS_VALID_CERT: {e}"))?,
            Err(_) => true,
        };

        let backend_bind_host =
            env::var("BACKEND_BIND_HOST").unwrap_or(DEFAULT_BACKEND_BIND_HOST.to_owned());
        let backend_bind_port: u16 = match env::var("BACKEND_BIND_PORT") {
            Ok(v) => v
                .parse()
                .map_err(|e| format!("Invalid BACKEND_BIND_PORT: {e}"))?,
            Err(_) => DEFAULT_BACKEND_BIND_PORT,
        };

        let daily_roll_hour = match env::var("DAILY_ROLL_HOUR") {
            Ok(v) => Self::parse_roll_hour(&v).map_err(|e| format!("Invalid DAILY_ROLL_HOUR: {e}"))?,
            Err(_) => DEFAULT_DAILY_ROLL_HOUR,
        };
        let daily_duration_hours = match env::var("DAILY_DURATION_HOURS") {
            Ok(v) => Self::parse_duration_hours(&v)
                .map_err(|e| format!("Invalid DAILY_DURATION_HOURS: {e}"))?,
            Err(_) => DEFAULT_DAILY_DURATION_HOURS,
        };
        let daily_share_number = match env::var("DAILY_SHARE_NUMBER") {
            Ok(v) => Self::parse_share_number(&v)
                .map_err(|e| format!("Invalid DAILY_SHARE_NUMBER: {e}"))?,
            Err(_) => DEFAULT_DAILY_SHARE_NUMBER,
        };

        let timezone: Tz = match env::var("TIMEZONE") {
            Ok(s) => match s.parse() {
                Ok(tz) => {
                    info!("Using timezone: {}", s);
                    tz
                }
                Err(_) => {
                    error!("Using UTC, could not parse timezone: {}", s);
                    Tz::UTC
                }
            },
            Err(_) => {
                info!("TIMEZONE not set, defaulting to UTC");
                Tz::UTC
            }
        };

        Ok(Self {
            unleashed_url,
            unleashed_username,
            unleashed_password,
            unleashed_ssid,
            unleashed_has_valid_cert,
            backend_bind_host,
            backend_bind_port,
            timezone,
            daily_roll_hour,
            daily_duration_hours,
            daily_share_number,
        })
    }

    fn validate_url(url: &str) -> Result<(), String> {
        if url.starts_with("http://") || url.starts_with("https://") {
            Ok(())
        } else {
            Err("UNLEASHED_URL must start with http:// or https://".to_string())
        }
    }

    fn parse_bool(s: &str) -> Result<bool, String> {
        match s.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => Err(format!("Boolean value must be true or false, found: {s}")),
        }
    }

    fn parse_roll_hour(s: &str) -> Result<u32, String> {
        let hour: u32 = s
            .trim()
            .parse()
            .map_err(|_| format!("must be an integer 0-23, found: {s}"))?;
        if hour > 23 {
            return Err(format!("must be 0-23, found: {hour}"));
        }
        Ok(hour)
    }

    fn parse_duration_hours(s: &str) -> Result<u32, String> {
        let hours: u32 = s
            .trim()
            .parse()
            .map_err(|_| format!("must be a positive integer, found: {s}"))?;
        if hours == 0 {
            return Err("must be at least 1 hour".to_string());
        }
        Ok(hours)
    }

    fn parse_share_number(s: &str) -> Result<u32, String> {
        s.trim()
            .parse()
            .map_err(|_| format!("must be a non-negative integer (0 = unlimited), found: {s}"))
    }
}
```

- [ ] **Step 4: Remove the UniFi backend, leaving a compiling skeleton**

Renaming the `Environment` fields breaks `unifi_api.rs`, which breaks
`handlers.rs`, `tasks.rs` and `main.rs`. Rather than leave the crate
uncompilable until Task 6, strip it back to something that builds now. Later
tasks fill it in.

```bash
git rm backend/src/unifi_api.rs
```

`backend/src/lib.rs`:

```rust
pub mod environment;
pub mod handlers;
pub mod models;
pub mod tasks;
```

`backend/src/handlers.rs` — deliberately does not touch `models`, because
Task 2 rewrites that module:

```rust
use axum::response::Json;
use tracing::debug;

pub async fn health_check_handler() -> Json<serde_json::Value> {
    debug!("Received health check request");
    Json(serde_json::json!({ "status": "ok" }))
}
```

`backend/src/tasks.rs`:

```rust
//! Scheduled tasks. The daily pass rotation lands in a later task.
```

`backend/src/main.rs` — keep tracing and environment setup, drop the API
client and the spawned task:

```rust
use axum::{Router, http::{self, Method}, routing::get};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

use backend::{
    environment::{ENVIRONMENT, Environment},
    handlers::*,
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_env_var("BACKEND_LOG_LEVEL")
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let env = match Environment::try_new() {
        Ok(env) => env,
        Err(e) => {
            error!("Failed to load environment variables: {e}");
            std::process::exit(1);
        }
    };
    ENVIRONMENT.set(env).expect("Failed to set environment variables");
    let environment = ENVIRONMENT.get().expect("Environment not set");

    let cors = CorsLayer::new()
        .allow_headers([http::header::CONTENT_TYPE])
        .allow_methods([Method::POST, Method::GET])
        .allow_origin(Any);

    let app = Router::new()
        .route("/api/health", get(health_check_handler))
        .layer(cors);

    let bind_address = format!(
        "{}:{}",
        environment.backend_bind_host, environment.backend_bind_port
    );
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("Could not bind listener");

    info!("Server running on http://{}", bind_address);
    axum::serve(listener, app).await.expect("Axum server should never error");
}
```

- [ ] **Step 5: Verify the whole crate builds and the tests pass**

Run: `cd backend && cargo build && cargo test`
Expected: build succeeds, 5 tests pass. From here every task starts green.

- [ ] **Step 6: Commit**

```bash
git add -A backend
git commit -m "feat: replace UniFi config with Unleashed config"
```

---

### Task 2: Domain model, XML parsing, and pure helpers

Everything here is pure and fully testable against fixtures captured from the real controller.

**Files:**
- Create: `backend/src/models.rs` (full rewrite)
- Create: `backend/tests/fixtures/guest_list.xml`
- Create: `backend/tests/fixtures/generate_key.xml`
- Create: `backend/tests/fixtures/error_privilege.xml`
- Create: `backend/tests/fixtures/error_duplicate.xml`
- Modify: `backend/Cargo.toml` (add `quick-xml`, `thiserror`)
- Test: `backend/src/models.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `Environment` from Task 1.
- Produces:
  - `GuestPass { id, name, code, ssid, created_at: i64, activated_at: Option<i64>, expires_at: i64, valid_time_secs: i64, used: bool, share_number: u32, client_macs: Vec<String>, remarks: String }`
  - `CreatePassRequest { name: String, duration_hours: u32, share_number: u32 }`
  - `UnleashedError` enum with variants `Http(reqwest::Error)`, `Auth`, `SessionLapsed`, `Controller { msg: String }`, `Parse(String)`
  - `parse_guest_list(xml: &str) -> Result<Vec<GuestPass>, UnleashedError>`
  - `parse_generated_key(xml: &str) -> Result<String, UnleashedError>`
  - `controller_error(xml: &str) -> Option<String>`
  - `sanitize_pass_name(raw: &str) -> Result<String, String>`
  - `period_start_date(now: DateTime<Tz>, roll_hour: u32) -> NaiveDate`
  - `daily_pass_name(date: NaiveDate) -> String`
  - `DUPLICATE_NAME_MSG: &str = "E_DuplicatedValue"`

- [ ] **Step 1: Write the fixtures**

These are literal responses captured from firmware 200.19.7.112.238 on 2026-09-16. Do not hand-edit them.

`backend/tests/fixtures/guest_list.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?><!DOCTYPE ajax-response><ajax-response><response type="object" id="system.1789589834629.9179"><response><guest-list><guest shared-guestpass="true" share-number="1" created-by="guestpass" role-id="2" countdown-by-issued="false" create-time="1789492045" valid-time="9244800" start-time="1789492085" expire-time="1798736885" email="" phone-number="" reauth-interval-unit="min" remarks="" name="work-laptop" x-key="008996" id="1" used="true" ssid="Grey Rock Guest" reauth-enabled="false"><client mac="80:d1:ce:06:bc:52" /></guest><guest shared-guestpass="true" share-number="0" created-by="guestpass" role-id="2" countdown-by-issued="false" create-time="1789589893" valid-time="86400" start-time="" expire-time="1790194693" email="" phone-number="" reauth-interval-unit="min" remarks="" name="daily-2026-09-16" x-key="399711" id="2" ssid="Grey Rock Guest" reauth-enabled="false" /></guest-list></response></response></ajax-response>
```

`backend/tests/fixtures/generate_key.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?><!DOCTYPE ajax-response><ajax-response><response type="object" id="system.1789589893100.4410"><xmsg type="0" x-key="399711"/></response></ajax-response>
```

`backend/tests/fixtures/error_privilege.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?><!DOCTYPE ajax-response><ajax-response><response type="object" id="system.1789589923131.5132"><xmsg type="-1" msg="AD_PrivilegeInsufficient" CHICK_ABLILITY="true" lmsg="You do not have authorization to perform this operation."/></response></ajax-response>
```

`backend/tests/fixtures/error_duplicate.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?><!DOCTYPE ajax-response><ajax-response><response type="object" id="system.1789590101222.3311"><xmsg type="-1" msg="E_DuplicatedValue" lmsg="Duplicated value."/></response></ajax-response>
```

- [ ] **Step 2: Write the failing tests**

Add to the bottom of `backend/src/models.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Tz;

    const GUEST_LIST: &str = include_str!("../tests/fixtures/guest_list.xml");
    const GENERATE_KEY: &str = include_str!("../tests/fixtures/generate_key.xml");
    const ERR_PRIVILEGE: &str = include_str!("../tests/fixtures/error_privilege.xml");
    const ERR_DUPLICATE: &str = include_str!("../tests/fixtures/error_duplicate.xml");

    #[test]
    fn parses_a_used_pass_with_its_clients() {
        let passes = parse_guest_list(GUEST_LIST).unwrap();
        let p = passes.iter().find(|p| p.id == "1").unwrap();
        assert_eq!(p.name, "work-laptop");
        assert_eq!(p.code, "008996");
        assert_eq!(p.ssid, "Grey Rock Guest");
        assert!(p.used);
        assert_eq!(p.activated_at, Some(1789492085));
        assert_eq!(p.valid_time_secs, 9244800);
        assert_eq!(p.client_macs, vec!["80:d1:ce:06:bc:52".to_string()]);
    }

    /// An unused pass carries start-time="" -- it must parse as None, not 0.
    #[test]
    fn parses_an_unused_pass_with_no_activation() {
        let passes = parse_guest_list(GUEST_LIST).unwrap();
        let p = passes.iter().find(|p| p.id == "2").unwrap();
        assert_eq!(p.name, "daily-2026-09-16");
        assert_eq!(p.activated_at, None);
        assert!(!p.used);
        assert_eq!(p.share_number, 0);
        assert!(p.client_macs.is_empty());
    }

    #[test]
    fn extracts_the_generated_key() {
        assert_eq!(parse_generated_key(GENERATE_KEY).unwrap(), "399711");
    }

    #[test]
    fn detects_controller_errors() {
        assert_eq!(
            controller_error(ERR_PRIVILEGE).as_deref(),
            Some("AD_PrivilegeInsufficient")
        );
        assert_eq!(
            controller_error(ERR_DUPLICATE).as_deref(),
            Some(DUPLICATE_NAME_MSG)
        );
        assert_eq!(controller_error(GENERATE_KEY), None);
    }

    #[test]
    fn sanitizes_names_to_the_controller_charset() {
        assert_eq!(sanitize_pass_name("Guest Pass").unwrap(), "Guest-Pass");
        assert_eq!(sanitize_pass_name("a,b;c|d").unwrap(), "abcd");
        assert_eq!(sanitize_pass_name("  spaced  ").unwrap(), "spaced");
        assert!(sanitize_pass_name("").is_err());
        assert!(sanitize_pass_name("!!!").is_err());
        assert_eq!(sanitize_pass_name(&"x".repeat(100)).unwrap().len(), 64);
    }

    /// Before the roll hour the active period still belongs to the previous
    /// day, so the display never falls into a window with no pass.
    #[test]
    fn period_start_respects_the_roll_hour() {
        let tz = Tz::America__New_York;
        let before = tz.with_ymd_and_hms(2026, 9, 17, 2, 0, 0).unwrap();
        let after = tz.with_ymd_and_hms(2026, 9, 17, 5, 0, 0).unwrap();
        assert_eq!(
            period_start_date(before, 4),
            chrono::NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()
        );
        assert_eq!(
            period_start_date(after, 4),
            chrono::NaiveDate::from_ymd_opt(2026, 9, 17).unwrap()
        );
    }

    #[test]
    fn daily_names_are_date_stamped() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 9, 16).unwrap();
        assert_eq!(daily_pass_name(d), "daily-2026-09-16");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd backend && cargo test models`
Expected: FAIL — `parse_guest_list` and friends do not exist.

- [ ] **Step 4: Add the dependencies**

In `backend/Cargo.toml`, add to `[dependencies]`:

```toml
quick-xml = "0.38"
thiserror = "2.0"
```

- [ ] **Step 5: Write the implementation**

Replace the whole of `backend/src/models.rs` above the test module:

```rust
use chrono::{DateTime, Datelike, NaiveDate, Timelike};
use chrono_tz::Tz;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

/// Returned by the controller when a pass name is already taken. Nothing is
/// created. The daily task treats this as success -- it is what makes the
/// rotation idempotent without a read-before-write race.
pub const DUPLICATE_NAME_MSG: &str = "E_DuplicatedValue";

/// Rejected by the controller in pass names, alongside all whitespace.
const FORBIDDEN_NAME_CHARS: &[char] = &[
    '!', '#', '$', '&', '(', ')', '<', '>', '"', '\'', '\\', '|', ';', '`', ',',
];
const MAX_NAME_LEN: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum UnleashedError {
    #[error("transport error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("authentication failed -- check UNLEASHED_USERNAME and UNLEASHED_PASSWORD")]
    Auth,
    #[error("session lapsed and could not be re-established")]
    SessionLapsed,
    #[error("controller rejected the request: {msg}")]
    Controller { msg: String },
    #[error("could not parse controller response: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuestPass {
    pub id: String,
    pub name: String,
    pub code: String,
    pub ssid: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    /// None until the guest first uses the pass.
    #[serde(rename = "activatedAt")]
    pub activated_at: Option<i64>,
    /// For an unused pass this is the deadline to first use (creation + 7
    /// days, fixed by the controller). For a used pass it is
    /// `activated_at + valid_time_secs`.
    #[serde(rename = "expiresAt")]
    pub expires_at: i64,
    /// Seconds of access granted once the pass is first used.
    #[serde(rename = "validTimeSecs")]
    pub valid_time_secs: i64,
    pub used: bool,
    /// 0 means unlimited devices.
    #[serde(rename = "shareNumber")]
    pub share_number: u32,
    #[serde(rename = "clientMacs")]
    pub client_macs: Vec<String>,
    pub remarks: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePassRequest {
    pub name: String,
    #[serde(rename = "durationHours")]
    pub duration_hours: u32,
    #[serde(rename = "shareNumber")]
    pub share_number: u32,
}

#[derive(Debug, Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
    /// False when today's pass is missing and only a stale one was found.
    #[serde(rename = "dailyPassCurrent")]
    pub daily_pass_current: bool,
}

fn attr_map(e: &quick_xml::events::BytesStart) -> Result<Vec<(String, String)>, UnleashedError> {
    let mut out = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|err| UnleashedError::Parse(err.to_string()))?;
        let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
        let val = a
            .unescape_value()
            .map_err(|err| UnleashedError::Parse(err.to_string()))?
            .to_string();
        out.push((key, val));
    }
    Ok(out)
}

fn get<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn pass_from_attrs(attrs: &[(String, String)]) -> GuestPass {
    // start-time is the empty string on an unused pass, so a plain parse()
    // failure must map to None rather than 0.
    let activated_at = get(attrs, "start-time")
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok());

    GuestPass {
        id: get(attrs, "id").unwrap_or_default().to_string(),
        name: get(attrs, "name").unwrap_or_default().to_string(),
        code: get(attrs, "x-key").unwrap_or_default().to_string(),
        ssid: get(attrs, "ssid").unwrap_or_default().to_string(),
        created_at: get(attrs, "create-time")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        activated_at,
        expires_at: get(attrs, "expire-time")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        valid_time_secs: get(attrs, "valid-time")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        used: get(attrs, "used") == Some("true"),
        share_number: get(attrs, "share-number")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        client_macs: Vec::new(),
        remarks: get(attrs, "remarks").unwrap_or_default().to_string(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EventKind {
    Start,
    Empty,
}

/// Parse a `getstat system <guest-list/>` response.
pub fn parse_guest_list(xml: &str) -> Result<Vec<GuestPass>, UnleashedError> {
    if let Some(msg) = controller_error(xml) {
        return Err(UnleashedError::Controller { msg });
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut passes: Vec<GuestPass> = Vec::new();
    let mut depth_in_guest = false;

    loop {
        match reader
            .read_event()
            .map_err(|e| UnleashedError::Parse(e.to_string()))?
        {
            ev @ (Event::Start(_) | Event::Empty(_)) => {
                let ev_kind = if matches!(ev, Event::Start(_)) {
                    EventKind::Start
                } else {
                    EventKind::Empty
                };
                let e = match &ev {
                    Event::Start(e) => e.clone(),
                    Event::Empty(e) => e.clone(),
                    _ => unreachable!(),
                };
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attrs = attr_map(&e)?;
                match name.as_str() {
                    "guest" => {
                        passes.push(pass_from_attrs(&attrs));
                        // A self-closing <guest/> never emits an End event,
                        // so only a Start may open the scope -- otherwise a
                        // later <client> would attach to the wrong pass.
                        depth_in_guest = matches!(ev_kind, EventKind::Start);
                    }
                    "client" => {
                        if depth_in_guest
                            && let Some(mac) = get(&attrs, "mac")
                            && let Some(last) = passes.last_mut()
                        {
                            last.client_macs.push(mac.to_string());
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                if e.name().as_ref() == b"guest" {
                    depth_in_guest = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(passes)
}

/// Extract `x-key` from a `generate-guest-key` response.
pub fn parse_generated_key(xml: &str) -> Result<String, UnleashedError> {
    if let Some(msg) = controller_error(xml) {
        return Err(UnleashedError::Controller { msg });
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader
            .read_event()
            .map_err(|e| UnleashedError::Parse(e.to_string()))?
        {
            Event::Start(e) | Event::Empty(e) => {
                if e.name().as_ref() == b"xmsg" {
                    let attrs = attr_map(&e)?;
                    if let Some(key) = get(&attrs, "x-key")
                        && !key.is_empty()
                    {
                        return Ok(key.to_string());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Err(UnleashedError::Parse(
        "no x-key in generate-guest-key response".to_string(),
    ))
}

/// Return the controller's error code when the response carries
/// `<xmsg type="-1" msg="..."/>`, otherwise None.
pub fn controller_error(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"xmsg"
                    && let Ok(attrs) = attr_map(&e)
                    && get(&attrs, "type") == Some("-1")
                {
                    return get(&attrs, "msg").map(|s| s.to_string());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

/// Coerce a name into the controller's accepted charset.
/// Whitespace becomes '-', forbidden characters are dropped, and the result
/// is truncated to 64 characters.
pub fn sanitize_pass_name(raw: &str) -> Result<String, String> {
    let collapsed: String = raw
        .trim()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .filter(|c| !FORBIDDEN_NAME_CHARS.contains(c))
        .collect();

    let truncated: String = collapsed.chars().take(MAX_NAME_LEN).collect();
    let trimmed = truncated.trim_matches('-').to_string();

    if trimmed.is_empty() {
        return Err("name is empty after removing characters the controller rejects".to_string());
    }
    Ok(trimmed)
}

/// The date of the period currently in effect. Before `roll_hour` the active
/// period still began on the previous day.
pub fn period_start_date(now: DateTime<Tz>, roll_hour: u32) -> NaiveDate {
    let date = NaiveDate::from_ymd_opt(now.year(), now.month(), now.day())
        .expect("valid date from a valid DateTime");
    if now.hour() < roll_hour {
        date.pred_opt().expect("date has a predecessor")
    } else {
        date
    }
}

pub fn daily_pass_name(date: NaiveDate) -> String {
    format!("daily-{}", date.format("%Y-%m-%d"))
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd backend && cargo test models`
Expected: PASS, 7 tests.

- [ ] **Step 7: Commit**

```bash
git add backend/src/models.rs backend/tests/fixtures backend/Cargo.toml backend/Cargo.lock
git commit -m "feat: add Unleashed guest pass model and XML parsing"
```

---

### Task 3: Unleashed session — login, CSRF, and transparent re-login

**Files:**
- Create: `backend/src/unleashed_api.rs`
- Modify: `backend/src/lib.rs`
- Modify: `backend/Cargo.toml` (reqwest `cookies` feature, dev-dep `wiremock`)
- Test: `backend/tests/session.rs`

**Interfaces:**
- Consumes: `Environment` (Task 1), `UnleashedError` (Task 2).
- Produces:
  - `UnleashedApi` with `pub async fn try_new() -> Result<Self, String>`
  - `pub async fn call(&self, action: &str, comp: &str, inner: &str) -> Result<String, UnleashedError>`
  - `pub static UNLEASHED_API: OnceLock<UnleashedApi>`
  - `pub fn extract_csrf_token(html: &str) -> Option<String>`

- [ ] **Step 1: Add dependencies**

In `backend/Cargo.toml`, change the `reqwest` line and add a dev-dependency section:

```toml
reqwest = { version = "0.13.4", features = ["json", "query", "cookies"] }

[dev-dependencies]
wiremock = "0.6"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

- [ ] **Step 2: Write the failing tests**

Create `backend/tests/session.rs`:

```rust
use backend::unleashed_api::extract_csrf_token;

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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd backend && cargo test --test session`
Expected: FAIL — module `unleashed_api` does not exist.

- [ ] **Step 4: Write the session implementation**

Create `backend/src/unleashed_api.rs`:

```rust
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
```

In `backend/src/lib.rs`, replace the `unifi_api` line:

```rust
pub mod environment;
pub mod handlers;
pub mod models;
pub mod tasks;
pub mod unleashed_api;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd backend && cargo test --test session`
Expected: PASS, 2 tests, and `cargo build` still succeeds — Task 1 already reduced `handlers.rs` and `tasks.rs` to a compiling skeleton.

- [ ] **Step 6: Commit**

```bash
git add backend/src/unleashed_api.rs backend/src/lib.rs backend/Cargo.toml backend/Cargo.lock backend/tests/session.rs
git commit -m "feat: add Unleashed session with CSRF handling and re-login"
```

---

### Task 4: Client operations — list and create

**Files:**
- Modify: `backend/src/unleashed_api.rs`
- Delete: `backend/src/unifi_api.rs`
- Test: `backend/tests/client_ops.rs`

**Interfaces:**
- Consumes: `UnleashedApi::call` (Task 3), `parse_guest_list`, `parse_generated_key`, `controller_error`, `sanitize_pass_name`, `DUPLICATE_NAME_MSG`, `CreatePassRequest`, `GuestPass` (Task 2).
- Produces:
  - `pub async fn list_passes(&self) -> Result<Vec<GuestPass>, UnleashedError>`
  - `pub async fn generate_key(&self) -> Result<String, UnleashedError>`
  - `pub async fn create_pass(&self, req: &CreatePassRequest) -> Result<CreateOutcome, UnleashedError>`
  - `pub enum CreateOutcome { Created(Box<GuestPass>), AlreadyExists }`

- [ ] **Step 1: Write the failing tests**

Create `backend/tests/client_ops.rs`:

```rust
use backend::models::{controller_error, parse_guest_list, DUPLICATE_NAME_MSG};

const GUEST_LIST: &str = include_str!("fixtures/guest_list.xml");
const ERR_DUPLICATE: &str = include_str!("fixtures/error_duplicate.xml");
const ERR_PRIVILEGE: &str = include_str!("fixtures/error_privilege.xml");

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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test --test client_ops`
Expected: FAIL to compile — `fixtures/` is resolved relative to `backend/tests/`, so this passes only once Task 2's fixtures exist. If Task 2 is done, this compiles and passes; the real work of this task is the two methods below.

- [ ] **Step 3: Add the operations**

Append to `impl UnleashedApi` in `backend/src/unleashed_api.rs`:

```rust
    /// All guest passes known to the controller.
    pub async fn list_passes(&self) -> Result<Vec<GuestPass>, UnleashedError> {
        let body = self.call("getstat", "system", "<guest-list/>").await?;
        parse_guest_list(&body)
    }

    /// Ask the controller to mint a pass code for our SSID.
    pub async fn generate_key(&self) -> Result<String, UnleashedError> {
        let inner = format!(
            "<xcmd cmd='generate-guest-key' ssid='{}'/>",
            xml_escape(&self.environment.unleashed_ssid)
        );
        let body = self.call("docmd", "system", &inner).await?;
        parse_generated_key(&body)
    }

    /// Create a single guest pass.
    ///
    /// `duration_hours` is always sent with `duration-unit='hour'`: the
    /// controller silently treats 'day' as hours, so `1 day` would yield a
    /// one-hour pass.
    pub async fn create_pass(
        &self,
        req: &CreatePassRequest,
    ) -> Result<CreateOutcome, UnleashedError> {
        let name = sanitize_pass_name(&req.name)
            .map_err(|e| UnleashedError::Controller { msg: e })?;

        let key = self.generate_key().await?;
        let inner = format!(
            "<xcmd cmd='create-guest' name='{}' ssid='{}' x-key='{}' \
             duration='{}' duration-unit='hour' share-number='{}' \
             reauth-enabled='false'/>",
            xml_escape(&name),
            xml_escape(&self.environment.unleashed_ssid),
            xml_escape(&key),
            req.duration_hours,
            req.share_number,
        );

        let body = self.call("docmd", "system", &inner).await?;

        if let Some(msg) = controller_error(&body) {
            if msg == DUPLICATE_NAME_MSG {
                return Ok(CreateOutcome::AlreadyExists);
            }
            return Err(UnleashedError::Controller { msg });
        }

        // The create response carries the pass, but re-listing is the only
        // way to get the controller-assigned id consistently.
        let created = self
            .list_passes()
            .await?
            .into_iter()
            .find(|p| p.name == name)
            .ok_or_else(|| {
                UnleashedError::Parse(format!("created pass '{name}' not found in list"))
            })?;

        Ok(CreateOutcome::Created(Box::new(created)))
    }
```

Add above the `impl` block:

```rust
#[derive(Debug)]
pub enum CreateOutcome {
    Created(Box<GuestPass>),
    /// The controller rejected the name as a duplicate, so the pass already
    /// existed and nothing was created.
    AlreadyExists,
}

/// Attribute values are interpolated into XML, so escape them.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}
```

- [ ] **Step 4: Delete the UniFi client**

```bash
git rm backend/src/unifi_api.rs
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd backend && cargo test --test client_ops`
Expected: PASS, 2 tests.

- [ ] **Step 6: Commit**

```bash
git add -A backend
git commit -m "feat: add guest pass list and create operations"
```

---

### Task 5: HTTP API and server wiring

**Files:**
- Modify: `backend/src/handlers.rs` (full rewrite)
- Modify: `backend/src/main.rs`
- Test: `backend/src/handlers.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `UnleashedApi::{list_passes, create_pass}`, `CreateOutcome` (Task 4), `period_start_date`, `daily_pass_name` (Task 2).
- Produces: `resolve_daily_pass(passes: &[GuestPass], expected_name: &str) -> Option<(&GuestPass, bool)>` returning the pass and whether it is current; handlers `health_check_handler`, `list_passes_handler`, `create_pass_handler`, `daily_pass_handler`.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `backend/src/handlers.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use self::backend_test_support::pass;

    mod backend_test_support {
        use crate::models::GuestPass;
        pub fn pass(name: &str, created_at: i64) -> GuestPass {
            GuestPass {
                id: name.to_string(),
                name: name.to_string(),
                code: "000000".to_string(),
                ssid: "Test".to_string(),
                created_at,
                activated_at: None,
                expires_at: created_at + 604800,
                valid_time_secs: 86400,
                used: false,
                share_number: 0,
                client_macs: vec![],
                remarks: String::new(),
            }
        }
    }

    #[test]
    fn prefers_the_pass_for_the_current_period() {
        let passes = vec![pass("daily-2026-09-15", 100), pass("daily-2026-09-16", 200)];
        let (found, current) = resolve_daily_pass(&passes, "daily-2026-09-16").unwrap();
        assert_eq!(found.name, "daily-2026-09-16");
        assert!(current);
    }

    /// If the roll never happened, show the newest daily pass rather than
    /// nothing, and report it as not current.
    #[test]
    fn falls_back_to_the_newest_daily_pass_when_today_is_missing() {
        let passes = vec![pass("daily-2026-09-14", 100), pass("daily-2026-09-15", 200)];
        let (found, current) = resolve_daily_pass(&passes, "daily-2026-09-16").unwrap();
        assert_eq!(found.name, "daily-2026-09-15");
        assert!(!current);
    }

    #[test]
    fn ignores_passes_that_are_not_daily() {
        let passes = vec![pass("work-laptop", 999)];
        assert!(resolve_daily_pass(&passes, "daily-2026-09-16").is_none());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd backend && cargo test handlers`
Expected: FAIL — `resolve_daily_pass` does not exist.

- [ ] **Step 3: Rewrite the handlers**

Replace the whole of `backend/src/handlers.rs` above the test module:

```rust
use axum::{http::StatusCode, response::Json};
use chrono::Utc;
use tracing::{debug, error};

use crate::{
    environment::ENVIRONMENT,
    models::*,
    unleashed_api::{CreateOutcome, UNLEASHED_API},
};

pub const DAILY_NAME_PREFIX: &str = "daily-";

/// Find the pass to display. Returns the pass and whether it is the one the
/// current period expects; a stale fallback keeps the display populated when
/// a roll was missed.
pub fn resolve_daily_pass<'a>(
    passes: &'a [GuestPass],
    expected_name: &str,
) -> Option<(&'a GuestPass, bool)> {
    if let Some(p) = passes.iter().find(|p| p.name == expected_name) {
        return Some((p, true));
    }
    passes
        .iter()
        .filter(|p| p.name.starts_with(DAILY_NAME_PREFIX))
        .max_by_key(|p| p.created_at)
        .map(|p| (p, false))
}

fn expected_daily_name() -> String {
    let env = ENVIRONMENT.get().expect("Environment not set");
    let now = Utc::now().with_timezone(&env.timezone);
    daily_pass_name(period_start_date(now, env.daily_roll_hour))
}

fn to_status(e: &UnleashedError) -> StatusCode {
    match e {
        UnleashedError::Auth | UnleashedError::SessionLapsed => StatusCode::BAD_GATEWAY,
        UnleashedError::Controller { .. } => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn list_passes_handler() -> Result<Json<Vec<GuestPass>>, StatusCode> {
    debug!("Received request to list guest passes");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");
    match client.list_passes().await {
        Ok(passes) => Ok(Json(passes)),
        Err(e) => {
            error!("Failed to list guest passes: {}", e);
            Err(to_status(&e))
        }
    }
}

pub async fn create_pass_handler(
    Json(request): Json<CreatePassRequest>,
) -> Result<Json<GuestPass>, StatusCode> {
    debug!("Received request to create a guest pass");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");
    match client.create_pass(&request).await {
        Ok(CreateOutcome::Created(pass)) => Ok(Json(*pass)),
        Ok(CreateOutcome::AlreadyExists) => Err(StatusCode::CONFLICT),
        Err(e) => {
            error!("Failed to create guest pass: {}", e);
            Err(to_status(&e))
        }
    }
}

pub async fn daily_pass_handler() -> Result<Json<GuestPass>, StatusCode> {
    debug!("Received request for today's guest pass");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");
    let passes = client.list_passes().await.map_err(|e| {
        error!("Failed to list guest passes: {}", e);
        to_status(&e)
    })?;

    match resolve_daily_pass(&passes, &expected_daily_name()) {
        Some((pass, _current)) => Ok(Json(pass.clone())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn health_check_handler() -> Result<Json<HealthCheckResponse>, StatusCode> {
    debug!("Received health check request");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");
    let daily_pass_current = match client.list_passes().await {
        Ok(passes) => resolve_daily_pass(&passes, &expected_daily_name())
            .map(|(_, current)| current)
            .unwrap_or(false),
        Err(_) => false,
    };

    Ok(Json(HealthCheckResponse {
        status: "ok".to_string(),
        daily_pass_current,
    }))
}
```

- [ ] **Step 4: Wire up the router**

In `backend/src/main.rs`, replace the imports and the `Router` block:

```rust
use backend::{
    environment::{ENVIRONMENT, Environment},
    handlers::*,
    tasks::run_daily_rotation,
    unleashed_api::{UNLEASHED_API, UnleashedApi},
};
```

```rust
    loop {
        match UnleashedApi::try_new().await {
            Ok(api) => {
                UNLEASHED_API.set(api).expect("Failed to set UnleashedApi");
                info!("Successfully connected to Unleashed controller");
                break;
            }
            Err(e) => {
                error!("Failed to initialize UnleashedApi: {}", e);
                warn!("Retrying connection in 5 seconds...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    tokio::spawn(run_daily_rotation());

    let cors = CorsLayer::new()
        .allow_headers([http::header::CONTENT_TYPE])
        .allow_methods([Method::POST, Method::GET])
        .allow_origin(Any);

    let app = Router::new()
        .route("/api/health", get(health_check_handler))
        .route("/api/passes", get(list_passes_handler))
        .route("/api/passes", post(create_pass_handler))
        .route("/api/passes/daily", get(daily_pass_handler))
        .layer(cors);
```

Also change the `routing` import to drop `delete`:

```rust
use axum::{
    Router,
    http::{self, Method},
    routing::{get, post},
};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd backend && cargo test handlers`
Expected: PASS, 3 tests. `cargo build` fails until Task 6 supplies `run_daily_rotation`, which this task's `main.rs` now spawns — that is the only outstanding symbol.

- [ ] **Step 6: Commit**

```bash
git add backend/src/handlers.rs backend/src/main.rs
git commit -m "feat: replace voucher routes with guest pass routes"
```

---

### Task 6: Daily rotation task

**Files:**
- Modify: `backend/src/tasks.rs` (full rewrite)
- Test: `backend/src/tasks.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `period_start_date`, `daily_pass_name`, `CreatePassRequest` (Task 2), `UnleashedApi::create_pass`, `CreateOutcome` (Task 4).
- Produces: `pub async fn run_daily_rotation()`, `pub fn seconds_until_next_roll(now: DateTime<Tz>, roll_hour: u32) -> i64`.

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `backend/src/tasks.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Tz;

    #[test]
    fn sleeps_until_todays_roll_when_it_is_still_ahead() {
        let now = Tz::UTC.with_ymd_and_hms(2026, 9, 16, 1, 0, 0).unwrap();
        assert_eq!(seconds_until_next_roll(now, 4), 3 * 3600);
    }

    #[test]
    fn sleeps_until_tomorrows_roll_once_today_has_passed() {
        let now = Tz::UTC.with_ymd_and_hms(2026, 9, 16, 5, 0, 0).unwrap();
        assert_eq!(seconds_until_next_roll(now, 4), 23 * 3600);
    }

    #[test]
    fn never_returns_a_non_positive_delay() {
        let now = Tz::UTC.with_ymd_and_hms(2026, 9, 16, 4, 0, 0).unwrap();
        assert!(seconds_until_next_roll(now, 4) > 0);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test tasks`
Expected: FAIL — `seconds_until_next_roll` does not exist.

- [ ] **Step 3: Write the implementation**

Replace the whole of `backend/src/tasks.rs` above the test module:

```rust
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use chrono_tz::Tz;
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};

use crate::{
    environment::ENVIRONMENT,
    models::{CreatePassRequest, daily_pass_name, period_start_date},
    unleashed_api::{CreateOutcome, UNLEASHED_API},
};

/// Seconds from `now` until the next occurrence of `roll_hour` local time.
/// Always strictly positive, so a task that wakes exactly on the boundary
/// cannot spin.
pub fn seconds_until_next_roll(now: DateTime<Tz>, roll_hour: u32) -> i64 {
    let tz = now.timezone();
    let today = now.date_naive();

    let candidate = today
        .and_hms_opt(roll_hour, 0, 0)
        .expect("valid roll hour")
        .and_local_timezone(tz)
        .latest()
        .expect("roll time resolvable in timezone");

    let target = if candidate > now {
        candidate
    } else {
        (today + ChronoDuration::days(1))
            .and_hms_opt(roll_hour, 0, 0)
            .expect("valid roll hour")
            .and_local_timezone(tz)
            .latest()
            .expect("roll time resolvable in timezone")
    };

    (target - now).num_seconds().max(1)
}

/// Ensure a pass exists for the period currently in effect.
///
/// The controller rejects duplicate names, so this simply attempts the
/// create and treats `AlreadyExists` as success. There is no
/// read-before-write, which means concurrent replicas cannot both mint a
/// code for the same day.
async fn ensure_daily_pass() {
    let env = ENVIRONMENT.get().expect("Environment not set");
    let client = UNLEASHED_API.get().expect("UnleashedApi not initialized");

    let now = Utc::now().with_timezone(&env.timezone);
    let name = daily_pass_name(period_start_date(now, env.daily_roll_hour));

    let request = CreatePassRequest {
        name: name.clone(),
        duration_hours: env.daily_duration_hours,
        share_number: env.daily_share_number,
    };

    match client.create_pass(&request).await {
        Ok(CreateOutcome::Created(pass)) => {
            info!("Created daily guest pass {} (code {})", pass.name, pass.code)
        }
        Ok(CreateOutcome::AlreadyExists) => {
            info!("Daily guest pass {} already exists", name)
        }
        Err(e) => error!("Failed to create daily guest pass {}: {}", name, e),
    }
}

pub async fn run_daily_rotation() {
    let env = ENVIRONMENT.get().expect("Environment not set");

    // Mint immediately on startup so a fresh deploy is never without a code.
    ensure_daily_pass().await;

    loop {
        let now = Utc::now().with_timezone(&env.timezone);
        let secs = seconds_until_next_roll(now, env.daily_roll_hour);
        info!(
            "Next guest pass roll at {:02}:00 ({}), in {}h {}m",
            env.daily_roll_hour,
            env.timezone,
            secs / 3600,
            (secs % 3600) / 60
        );

        sleep(Duration::from_secs(secs as u64)).await;

        if UNLEASHED_API.get().is_none() {
            warn!("Unleashed client not ready, skipping this roll");
            continue;
        }
        ensure_daily_pass().await;
    }
}
```

- [ ] **Step 4: Run the full test suite**

Run: `cd backend && cargo test`
Expected: PASS, all tests. The crate now builds — `cargo build` should succeed.

- [ ] **Step 5: Commit**

```bash
git add backend/src/tasks.rs
git commit -m "feat: replace daily purge with daily pass rotation"
```

---

### Task 7: Frontend types and API client

**Files:**
- Modify: `frontend/src/types/voucher.ts` → replace contents (keep the path; renaming ripples through many imports and buys nothing)
- Modify: `frontend/src/utils/api.ts`

**Interfaces:**
- Consumes: the backend routes from Task 5.
- Produces: `GuestPass` interface, `PassCreateData`, and the `api` object with `getAllPasses`, `getDailyPass`, `createPass`.

- [ ] **Step 1: Replace the types**

Replace all of `frontend/src/types/voucher.ts`:

```ts
export interface GuestPass {
  id: string;
  name: string;
  code: string;
  ssid: string;
  createdAt: number;
  /** null until the guest first uses the pass */
  activatedAt: number | null;
  /**
   * For an unused pass this is the deadline to first use (fixed by the
   * controller at 7 days). For a used pass it is activatedAt + validTimeSecs.
   */
  expiresAt: number;
  /** seconds of access granted once the pass is first used */
  validTimeSecs: number;
  used: boolean;
  /** 0 means unlimited devices */
  shareNumber: number;
  clientMacs: string[];
  remarks: string;
}

export interface PassCreateData {
  name: string;
  durationHours: number;
  shareNumber: number;
}
```

- [ ] **Step 2: Replace the API client**

Replace all of `frontend/src/utils/api.ts`:

```ts
import { GuestPass, PassCreateData } from "@/types/voucher";
import { notifyVouchersUpdated } from "./actions";

async function call<T>(endpoint: string, opts: RequestInit = {}) {
  const res = await fetch(`/rust-api/${endpoint}`, {
    headers: { "Content-Type": "application/json" },
    ...opts,
  });
  if (!res.ok) {
    const error = new Error(res.statusText);
    (error as any).status = res.status;
    throw error;
  }
  return res.json() as Promise<T>;
}

export const MIN_PASS_DURATION_HOURS = 1;
/** One year. The controller counts duration in hours only. */
export const MAX_PASS_DURATION_HOURS = 8760;

/** 0 is valid and means unlimited devices. */
export const MIN_PASS_SHARES = 0;
export const MAX_PASS_SHARES = 1000;

export const api = {
  getAllPasses: () => call<GuestPass[]>("/passes"),

  getDailyPass: () => call<GuestPass>("/passes/daily"),

  createPass: async (data: PassCreateData) => {
    const result = await call<GuestPass>("/passes", {
      method: "POST",
      body: JSON.stringify(data),
    });
    await notifyVouchersUpdated();
    return result;
  },
};
```

- [ ] **Step 3: Verify the types compile in isolation**

Run: `cd frontend && npx tsc --noEmit`
Expected: FAIL, but only with errors in components that still reference removed fields — those are fixed in Task 8. Confirm there are no errors reported inside `types/voucher.ts` or `utils/api.ts` themselves.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/types frontend/src/utils/api.ts
git commit -m "refactor: replace voucher types with guest pass types"
```

---

### Task 8: Strip removed features and rebrand the frontend

**Files:**
- Delete: `frontend/src/app/print/`, `frontend/src/app/welcome/`, `frontend/src/app/kiosk/`, `frontend/src/components/tabs/TestTab.tsx`, `frontend/src/utils/print.ts`, `frontend/src/utils/ipv4.ts`, `frontend/src/types/print.ts`
- Modify: `scripts/entrypoint.sh` (drop `PRINT_CONFIG` from the runtime-config key list)
- Modify: `frontend/src/types/config.ts`, `frontend/src/utils/runtimeConfig.ts`, `frontend/src/proxy.ts`, `frontend/src/components/tabs/Tabs.tsx`, `frontend/src/components/tabs/CustomCreateTab.tsx`, `frontend/src/components/tabs/QuickCreateTab.tsx`, `frontend/src/components/tabs/VouchersTab.tsx`, `frontend/src/components/VoucherCard.tsx`, `frontend/src/contexts/GlobalContext.tsx`, `frontend/src/components/Header.tsx`, `frontend/src/components/utils/WifiQr.tsx`, `frontend/package.json`
- Replace: `frontend/public/logo.svg` (already committed — the RUCKUS Networks lockup)

**Interfaces:**
- Consumes: `GuestPass`, `PassCreateData`, `api` (Task 7).
- Produces: a compiling frontend with no references to printing, rolling vouchers, deletion, or per-voucher limits.

- [ ] **Step 1: Delete the removed routes and utilities**

```bash
cd /Users/todd/src/greyrock-labs/unleashed-voucher-manager
git rm -r frontend/src/app/print frontend/src/app/welcome frontend/src/app/kiosk
git rm frontend/src/components/tabs/TestTab.tsx frontend/src/utils/print.ts frontend/src/utils/ipv4.ts frontend/src/types/print.ts
```

- [ ] **Step 2: Simplify the runtime config**

Replace all of `frontend/src/types/config.ts`:

```ts
export type RuntimeConfig = {
  IS_LOGO_INVERTIBLE: boolean;
  WIFI_SSID?: string;
  WIFI_PASSWORD?: string;
  WIFI_TYPE?: string;
  WIFI_HIDDEN?: string;
};

export const DEFAULT_RUNTIME_CONFIG: RuntimeConfig = {
  IS_LOGO_INVERTIBLE: false,
};
```

Then open `frontend/src/utils/runtimeConfig.ts` and delete the `PRINT_CONFIG` key and its JSON parsing, leaving the `WIFI_*` and `IS_LOGO_INVERTIBLE` reads untouched.

Also edit `scripts/entrypoint.sh`, which writes `runtime-config.json` at container start. Remove `'PRINT_CONFIG'` from its `keys` array, leaving:

```js
const keys = ['WIFI_SSID','WIFI_PASSWORD','WIFI_TYPE','WIFI_HIDDEN','IS_LOGO_INVERTIBLE'];
```

- [ ] **Step 3: Simplify the proxy**

Replace all of `frontend/src/proxy.ts`:

```ts
import { NextResponse, NextRequest } from "next/server";

export const config = {
  matcher: ["/rust-api/:path*"],
};

const DEFAULT_FRONTEND_TO_BACKEND_URL = "http://127.0.0.1";
const DEFAULT_BACKEND_BIND_PORT = "8080";

export function proxy(request: NextRequest) {
  const backend =
    process.env.FRONTEND_TO_BACKEND_URL ?? DEFAULT_FRONTEND_TO_BACKEND_URL;
  const port = process.env.BACKEND_BIND_PORT ?? DEFAULT_BACKEND_BIND_PORT;

  const url = new URL(request.nextUrl.pathname.replace(/^\/rust-api/, "/api"), `${backend}:${port}`);
  url.search = request.nextUrl.search;

  return NextResponse.rewrite(url);
}
```

Note: `GUEST_SUBNETWORK` gating is gone with the `/welcome` page, so the subnet check and its `ipv4` helper are deleted rather than carried forward.

- [ ] **Step 4: Update the components**

Work through each file and remove every reference the compiler flags:

- `Tabs.tsx` — drop the `TestTab` import and its tab entry.
- `CustomCreateTab.tsx` — keep name, duration and share-count inputs. Delete the data-usage, rx-rate and tx-rate inputs and their state. Duration is now **hours**, using `MIN_PASS_DURATION_HOURS`/`MAX_PASS_DURATION_HOURS`. Label the share input "Devices (0 = unlimited)" and default it to `0`.
- `QuickCreateTab.tsx` — presets become hour counts: 1, 4, 8, 24, 72, 168. Each calls `api.createPass({ name, durationHours, shareNumber: 0 })`.
- `VouchersTab.tsx` — remove bulk selection, the delete buttons, and the "delete expired" action. Keep list rendering and the name filter, now driven by `api.getAllPasses()`.
- `VoucherCard.tsx` — render `code`, `name`, `ssid`, `createdAt`, `expiresAt`, `used`, and `clientMacs.length`. Delete the data-limit and rate-limit rows. Where the card showed a guest limit, show `shareNumber === 0 ? "Unlimited" : shareNumber`.
- `GlobalContext.tsx` — remove rolling-voucher and print state; keep notifications and theme.

- [ ] **Step 5: Rebrand the header**

`frontend/public/logo.svg` is already the RUCKUS Networks lockup, committed
separately. It carries an internal `@media (prefers-color-scheme: dark)` rule
that flips the wordmark to white while leaving the brand orange alone, because
the logo is rendered through a Next `Image` element where `currentColor` is
unavailable.

Two consequences to handle in `frontend/src/components/Header.tsx`:

- The lockup is **219.69 x 108.38**, not square. The existing `width={35}
  height={35}` would squash it. Use `width={110} height={54}` to preserve the
  ratio.
- Leave `IS_LOGO_INVERTIBLE` wired up but set it to `false` by default.
  `dark:invert` would turn the brand orange blue; the SVG's own media query
  already handles dark mode.

```tsx
          <Image
            src="/logo.svg"
            width={110}
            height={54}
            loading="eager"
            alt="Unleashed Voucher Manager logo"
            className={"shrink-0" + (isLogoInvertible ? " dark:invert" : "")}
          />
          <h1 className="text-xl md:text-2xl font-semibold text-brand">
            <span className="block sm:hidden">UVM</span>
            <span className="hidden sm:block">Unleashed Voucher Manager</span>
          </h1>
```

Note: the SVG media query follows the OS preference, not the app's manual
theme switcher, so toggling the theme in-app will not re-colour the logo.
Accepted — it beats a black wordmark on a black background.

- [ ] **Step 6: Stop using the lockup as the QR centre image**

`frontend/src/components/utils/WifiQr.tsx` defaults `imageSrc` to
`/logo.svg`. That was fine for UVM's square 32x32 mark, but a 2:1 lockup in
the middle of a QR code both looks wrong and eats more of the code's error
budget than a square mark does, which can hurt scanning.

Change the default to `undefined` and skip rendering the centre image when it
is not set:

```tsx
  imageSrc,
```

Verify the QR still scans with a phone camera after the change.

- [ ] **Step 7: Rename the package**

In `frontend/package.json`, change the name field:

```json
  "name": "unleashed-voucher-manager",
```

- [ ] **Step 8: Verify the frontend compiles and builds**

Run: `cd frontend && npx tsc --noEmit && npm run build`
Expected: both succeed with no errors.

- [ ] **Step 9: Commit**

```bash
git add -A frontend
git commit -m "refactor: remove printing and rolling vouchers, rebrand to Ruckus"
```

---

### Task 9: Guest-facing display page

**Files:**
- Create: `frontend/src/app/display/page.tsx`
- Modify: `frontend/src/proxy.ts` (add `/display` to the matcher only if gating is later needed — not required now)

**Interfaces:**
- Consumes: `api.getDailyPass()` (Task 7), the existing `WifiQr` component at `frontend/src/components/utils/WifiQr.tsx`, `Spinner` (default export) at `frontend/src/components/utils/Spinner.tsx`, and `useServerEvents` at `frontend/src/hooks/useServerEvents.ts` — which takes **no arguments** and signals via a `vouchersUpdated` window CustomEvent.
- Produces: a read-only page at `/display`.

- [ ] **Step 1: Write the page**

Create `frontend/src/app/display/page.tsx`:

```tsx
"use client";

import { useCallback, useEffect, useState } from "react";
import { api } from "@/utils/api";
import { GuestPass } from "@/types/voucher";
import { useServerEvents } from "@/hooks/useServerEvents";
import WifiQr from "@/components/utils/WifiQr";
import Spinner from "@/components/utils/Spinner";

export default function DisplayPage() {
  const [pass, setPass] = useState<GuestPass | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      setPass(await api.getDailyPass());
    } catch {
      setPass(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    // The pass only changes once a day; poll hourly as a safety net so a
    // wall-mounted tablet recovers without anyone touching it.
    const id = setInterval(refresh, 60 * 60 * 1000);
    return () => clearInterval(id);
  }, [refresh]);

  // The hook takes no arguments -- it dispatches a `vouchersUpdated`
  // CustomEvent on window, which is what we subscribe to.
  useServerEvents();
  useEffect(() => {
    window.addEventListener("vouchersUpdated", refresh);
    return () => window.removeEventListener("vouchersUpdated", refresh);
  }, [refresh]);

  if (loading) {
    return (
      <main className="flex min-h-screen items-center justify-center">
        <Spinner />
      </main>
    );
  }

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-10 p-8">
      <h1 className="text-3xl font-light opacity-70">Guest WiFi</h1>

      {pass ? (
        <p className="font-mono text-7xl font-bold tracking-widest sm:text-8xl">
          {pass.code}
        </p>
      ) : (
        <p className="text-2xl opacity-60">No guest code available</p>
      )}

      <WifiQr />
    </main>
  );
}
```

- [ ] **Step 2: Verify it builds**

Run: `cd frontend && npm run build`
Expected: succeeds, and the build output lists `/display` among the routes.

- [ ] **Step 3: Check it renders**

Run: `cd frontend && npm run dev`, then open `http://localhost:3000/display`.
Expected: the page renders. Without a backend it shows "No guest code available" — that is the correct degraded state, not a failure.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/app/display
git commit -m "feat: add guest-facing display page"
```

---

### Task 10: Container build

**Files:**
- Modify: `Dockerfile`
- Create: `docker-bake.hcl`
- Modify: `compose.yaml`

**Interfaces:**
- Consumes: the built backend and frontend.
- Produces: an image buildable with `docker buildx bake image-local`.

- [ ] **Step 1: Write the bake file**

`scripts/healthcheck.sh` already probes `/api/health` and `scripts/run_wrapper.sh`
references no removed variables — both are correct as-is, leave them alone.
(`scripts/entrypoint.sh` was handled in Task 8.)


Create `docker-bake.hcl`, mirroring `cert-manager-webhook-cloudns`:

```hcl
variable "DEFAULT_TAG" {
  default = "unleashed-voucher-manager:local"
}

// Special target: https://github.com/docker/metadata-action#bake-definition
target "docker-metadata-action" {
  tags = ["${DEFAULT_TAG}"]
}

group "default" {
  targets = ["image-local"]
}

target "image" {
  inherits = ["docker-metadata-action"]
  // GHCR links a package to its repo from the manifest ANNOTATION, not the
  // config label -- the label alone leaves the package orphaned.
  annotations = [
    "index,manifest:org.opencontainers.image.source=https://github.com/greyrock-labs/unleashed-voucher-manager"
  ]
}

target "image-local" {
  inherits = ["image"]
  output = ["type=docker"]
}

target "image-all" {
  inherits = ["image"]
  // amd64 only, by choice. The Forgejo runner cannot mount binfmt_misc, so
  // QEMU emulation is unavailable there -- do not add setup-qemu-action.
  platforms = [
    "linux/amd64"
  ]
}
```

- [ ] **Step 2: Update compose.yaml**

Replace the `environment:` block with the Unleashed variables:

```yaml
    environment:
      UNLEASHED_URL: "https://unleashed.example.com"
      UNLEASHED_USERNAME: "guestpass"
      UNLEASHED_PASSWORD: "changeme"
      UNLEASHED_SSID: "Guest"
      UNLEASHED_HAS_VALID_CERT: "true"
      TIMEZONE: "UTC"
      DAILY_ROLL_HOUR: "4"
      DAILY_DURATION_HOURS: "24"
      DAILY_SHARE_NUMBER: "0"
      WIFI_SSID: "Guest"
      WIFI_PASSWORD: ""
```

- [ ] **Step 3: Build the image**

Run: `docker buildx bake image-local`
Expected: builds successfully and tags `unleashed-voucher-manager:local`.

- [ ] **Step 4: Commit**

```bash
git add Dockerfile docker-bake.hcl compose.yaml
git commit -m "chore: add bake definition and update container config"
```

---

### Task 11: Forgejo CI and release workflows

**Files:**
- Create: `.forgejo/workflows/ci.yaml`
- Create: `.forgejo/workflows/release.yaml`
- Create: `.forgejo/actionlint.yaml`

**Interfaces:**
- Consumes: `docker-bake.hcl` (Task 10), the Helm chart (Task 12).
- Produces: CI on push/PR, and a tag-triggered release publishing to GHCR.

- [ ] **Step 1: Copy the actionlint config**

```bash
cp /Users/todd/src/greyrock-labs/cert-manager-webhook-cloudns/.forgejo/actionlint.yaml \
   /Users/todd/src/greyrock-labs/unleashed-voucher-manager/.forgejo/actionlint.yaml
```

- [ ] **Step 2: Write the CI workflow**

Create `.forgejo/workflows/ci.yaml`:

```yaml
---
name: CI

on:
  pull_request:
  push:
    branches:
      - main
  workflow_dispatch:

jobs:
  backend:
    name: Build and test backend
    runs-on: docker
    steps:
      - name: Checkout
        uses: https://github.com/actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1

      - name: Build
        run: cd backend && cargo build --locked

      - name: Run tests
        run: cd backend && cargo test --locked

  frontend:
    name: Build frontend
    runs-on: docker
    steps:
      - name: Checkout
        uses: https://github.com/actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1

      - name: Install dependencies
        run: cd frontend && npm ci

      - name: Typecheck
        run: cd frontend && npx tsc --noEmit

      - name: Build
        run: cd frontend && npm run build
```

- [ ] **Step 3: Write the release workflow**

Create `.forgejo/workflows/release.yaml`:

```yaml
---
name: Release

on:
  push:
    tags:
      - "v*"
  workflow_dispatch:

jobs:
  release:
    name: Build and publish
    runs-on: docker
    steps:
      - name: Checkout
        uses: https://github.com/actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1

      - name: Derive version from tag
        id: version
        run: echo "version=${GITHUB_REF_NAME#v}" >> "$GITHUB_OUTPUT"

      - name: Run backend tests
        run: cd backend && cargo test --locked

      - name: Log in to GHCR
        uses: https://github.com/docker/login-action@dbcb813823bdd20940b903addbd779551569679f # v4.6.0
        with:
          registry: ghcr.io
          username: ${{ secrets.GHCR_USERNAME }}
          password: ${{ secrets.GHCR_TOKEN }}

      - name: Set up Docker Buildx
        uses: https://github.com/docker/setup-buildx-action@37fe631027851001ddb9b187196cc803df7f5f0e # v4.3.0

      - name: Generate image tags and labels
        id: meta
        uses: https://github.com/docker/metadata-action@dc802804100637a589fabce1cb79ff13a1411302 # v6.2.0
        with:
          images: ghcr.io/greyrock-labs/unleashed-voucher-manager
          # Must be the GitHub URL, not the Forgejo one -- this is what links
          # the package to the repo.
          labels: |
            org.opencontainers.image.source=https://github.com/greyrock-labs/unleashed-voucher-manager
          tags: |
            type=semver,pattern={{version}}
            type=semver,pattern={{major}}.{{minor}}
            type=semver,pattern={{major}}
            type=sha,prefix=sha-

      - name: Build and push image
        uses: https://github.com/docker/bake-action@d3418bd7d0e9324001bca92fa8ba175ea7e6dc9b # v7.3.0
        with:
          files: |
            ./docker-bake.hcl
            cwd://${{ steps.meta.outputs.bake-file }}
          targets: image-all
          push: true

      - name: Set up Helm
        uses: https://github.com/azure/setup-helm@9bc31f4ebc9c6b171d7bfbaa5d006ae7abdb4310 # v5.0.1

      - name: Package and push chart
        env:
          VERSION: ${{ steps.version.outputs.version }}
        run: |
          helm package deploy/unleashed-voucher-manager \
            --version "${VERSION}" \
            --app-version "${VERSION}" \
            --destination dist
          helm push "dist/unleashed-voucher-manager-${VERSION}.tgz" \
            oci://ghcr.io/greyrock-labs/helm
```

- [ ] **Step 4: Commit**

```bash
git add .forgejo
git commit -m "ci: add Forgejo build and release workflows"
```

---

### Task 12: Helm chart

**Files:**
- Create: `deploy/unleashed-voucher-manager/Chart.yaml`
- Create: `deploy/unleashed-voucher-manager/values.yaml`
- Create: `deploy/unleashed-voucher-manager/templates/deployment.yaml`
- Create: `deploy/unleashed-voucher-manager/templates/service.yaml`
- Create: `deploy/unleashed-voucher-manager/templates/_helpers.tpl`

**Interfaces:**
- Consumes: the image published by Task 11.
- Produces: a chart that renders with `helm template`.

- [ ] **Step 1: Write Chart.yaml**

```yaml
apiVersion: v2
name: unleashed-voucher-manager
description: Guest WiFi pass manager for Ruckus Unleashed
# sources[0] is what links the published chart package to its GitHub repo on
# GHCR -- the chart-side counterpart of the image's manifest annotation. It
# must be the GitHub URL, not the Forgejo one. Without it the chart package
# is orphaned.
sources:
  - https://github.com/greyrock-labs/unleashed-voucher-manager
type: application
version: 0.0.0
appVersion: "0.0.0"
```

- [ ] **Step 2: Write values.yaml**

```yaml
image:
  repository: ghcr.io/greyrock-labs/unleashed-voucher-manager
  tag: ""
  pullPolicy: IfNotPresent

replicaCount: 1

service:
  type: ClusterIP
  port: 3000

# Non-secret configuration, rendered into the Deployment as env vars.
config:
  UNLEASHED_URL: ""
  UNLEASHED_SSID: ""
  UNLEASHED_HAS_VALID_CERT: "true"
  TIMEZONE: "UTC"
  DAILY_ROLL_HOUR: "4"
  DAILY_DURATION_HOURS: "24"
  # 0 means unlimited devices may share the daily code.
  DAILY_SHARE_NUMBER: "0"
  WIFI_SSID: ""

# Name of an existing Secret supplying UNLEASHED_USERNAME and
# UNLEASHED_PASSWORD. How that Secret is produced is out of scope for this
# chart.
existingSecret: ""

resources: {}
nodeSelector: {}
tolerations: []
affinity: {}
```

- [ ] **Step 3: Write _helpers.tpl**

```yaml
{{- define "unleashed-voucher-manager.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "unleashed-voucher-manager.fullname" -}}
{{- printf "%s-%s" .Release.Name (include "unleashed-voucher-manager.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "unleashed-voucher-manager.labels" -}}
app.kubernetes.io/name: {{ include "unleashed-voucher-manager.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}
```

- [ ] **Step 4: Write deployment.yaml**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "unleashed-voucher-manager.fullname" . }}
  labels:
    {{- include "unleashed-voucher-manager.labels" . | nindent 4 }}
spec:
  replicas: {{ .Values.replicaCount }}
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "unleashed-voucher-manager.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
  template:
    metadata:
      labels:
        app.kubernetes.io/name: {{ include "unleashed-voucher-manager.name" . }}
        app.kubernetes.io/instance: {{ .Release.Name }}
    spec:
      containers:
        - name: {{ .Chart.Name }}
          image: "{{ .Values.image.repository }}:{{ .Values.image.tag | default .Chart.AppVersion }}"
          imagePullPolicy: {{ .Values.image.pullPolicy }}
          ports:
            - name: http
              containerPort: 3000
          env:
            {{- range $key, $value := .Values.config }}
            - name: {{ $key }}
              value: {{ $value | quote }}
            {{- end }}
          {{- if .Values.existingSecret }}
          envFrom:
            - secretRef:
                name: {{ .Values.existingSecret }}
          {{- end }}
          livenessProbe:
            httpGet:
              path: /
              port: http
          readinessProbe:
            httpGet:
              path: /
              port: http
          resources:
            {{- toYaml .Values.resources | nindent 12 }}
      {{- with .Values.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
```

- [ ] **Step 5: Write service.yaml**

```yaml
apiVersion: v1
kind: Service
metadata:
  name: {{ include "unleashed-voucher-manager.fullname" . }}
  labels:
    {{- include "unleashed-voucher-manager.labels" . | nindent 4 }}
spec:
  type: {{ .Values.service.type }}
  ports:
    - port: {{ .Values.service.port }}
      targetPort: http
      protocol: TCP
      name: http
  selector:
    app.kubernetes.io/name: {{ include "unleashed-voucher-manager.name" . }}
    app.kubernetes.io/instance: {{ .Release.Name }}
```

- [ ] **Step 6: Verify the chart renders**

Run: `helm template test deploy/unleashed-voucher-manager`
Expected: renders a Deployment and a Service with no errors.

Run: `helm lint deploy/unleashed-voucher-manager`
Expected: no failures.

- [ ] **Step 7: Commit**

```bash
git add deploy
git commit -m "feat: add Helm chart"
```

---

### Task 13: Documentation and repository metadata

**Files:**
- Modify: `README.md` (full rewrite)
- Modify: `backend/Cargo.toml` (package name)
- Create: `NOTICE`

**Interfaces:**
- Consumes: everything above.
- Produces: a README describing this project rather than UVM.

- [ ] **Step 1: Rename the backend package**

In `backend/Cargo.toml`:

```toml
[package]
name = "backend"
```

Leave the crate name as `backend` — the test files import `backend::`, and renaming it would ripple through every `use` for no benefit.

- [ ] **Step 2: Write the NOTICE file**

```
This project is derived from unifi-voucher-manager
(https://github.com/etiennecollin/unifi-voucher-manager), Copyright (c)
Etienne Collin, used under the MIT License. See LICENSE.

The Ruckus Unleashed protocol implementation was informed by:
  - FetchPass (https://github.com/fmuffat/FetchPass), MIT
  - aioruckus (https://github.com/ms264556/aioruckus), 0BSD

This is an independent, unofficial project, not affiliated with,
endorsed by, or sponsored by CommScope, RUCKUS Networks, or Ubiquiti Inc.
```

- [ ] **Step 3: Rewrite the README**

The README must cover: what it does, the daily-code behaviour, the full environment variable table from the spec's Configuration section, a Docker Compose quick start, and — prominently — the three constraints that will otherwise generate bug reports:

1. The Guest Pass Manager account cannot delete; passes accumulate and are pruned in the Unleashed admin UI.
2. An unused code stays claimable for 7 days, so a new daily code does not invalidate yesterday's.
3. `DAILY_SHARE_NUMBER=0` means unlimited devices; setting it to `1` admits exactly one guest.

- [ ] **Step 4: Verify nothing still references UniFi**

Run:

```bash
cd /Users/todd/src/greyrock-labs/unleashed-voucher-manager
grep -rniE "unifi|voucher" --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.yaml' --include='*.toml' . | grep -v node_modules | grep -v target
```

Expected: matches only in `NOTICE`, `LICENSE`, `README.md` (attribution), and the repository/chart name itself. Any match in `backend/src` or `frontend/src` is a leftover to fix.

- [ ] **Step 5: Run the full suite one last time**

```bash
cd backend && cargo test --locked && cargo build --release
cd ../frontend && npx tsc --noEmit && npm run build
cd .. && helm lint deploy/unleashed-voucher-manager && docker buildx bake image-local
```

Expected: all succeed.

- [ ] **Step 6: Commit**

```bash
git add README.md NOTICE backend/Cargo.toml
git commit -m "docs: document the Unleashed voucher manager"
```

---

## Deferred

Recorded so they are not silently lost:

- **Auto-purge finding (2026-09-23).** Whether the controller removes expired passes by itself. If it does not, the pass list grows without bound and a README note about periodic manual pruning should become a stronger warning.
- **Admin credential support.** Would unlock deletion — bulk delete, expired cleanup, and genuinely invalidating yesterday's code. Adds a second endpoint surface (`/admin/login.jsp`, `/admin/_cmdstat.jsp`) and a second auth path. Deliberately out of scope for v1.
