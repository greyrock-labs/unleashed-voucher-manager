//! Turning guest passes into the `Voucher` shape the frontend uses, and the
//! rolling-voucher rules.

use chrono::{DateTime, TimeZone};
use chrono_tz::Tz;

use super::guest::GuestPass;
use crate::models::Voucher;

/// UVM used `[ROLLING] `, but the controller cannot take whitespace in names.
pub const ROLLING_PREFIX: &str = "[ROLLING]-";

const DATE_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

pub fn format_time(unix: i64, timezone: Tz) -> String {
    match timezone.timestamp_opt(unix, 0).single() {
        Some(time) => time.format(DATE_TIME_FORMAT).to_string(),
        None => unix.to_string(),
    }
}

/// With `countdown-by-issued` the controller sets `start-time` at creation,
/// so only `used` and connected clients show that a pass has been used.
pub fn is_used(pass: &GuestPass) -> bool {
    pass.used || !pass.client_macs.is_empty()
}

pub fn is_expired(pass: &GuestPass, now: i64) -> bool {
    pass.expire_time <= now
}

pub fn to_voucher(pass: &GuestPass, timezone: Tz, now: i64) -> Voucher {
    let used = is_used(pass);
    Voucher {
        id: pass.id.clone(),
        created_at: format_time(pass.create_time, timezone),
        name: pass.name.clone(),
        code: pass.key.clone(),
        authorized_guest_limit: match pass.share_number {
            0 => None,
            n => Some(n),
        },
        authorized_guest_count: pass.client_macs.len() as u64,
        activated_at: used
            .then(|| format_time(pass.start_time.unwrap_or(pass.create_time), timezone)),
        expires_at: Some(format_time(pass.expire_time, timezone)),
        expired: is_expired(pass, now),
        time_limit_minutes: pass.valid_time / 60,
        remarks: pass.remarks.clone(),
    }
}

pub fn is_rolling(pass: &GuestPass) -> bool {
    pass.name.starts_with(ROLLING_PREFIX)
}

pub fn rolling_name(created: DateTime<Tz>, ip: &str) -> String {
    format!("{ROLLING_PREFIX}{}-{ip}", created.format("%Y%m%d%H%M%S"))
}

/// The newest rolling pass that is still unused and unexpired.
pub fn current_rolling(passes: &[GuestPass], now: i64) -> Option<&GuestPass> {
    passes
        .iter()
        .filter(|p| is_rolling(p) && !is_used(p) && !is_expired(p, now))
        .max_by_key(|p| (p.create_time, p.id.parse::<u64>().unwrap_or(0)))
}

/// Whether `ip` already minted a rolling pass that has not expired.
pub fn has_live_rolling_for_ip(passes: &[GuestPass], ip: &str, now: i64) -> bool {
    let suffix = format!("-{ip}");
    passes
        .iter()
        .any(|p| is_rolling(p) && !is_expired(p, now) && p.name.ends_with(&suffix))
}
