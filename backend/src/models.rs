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
    /// "ok" when the controller answered, "degraded" when it did not.
    /// Deliberately still served with HTTP 200 -- see the handler.
    pub status: String,
    /// False when today's pass is missing and only a stale one was found.
    #[serde(rename = "dailyPassCurrent")]
    pub daily_pass_current: bool,
    /// False when the Unleashed controller could not be reached at all.
    #[serde(rename = "controllerReachable")]
    pub controller_reachable: bool,
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
