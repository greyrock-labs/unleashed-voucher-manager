//! Building `mon_createguest.jsp` requests and reading their responses.

/// Most passes the controller creates in one batch.
pub const MAX_BATCH: u32 = 100;

const MINUTES_PER_HOUR: u64 = 60;
const HOURS_PER_DAY: u64 = 24;
const HOURS_PER_WEEK: u64 = 24 * 7;

/// What to create. `count` 1 creates a single named pass; 2 or more create
/// a batch, which the controller names itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateParams {
    pub count: u32,
    pub name: String,
    pub minutes: u64,
    /// Devices per pass; `None` is unlimited.
    pub share_limit: Option<u64>,
    /// A custom pass key; `None` lets the controller generate one.
    pub key: Option<String>,
    pub remarks: Option<String>,
    /// The guest WLAN the pass is for.
    pub ssid: String,
}

/// The controller's verdict on a create request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    /// `DONE` (single) or `OK` (batch). Not proof a pass exists: a name with
    /// whitespace also answers `OK` and creates nothing.
    Accepted,
    KeyDuplicated(String),
    Failed(String),
}

/// The controller only accepts whole hours, days or weeks. Round the minutes
/// up to whole hours, then pick the largest unit that divides them exactly.
pub fn duration_for_minutes(minutes: u64) -> Result<(u64, &'static str), String> {
    if minutes == 0 {
        return Err("duration must be at least one minute".to_string());
    }
    let hours = minutes.div_ceil(MINUTES_PER_HOUR);
    Ok(if hours.is_multiple_of(HOURS_PER_WEEK) {
        (hours / HOURS_PER_WEEK, "week")
    } else if hours.is_multiple_of(HOURS_PER_DAY) {
        (hours / HOURS_PER_DAY, "day")
    } else {
        (hours, "hour")
    })
}

/// The controller silently creates nothing for a name containing whitespace,
/// so replace every whitespace run with a single `-`.
pub fn sanitize_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("-")
}

/// Keys are 2 to 16 characters without whitespace, `# & + " ' < >` or comma.
/// The controller stores them uppercased.
pub fn validate_key(key: &str) -> Result<String, String> {
    let length = key.chars().count();
    if !(2..=16).contains(&length) {
        return Err("key must be 2 to 16 characters".to_string());
    }
    if key
        .chars()
        .any(|c| c.is_whitespace() || "#&+\"'<>,".contains(c))
    {
        return Err("key cannot contain whitespace, # & + \" ' < > or a comma".to_string());
    }
    Ok(key.to_uppercase())
}

/// The form fields for `mon_createguest.jsp`.
pub fn build_form(params: &CreateParams) -> Result<Vec<(&'static str, String)>, String> {
    if params.count == 0 || params.count > MAX_BATCH {
        return Err(format!("count must be 1 to {MAX_BATCH}"));
    }
    let batch = params.count > 1;
    if batch && params.key.is_some() {
        return Err("a custom key needs a count of 1".to_string());
    }
    let (duration, unit) = duration_for_minutes(params.minutes)?;
    let key = match &params.key {
        Some(key) => validate_key(key)?,
        None => String::new(),
    };
    let name = sanitize_name(&params.name);
    if !batch && name.is_empty() {
        return Err("name cannot be empty".to_string());
    }

    Ok(vec![
        (
            "gentype",
            if batch { "multiple" } else { "single" }.to_string(),
        ),
        ("fullname", if batch { String::new() } else { name }),
        ("remarks", params.remarks.clone().unwrap_or_default()),
        ("duration", duration.to_string()),
        ("duration-unit", unit.to_string()),
        ("key", key),
        (
            "createToNum",
            if batch {
                params.count.to_string()
            } else {
                String::new()
            },
        ),
        ("batchpass", String::new()),
        ("guest-wlan", params.ssid.clone()),
        ("shared", "true".to_string()),
        ("reauth", "false".to_string()),
        ("reauth-time", String::new()),
        ("reauth-unit", "min".to_string()),
        ("email", String::new()),
        ("countrycode", String::new()),
        ("phonenumber", String::new()),
        ("limitnumber", params.share_limit.unwrap_or(0).to_string()),
    ])
}

/// Read a `mon_createguest.jsp` response. Batch responses put
/// `batchEmailData.push(...)` script lines before the JSON object.
pub fn parse_create_response(body: &str) -> CreateOutcome {
    let json = match (body.find('{'), body.rfind('}')) {
        (Some(start), Some(end)) if start < end => &body[start..=end],
        _ => return CreateOutcome::Failed(format!("unexpected response: {}", body.trim())),
    };
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(e) => return CreateOutcome::Failed(format!("unreadable response: {e}")),
    };
    let field = |name: &str| value.get(name).and_then(|v| v.as_str()).unwrap_or("");
    match field("result") {
        "DONE" | "OK" => CreateOutcome::Accepted,
        "KEY_DUPLICATED" => CreateOutcome::KeyDuplicated(field("errorMsg").to_string()),
        other => {
            let message = field("errorMsg");
            CreateOutcome::Failed(if message.is_empty() || message == "~~" {
                format!("controller answered {other:?}")
            } else {
                message.to_string()
            })
        }
    }
}
