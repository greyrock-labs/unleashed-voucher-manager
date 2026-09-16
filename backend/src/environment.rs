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
