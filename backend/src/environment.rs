use std::{env, sync::OnceLock};

use chrono_tz::Tz;
use tracing::{error, info};

const DEFAULT_BACKEND_BIND_HOST: &str = "127.0.0.1";
const DEFAULT_BACKEND_BIND_PORT: u16 = 8080;
const DEFAULT_ROLLING_VOUCHER_DURATION_MINUTES: u64 = 480;

pub static ENVIRONMENT: OnceLock<Environment> = OnceLock::new();

#[derive(Clone)]
pub struct Environment {
    pub unleashed_url: String,
    pub unleashed_username: String,
    pub unleashed_password: String,
    pub unleashed_ssid: String,
    pub unleashed_has_valid_cert: bool,
    pub backend_bind_host: String,
    pub backend_bind_port: u16,
    pub purge_all_expired_vouchers: bool,
    pub rolling_voucher_duration_minutes: u64,
    pub timezone: Tz,
}

impl std::fmt::Debug for Environment {
    /// Hand-written so the controller password never reaches a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("unleashed_url", &self.unleashed_url)
            .field("unleashed_username", &self.unleashed_username)
            .field("unleashed_password", &"<redacted>")
            .field("unleashed_ssid", &self.unleashed_ssid)
            .field("unleashed_has_valid_cert", &self.unleashed_has_valid_cert)
            .field("backend_bind_host", &self.backend_bind_host)
            .field("backend_bind_port", &self.backend_bind_port)
            .field(
                "purge_all_expired_vouchers",
                &self.purge_all_expired_vouchers,
            )
            .field(
                "rolling_voucher_duration_minutes",
                &self.rolling_voucher_duration_minutes,
            )
            .field("timezone", &self.timezone)
            .finish()
    }
}

impl Environment {
    pub fn try_new() -> Result<Self, String> {
        #[cfg(feature = "dotenv")]
        dotenvy::dotenv().map_err(|e| format!("Failed to load .env file: {e}"))?;

        let required = |name: &str| -> Result<String, String> {
            match env::var(name) {
                Ok(value) if !value.trim().is_empty() => Ok(value),
                Ok(_) => Err(format!("{name} is empty")),
                Err(e) => Err(format!("{name}: {e}")),
            }
        };

        let unleashed_url = required("UNLEASHED_URL")?.trim_end_matches('/').to_string();
        if !unleashed_url.starts_with("http://") && !unleashed_url.starts_with("https://") {
            return Err("UNLEASHED_URL must start with http:// or https://".to_string());
        }
        let unleashed_username = required("UNLEASHED_USERNAME")?;
        let unleashed_password = required("UNLEASHED_PASSWORD")?;
        let unleashed_ssid = required("UNLEASHED_SSID")?;

        let backend_bind_host: String =
            env::var("BACKEND_BIND_HOST").unwrap_or(DEFAULT_BACKEND_BIND_HOST.to_owned());
        let backend_bind_port: u16 = match env::var("BACKEND_BIND_PORT") {
            Ok(port_str) => port_str
                .parse()
                .map_err(|e| format!("Invalid BACKEND_BIND_PORT: {e}"))?,
            Err(_) => DEFAULT_BACKEND_BIND_PORT,
        };

        let rolling_voucher_duration_minutes = match env::var("ROLLING_VOUCHER_DURATION_MINUTES") {
            Ok(val) => val
                .parse()
                .map_err(|e| format!("Invalid ROLLING_VOUCHER_DURATION_MINUTES: {e}"))?,
            Err(_) => DEFAULT_ROLLING_VOUCHER_DURATION_MINUTES,
        };
        if rolling_voucher_duration_minutes == 0 {
            return Err("ROLLING_VOUCHER_DURATION_MINUTES must be at least 1".to_string());
        }

        let purge_all_expired_vouchers: bool = match env::var("PURGE_ALL_EXPIRED_VOUCHERS") {
            Ok(val) => Self::parse_bool(&val)
                .map_err(|e| format!("Invalid PURGE_ALL_EXPIRED_VOUCHERS: {e}"))?,
            Err(_) => false,
        };

        let unleashed_has_valid_cert: bool = match env::var("UNLEASHED_HAS_VALID_CERT") {
            Ok(val) => Self::parse_bool(&val)
                .map_err(|e| format!("Invalid UNLEASHED_HAS_VALID_CERT: {e}"))?,
            Err(_) => true,
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
                info!("TIMEZONE environment variable not set, defaulting to UTC");
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
            rolling_voucher_duration_minutes,
            purge_all_expired_vouchers,
            timezone,
        })
    }

    fn parse_bool(s: &str) -> Result<bool, String> {
        match s.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => Err(format!("Boolean value must be true or false, found: {s}")),
        }
    }
}
