# Unleashed Voucher Manager Implementation Plan

> **Status: implemented in v1.0.0. Do not execute this plan again.** It records how the port was built. Later releases changed some details (rolling voucher rules, character validation, logging, startup retries), so the code and the spec are the current reference.

**Goal:** Port the imported unifi-voucher-manager (UVM) code to Ruckus Unleashed guest passes, keeping UVM's routes, JSON shapes and features.

**Architecture:** The Axum backend's UniFi client (`backend/src/unifi_api.rs`) is replaced by an `unleashed` module (guest-list parsing, create-request building, an authenticated session, and pass-to-voucher mapping) behind an `UnleashedAPI` with the same method set the handlers already call. The Next.js frontend changes only where Unleashed forces it: no data or rate limits, durations in hours, days or weeks, custom keys and remarks, and a kiosk that re-checks every minute.

**Tech Stack:** Rust 2024 (axum 0.8, reqwest 0.13 with cookies and form, roxmltree 0.21, chrono-tz, tokio), Next.js 16 with TypeScript, Forgejo Actions, Docker Bake.

**Spec:** `docs/superpowers/specs/2026-10-08-unleashed-voucher-manager-design.md`

## Global Constraints

- Every controller call goes through `/admin/` pages: `GET login.jsp`, then `GET dashboard.jsp` (required before AJAX works), `POST _conf.jsp` (`text/xml`), `POST mon_createguest.jsp` (urlencoded form).
- The CSRF token comes from the `HTTP_X_CSRF_TOKEN` response header on login, falling back to `_csrfTokenVar.jsp`, and is sent as `X-CSRF-Token`.
- A `302` from any AJAX call means the session lapsed: re-login once and retry once; a second `302` is an authentication error.
- Durations: minutes are rounded up to whole hours, then sent as `week`, `day` or `hour` (largest exact unit). Minutes are never sent.
- Batches: `count` 1 is `gentype=single`; 2 to 100 is `gentype=multiple`; anything else is `400`. A custom key needs `count` 1.
- Names: whitespace is replaced with `-` before sending. Duplicate names are allowed.
- Keys: 2 to 16 characters, no whitespace or `# & + " ' < >` or comma, uppercased.
- A create is only successful if a new pass appears in the guest list afterwards; otherwise `502`.
- `KEY_DUPLICATED` is `409`; other controller failures are `502`; invalid requests are `400`.
- Delete ids must be all ASCII digits, otherwise `400`.
- Rolling voucher names: `[ROLLING]-<YYYYmmddHHMMSS>-<ip>`. A pass is used when `used="true"` or it has `<client>` children; `start-time` alone does not mean used.
- The client address is the first entry of `X-Forwarded-For`, trimmed.
- Environment variables: `UNLEASHED_URL`, `UNLEASHED_USERNAME`, `UNLEASHED_PASSWORD`, `UNLEASHED_SSID` (required), `UNLEASHED_HAS_VALID_CERT` (default `true`); all other UVM variables keep their names and defaults.
- The first release is `v1.0.0`. Image: `ghcr.io/greyrock-labs/unleashed-voucher-manager`, linux/amd64 only.
- Docs, comments and commit messages describe this repository only: no internal hostnames, addresses or SSIDs, and no references to earlier attempts.
- Shell note for macOS: `sed -i` differs between GNU and BSD; the steps below write whole files or use the editor instead.

## Review Focus

- **UVM's default names contain spaces** ("Quick Voucher", "Custom Voucher"). Expected: the voucher is created as `Quick-Voucher`, not a silent failure. Pinned by `replaces_whitespace_in_names` (Task 2) and `creates_a_single_pass_with_a_spaced_name` (Task 5).
- **The controller answers OK but creates nothing.** Expected: the UI reports a failure (`502`), never a phantom success. Pinned by `an_ok_that_creates_nothing_is_an_error` (Task 5).
- **The controller reboots or the session lapses mid-use**, including several browser requests arriving at once. Expected: one transparent re-login and every request succeeds. Pinned by `logs_in_again_when_the_session_expires`, `concurrent_requests_share_one_relogin` (Task 3) and `survives_a_controller_reboot` (Task 5).
- **A proxy appends its own address to `X-Forwarded-For`** (`"192.0.2.7, 10.0.0.1"`). Expected: the guest's address is `192.0.2.7` for subnet gating and the one-voucher-per-IP rule. Pinned by `takes_the_first_forwarded_address` (Task 5) and the proxy change (Task 7).
- **A crafted delete request** (`ids=1'></guest><guest id='2`). Expected: `400` and nothing deleted. Pinned by `rejects_non_numeric_ids` (Task 5).

---

### Task 1: Guest-list parser

**Files:**
- Create: `backend/src/unleashed/mod.rs`
- Create: `backend/src/unleashed/guest.rs`
- Create: `backend/tests/fixtures/guest_list.xml`
- Create: `backend/tests/guest.rs`
- Modify: `backend/Cargo.toml`, `backend/Cargo.lock` (add `roxmltree`)
- Modify: `backend/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `backend::unleashed::guest::GuestPass` (fields `id: String, name: String, key: String, remarks: String, share_number: u64, valid_time: u64, create_time: i64, start_time: Option<i64>, expire_time: i64, used: bool, client_macs: Vec<String>`) and `parse_guest_list(xml: &str) -> Result<Vec<GuestPass>, String>`.

- [ ] **Step 1: Add the XML dependency**

Run: `cd backend && cargo add roxmltree@0.21.1`
Expected: `Adding roxmltree v0.21.1 to dependencies`.

- [ ] **Step 2: Write the fixture and the failing tests**

`backend/tests/fixtures/guest_list.xml` is a real controller response with the key and MAC sanitised:

````xml
<?xml version="1.0" encoding="utf-8"?><!DOCTYPE ajax-response><ajax-response><response type="object" id="guest-list.1791478508610"><resultset><guest shared-guestpass="true" share-number="1" created-by="admin" role-id="2147483647" countdown-by-issued="true" create-time="1791475860" valid-time="7344000" start-time="1791475860" expire-time="1798819860" email="" phone-number="" reauth-interval-unit="min" remarks="" name="guest-1" x-key="000000" id="1" full-name="guest-1" wlan="Guest" used="true" key="000000"><client mac="00:00:00:00:00:01" /></guest><guest shared-guestpass="true" share-number="0" created-by="unleashed-voucher-manager" role-id="2147483647" countdown-by-issued="true" create-time="1791478540" valid-time="14400" start-time="1791478540" expire-time="1791492940" email="" phone-number="" reauth-interval-unit="min" remarks="Batch generation &amp; more" name="Guest-4" x-key="602831" id="4" full-name="Guest-4" wlan="Guest" key="602831" /></resultset></response></ajax-response>
````

`backend/tests/guest.rs`:

````rust
use backend::unleashed::guest::{GuestPass, parse_guest_list};

const FIXTURE: &str = include_str!("fixtures/guest_list.xml");

#[test]
fn parses_every_pass() {
    let passes = parse_guest_list(FIXTURE).unwrap();
    assert_eq!(passes.len(), 2);
}

#[test]
fn parses_a_used_pass_with_a_client() {
    let pass = &parse_guest_list(FIXTURE).unwrap()[0];
    assert_eq!(
        pass,
        &GuestPass {
            id: "1".into(),
            name: "guest-1".into(),
            key: "000000".into(),
            remarks: "".into(),
            share_number: 1,
            valid_time: 7_344_000,
            create_time: 1_791_475_860,
            start_time: Some(1_791_475_860),
            expire_time: 1_798_819_860,
            used: true,
            client_macs: vec!["00:00:00:00:00:01".into()],
        }
    );
}

#[test]
fn parses_an_unused_pass_and_decodes_entities() {
    let pass = &parse_guest_list(FIXTURE).unwrap()[1];
    assert!(!pass.used);
    assert!(pass.client_macs.is_empty());
    assert_eq!(pass.share_number, 0);
    assert_eq!(pass.remarks, "Batch generation & more");
}

#[test]
fn empty_list_is_empty() {
    let xml = "<ajax-response><response><resultset /></response></ajax-response>";
    assert!(parse_guest_list(xml).unwrap().is_empty());
}

#[test]
fn empty_start_time_is_none() {
    let xml = r#"<r><guest id="7" start-time="" create-time="1" expire-time="2" /></r>"#;
    assert_eq!(parse_guest_list(xml).unwrap()[0].start_time, None);
}

#[test]
fn rejects_html() {
    assert!(parse_guest_list("<!DOCTYPE html><html><body>Moved").is_err());
}

#[test]
fn rejects_a_non_numeric_time() {
    let xml = r#"<r><guest id="7" create-time="soon" expire-time="2" /></r>"#;
    assert!(parse_guest_list(xml).is_err());
}
````

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd backend && cargo test --test guest`
Expected: compile error `unresolved import backend::unleashed`.

- [ ] **Step 4: Implement the parser**

`backend/src/unleashed/mod.rs`, with only the first module for now:

````rust
//! Client for the Ruckus Unleashed admin AJAX interface.

pub mod guest;
````

`backend/src/unleashed/guest.rs`:

````rust
//! Guest passes as the controller lists them.

/// One guest pass from a `guest-list` getconf response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestPass {
    pub id: String,
    pub name: String,
    pub key: String,
    pub remarks: String,
    /// `share-number`: how many devices may use the pass; 0 is unlimited.
    pub share_number: u64,
    /// Seconds of access the pass grants.
    pub valid_time: u64,
    /// Unix seconds.
    pub create_time: i64,
    /// Unix seconds; `None` when the controller left it empty.
    pub start_time: Option<i64>,
    /// Unix seconds.
    pub expire_time: i64,
    pub used: bool,
    pub client_macs: Vec<String>,
}

/// Parse a `guest-list` getconf response.
///
/// The controller duplicates `name`/`full-name` and `key`/`x-key`; the
/// `full-name` and `x-key` forms are preferred when present.
pub fn parse_guest_list(xml: &str) -> Result<Vec<GuestPass>, String> {
    let doc = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("invalid guest-list XML: {e}"))?;

    doc.descendants()
        .filter(|n| n.has_tag_name("guest"))
        .map(|n| {
            let attr = |name: &str| n.attribute(name).unwrap_or("");
            let id = attr("id");
            if id.is_empty() {
                return Err("guest without an id".to_string());
            }
            let number = |name: &str| -> Result<i64, String> {
                let raw = attr(name);
                if raw.is_empty() {
                    return Ok(0);
                }
                raw.parse()
                    .map_err(|_| format!("guest {id}: {name} is not a number: {raw:?}"))
            };
            let start_time = match attr("start-time") {
                "" => None,
                _ => Some(number("start-time")?),
            };
            Ok(GuestPass {
                id: id.to_string(),
                name: n
                    .attribute("full-name")
                    .or(n.attribute("name"))
                    .unwrap_or("")
                    .to_string(),
                key: n
                    .attribute("x-key")
                    .or(n.attribute("key"))
                    .unwrap_or("")
                    .to_string(),
                remarks: attr("remarks").to_string(),
                share_number: number("share-number")?.max(0) as u64,
                valid_time: number("valid-time")?.max(0) as u64,
                create_time: number("create-time")?,
                start_time,
                expire_time: number("expire-time")?,
                used: attr("used") == "true",
                client_macs: n
                    .children()
                    .filter(|c| c.has_tag_name("client"))
                    .filter_map(|c| c.attribute("mac").map(str::to_string))
                    .collect(),
            })
        })
        .collect()
}
````

In `backend/src/lib.rs`, add `pub mod unleashed;` after `pub mod unifi_api;`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd backend && cargo test --test guest`
Expected: `7 passed`.

- [ ] **Step 6: Commit**

```bash
git add backend/Cargo.toml backend/Cargo.lock backend/src/lib.rs backend/src/unleashed backend/tests/guest.rs backend/tests/fixtures/guest_list.xml
git commit -m "feat(backend): parse the Unleashed guest list"
```

---

### Task 2: Create requests and responses

**Files:**
- Create: `backend/src/unleashed/create.rs`
- Create: `backend/tests/fixtures/create_single.txt`, `create_batch.txt`, `create_keydup.txt`
- Create: `backend/tests/create.rs`
- Modify: `backend/src/unleashed/mod.rs`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: in `backend::unleashed::create`: `MAX_BATCH: u32 = 100`; `struct CreateParams { count: u32, name: String, minutes: u64, share_limit: Option<u64>, key: Option<String>, remarks: Option<String>, ssid: String }`; `enum CreateOutcome { Accepted, KeyDuplicated(String), Failed(String) }`; `duration_for_minutes(u64) -> Result<(u64, &'static str), String>`; `sanitize_name(&str) -> String`; `validate_key(&str) -> Result<String, String>`; `build_form(&CreateParams) -> Result<Vec<(&'static str, String)>, String>`; `parse_create_response(&str) -> CreateOutcome`.

- [ ] **Step 1: Write the fixtures and the failing tests**

These are real `mon_createguest.jsp` responses (WLAN name sanitised). Keep the leading blank line and the batch's script prefix exactly.

`backend/tests/fixtures/create_single.txt`:

````text

{"result":"DONE",
 "errorMsg":"~~",
"key":"RAW134315",
        "fullname":"raw-134315",
        "expiretime":"1791484995",
        "wlan":"Guest",        
        "emailaddr":"null",
        "phonenumber":"null",
        "newnum":"null",
        "totalnum":"null",
        "duration":"null",
        "duration_unit":"~~",        
    "ids":""}
````

`backend/tests/fixtures/create_batch.txt`:

````text
batchEmailData.push('Guest-3|467296|');batchSMSData.push('Guest-3|467296|');batchEmailData.push('Guest-4|797508|');batchSMSData.push('Guest-4|797508|');
{"result":"OK",
 "errorMsg":"~~",
"key":"136467",
        "fullname":"null",
        "expiretime":"1791484995",
        "wlan":"Guest",        
        "emailaddr":"null",
        "phonenumber":"null",
        "newnum":"null",
        "totalnum":"null",
        "duration":"null",
        "duration_unit":"~~",        
    "ids":"3:4"}
````

`backend/tests/fixtures/create_keydup.txt`:

````text

{"result":"KEY_DUPLICATED",
 "errorMsg":"The key key already exists. Please enter a different key.",
"key":"RAW134315",
        "fullname":"raw2-134315",
        "expiretime":"1791484995",
        "wlan":"Guest",        
        "emailaddr":"null",
        "phonenumber":"null",
        "newnum":"null",
        "totalnum":"null",
        "duration":"null",
        "duration_unit":"~~",        
    "ids":""}
````

`backend/tests/create.rs`:

````rust
use backend::unleashed::create::{
    CreateOutcome, CreateParams, MAX_BATCH, build_form, duration_for_minutes,
    parse_create_response, sanitize_name, validate_key,
};

fn params() -> CreateParams {
    CreateParams {
        count: 1,
        name: "Quick Voucher".into(),
        minutes: 60,
        share_limit: None,
        key: None,
        remarks: None,
        ssid: "Guest".into(),
    }
}

fn field<'a>(form: &'a [(&'static str, String)], name: &str) -> &'a str {
    &form.iter().find(|(k, _)| *k == name).unwrap().1
}

#[test]
fn rounds_minutes_up_to_hours() {
    assert_eq!(duration_for_minutes(1), Ok((1, "hour")));
    assert_eq!(duration_for_minutes(60), Ok((1, "hour")));
    assert_eq!(duration_for_minutes(90), Ok((2, "hour")));
    assert_eq!(duration_for_minutes(480), Ok((8, "hour")));
}

#[test]
fn uses_days_and_weeks_when_exact() {
    assert_eq!(duration_for_minutes(1440), Ok((1, "day")));
    assert_eq!(duration_for_minutes(4320), Ok((3, "day")));
    assert_eq!(duration_for_minutes(10080), Ok((1, "week")));
    assert_eq!(duration_for_minutes(43200), Ok((30, "day")));
    assert_eq!(duration_for_minutes(1439), Ok((1, "day")));
}

#[test]
fn rejects_zero_minutes() {
    assert!(duration_for_minutes(0).is_err());
}

#[test]
fn replaces_whitespace_in_names() {
    assert_eq!(sanitize_name("Quick Voucher"), "Quick-Voucher");
    assert_eq!(sanitize_name("  a \t b\nc "), "a-b-c");
    assert_eq!(
        sanitize_name("[ROLLING]-1-192.0.2.1"),
        "[ROLLING]-1-192.0.2.1"
    );
}

#[test]
fn validates_keys() {
    assert_eq!(validate_key("abc123"), Ok("ABC123".into()));
    assert!(validate_key("a").is_err());
    assert!(validate_key("12345678901234567").is_err());
    for bad in [
        "ab cd", "ab#", "ab&", "ab+", "ab\"", "ab'", "ab<", "ab>", "ab,",
    ] {
        assert!(validate_key(bad).is_err(), "{bad} should be rejected");
    }
}

#[test]
fn builds_a_single_pass_form() {
    let form = build_form(&CreateParams {
        share_limit: Some(3),
        key: Some("abc123".into()),
        remarks: Some("front desk".into()),
        ..params()
    })
    .unwrap();
    assert_eq!(field(&form, "gentype"), "single");
    assert_eq!(field(&form, "fullname"), "Quick-Voucher");
    assert_eq!(field(&form, "key"), "ABC123");
    assert_eq!(field(&form, "limitnumber"), "3");
    assert_eq!(field(&form, "remarks"), "front desk");
    assert_eq!(field(&form, "duration"), "1");
    assert_eq!(field(&form, "duration-unit"), "hour");
    assert_eq!(field(&form, "guest-wlan"), "Guest");
    assert_eq!(field(&form, "createToNum"), "");
}

#[test]
fn unlimited_share_is_zero() {
    let form = build_form(&params()).unwrap();
    assert_eq!(field(&form, "limitnumber"), "0");
    assert_eq!(field(&form, "key"), "");
}

#[test]
fn builds_a_batch_form() {
    let form = build_form(&CreateParams {
        count: 5,
        ..params()
    })
    .unwrap();
    assert_eq!(field(&form, "gentype"), "multiple");
    assert_eq!(field(&form, "createToNum"), "5");
    assert_eq!(field(&form, "fullname"), "");
}

#[test]
fn rejects_bad_counts() {
    for count in [0, MAX_BATCH + 1] {
        assert!(build_form(&CreateParams { count, ..params() }).is_err());
    }
    assert!(
        build_form(&CreateParams {
            count: MAX_BATCH,
            ..params()
        })
        .is_ok()
    );
}

#[test]
fn rejects_a_key_on_a_batch() {
    let request = CreateParams {
        count: 2,
        key: Some("abc123".into()),
        ..params()
    };
    assert!(build_form(&request).is_err());
}

#[test]
fn rejects_an_empty_single_name() {
    assert!(
        build_form(&CreateParams {
            name: "   ".into(),
            ..params()
        })
        .is_err()
    );
}

#[test]
fn reads_responses() {
    let read = |name| {
        parse_create_response(
            &std::fs::read_to_string(format!(
                "{}/tests/fixtures/{name}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        )
    };
    assert_eq!(read("create_single.txt"), CreateOutcome::Accepted);
    assert_eq!(read("create_batch.txt"), CreateOutcome::Accepted);
    assert_eq!(
        read("create_keydup.txt"),
        CreateOutcome::KeyDuplicated(
            "The key key already exists. Please enter a different key.".into()
        )
    );
}

#[test]
fn unknown_results_fail() {
    assert_eq!(
        parse_create_response(r#"{"result":"NOPE","errorMsg":"~~"}"#),
        CreateOutcome::Failed("controller answered \"NOPE\"".into())
    );
    assert!(matches!(
        parse_create_response("<!DOCTYPE html>"),
        CreateOutcome::Failed(_)
    ));
}
````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test --test create`
Expected: compile error `unresolved import backend::unleashed::create`.

- [ ] **Step 3: Implement**

Add `pub mod create;` to `backend/src/unleashed/mod.rs` (keep the `pub mod` lines alphabetical; `cargo fmt` sorts them), then `backend/src/unleashed/create.rs`:

````rust
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
````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd backend && cargo test --test create`
Expected: `13 passed`.

- [ ] **Step 5: Commit**

```bash
git add backend/src/unleashed backend/tests/create.rs backend/tests/fixtures/create_*.txt
git commit -m "feat(backend): build Unleashed guest pass create requests"
```

---

### Task 3: Controller session and mock controller

**Files:**
- Create: `backend/src/unleashed/session.rs`
- Create: `backend/tests/support/mod.rs`
- Create: `backend/tests/session.rs`
- Modify: `backend/Cargo.toml`, `backend/Cargo.lock` (reqwest features)
- Modify: `backend/src/unleashed/mod.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: in `backend::unleashed::session`: `enum SessionError { Connect(String), Auth(String) }` (implements `Display`); `struct Session` with `Session::new(url: &str, username: &str, password: &str, verify_tls: bool) -> Result<Session, SessionError>`, `async fn login(&self) -> Result<(), SessionError>`, `async fn conf(&self, xml: &str) -> Result<String, SessionError>`, `async fn form(&self, page: &str, fields: &[(&'static str, String)]) -> Result<String, SessionError>`. Test support: `tests/support/mod.rs` exposes `Mock` (`start()`, `url`, `state`, `expire_sessions()`, `logins()`, `add_pass(name, used, expires_in) -> u32`, `pass_ids()`), `MockState` flags `always_redirect`, `token_in_script_only`, `ignore_creates`, field `create_forms`, and `USERNAME`/`PASSWORD`. Task 5's API tests reuse it.

- [ ] **Step 1: Enable reqwest's cookie store and form bodies**

In `backend/Cargo.toml`, change the reqwest line to:

```toml
reqwest = { version = "0.13.4", features = ["cookies", "form", "json", "query"] }
```

Run: `cd backend && cargo build`
Expected: builds; `Cargo.lock` gains `cookie_store`.

- [ ] **Step 2: Write the mock controller**

`backend/tests/support/mod.rs` imitates the observed controller behaviour: login redirects, the dashboard step that activates a session, CSRF checks, session expiry, the guest list, `delobj`, single and batch creates, `KEY_DUPLICATED`, and the silent no-create for names with whitespace.

````rust
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

async fn root() -> Response {
    redirect("/admin/login.jsp")
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
    let mut response = redirect("/admin/dashboard.jsp");
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
````

- [ ] **Step 3: Write the failing session tests**

`backend/tests/session.rs`:

````rust
mod support;

use backend::unleashed::session::{Session, SessionError};
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
````

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cd backend && cargo test --test session`
Expected: compile error `unresolved import backend::unleashed::session`.

- [ ] **Step 5: Implement the session**

Add `pub mod session;` to `backend/src/unleashed/mod.rs` (alphabetical order), then `backend/src/unleashed/session.rs`:

````rust
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
                response.url(),
                response.status()
            ))
        })?;
    response
        .url()
        .join(location)
        .map_err(|e| SessionError::Connect(format!("bad redirect {location:?}: {e}")))
}

fn connect_error(e: reqwest::Error) -> SessionError {
    warn!("Controller request failed: {e}");
    SessionError::Connect(e.without_url().to_string())
}
````

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd backend && cargo test --test session`
Expected: `8 passed`. The mock only accepts AJAX calls from sessions that loaded `dashboard.jsp`, so `logs_in_and_lists` proves that step is done.

- [ ] **Step 7: Commit**

```bash
git add backend/Cargo.toml backend/Cargo.lock backend/src/unleashed backend/tests/support backend/tests/session.rs
git commit -m "feat(backend): add an Unleashed admin session with re-login"
```

---

### Task 4: Map guest passes to vouchers

**Files:**
- Create: `backend/src/unleashed/mapping.rs`
- Create: `backend/tests/mapping.rs`
- Modify: `backend/src/models.rs` (the `Voucher` struct)
- Modify: `backend/src/unleashed/mod.rs`

**Interfaces:**
- Consumes: `GuestPass` (Task 1).
- Produces: `Voucher` without `data_usage_limit_mbytes`, `rx_rate_limit_kbps`, `tx_rate_limit_kbps`, and with `remarks: String` (`#[serde(default)]`, JSON `remarks`). In `backend::unleashed::mapping`: `ROLLING_PREFIX: &str = "[ROLLING]-"`; `format_time(i64, Tz) -> String`; `is_used(&GuestPass) -> bool`; `is_expired(&GuestPass, now: i64) -> bool`; `to_voucher(&GuestPass, Tz, now: i64) -> Voucher`; `is_rolling(&GuestPass) -> bool`; `rolling_name(DateTime<Tz>, ip: &str) -> String`; `current_rolling(&[GuestPass], now: i64) -> Option<&GuestPass>`; `has_live_rolling_for_ip(&[GuestPass], ip: &str, now: i64) -> bool`.

- [ ] **Step 1: Write the failing tests**

`backend/tests/mapping.rs`:

````rust
use backend::unleashed::{
    guest::GuestPass,
    mapping::{current_rolling, has_live_rolling_for_ip, rolling_name, to_voucher},
};
use chrono::TimeZone;
use chrono_tz::Tz;

const NOW: i64 = 1_791_478_540;

fn pass(id: &str, name: &str) -> GuestPass {
    GuestPass {
        id: id.into(),
        name: name.into(),
        key: "123456".into(),
        remarks: "".into(),
        share_number: 0,
        valid_time: 3600,
        create_time: NOW,
        start_time: Some(NOW),
        expire_time: NOW + 3600,
        used: false,
        client_macs: vec![],
    }
}

#[test]
fn maps_an_unused_pass() {
    let v = to_voucher(&pass("4", "Guest-4"), Tz::UTC, NOW);
    assert_eq!(v.id, "4");
    assert_eq!(v.code, "123456");
    assert_eq!(v.created_at, "2026-10-08 16:55:40");
    assert_eq!(v.expires_at.as_deref(), Some("2026-10-08 17:55:40"));
    assert_eq!(v.activated_at, None, "start-time alone does not mean used");
    assert_eq!(v.authorized_guest_limit, None, "0 is unlimited");
    assert_eq!(v.authorized_guest_count, 0);
    assert_eq!(v.time_limit_minutes, 60);
    assert!(!v.expired);
}

#[test]
fn maps_a_used_pass() {
    let p = GuestPass {
        share_number: 3,
        client_macs: vec!["00:00:00:00:00:01".into(), "00:00:00:00:00:02".into()],
        ..pass("1", "guest-1")
    };
    let v = to_voucher(&p, Tz::UTC, NOW);
    assert_eq!(v.activated_at.as_deref(), Some("2026-10-08 16:55:40"));
    assert_eq!(v.authorized_guest_limit, Some(3));
    assert_eq!(v.authorized_guest_count, 2);
}

#[test]
fn used_flag_without_clients_counts_as_used() {
    let p = GuestPass {
        used: true,
        ..pass("1", "a")
    };
    assert!(to_voucher(&p, Tz::UTC, NOW).activated_at.is_some());
}

#[test]
fn formats_in_the_configured_timezone() {
    let v = to_voucher(&pass("1", "a"), chrono_tz::America::New_York, NOW);
    assert_eq!(v.created_at, "2026-10-08 12:55:40");
}

#[test]
fn expiry_is_inclusive() {
    let p = pass("1", "a");
    assert!(!to_voucher(&p, Tz::UTC, p.expire_time - 1).expired);
    assert!(to_voucher(&p, Tz::UTC, p.expire_time).expired);
}

#[test]
fn names_rolling_passes_without_whitespace() {
    let created = Tz::UTC.timestamp_opt(NOW, 0).unwrap();
    let name = rolling_name(created, "192.0.2.7");
    assert_eq!(name, "[ROLLING]-20261008165540-192.0.2.7");
    assert!(!name.contains(char::is_whitespace));
}

#[test]
fn picks_the_newest_unused_unexpired_rolling_pass() {
    let passes = vec![
        GuestPass {
            create_time: NOW - 10,
            ..pass("1", "[ROLLING]-a-192.0.2.1")
        },
        GuestPass {
            create_time: NOW - 5,
            ..pass("2", "[ROLLING]-b-192.0.2.2")
        },
        GuestPass {
            used: true,
            ..pass("3", "[ROLLING]-c-192.0.2.3")
        },
        GuestPass {
            expire_time: NOW,
            ..pass("4", "[ROLLING]-d-192.0.2.4")
        },
        pass("5", "not rolling"),
    ];
    assert_eq!(current_rolling(&passes, NOW).unwrap().id, "2");
}

#[test]
fn no_rolling_pass_when_all_used_or_expired() {
    let passes = vec![
        GuestPass {
            used: true,
            ..pass("1", "[ROLLING]-a-192.0.2.1")
        },
        GuestPass {
            expire_time: NOW - 1,
            ..pass("2", "[ROLLING]-b-192.0.2.2")
        },
    ];
    assert!(current_rolling(&passes, NOW).is_none());
}

#[test]
fn matches_rolling_passes_by_whole_ip() {
    let passes = vec![pass("1", "[ROLLING]-20261008133540-11.2.3.4")];
    assert!(has_live_rolling_for_ip(&passes, "11.2.3.4", NOW));
    assert!(!has_live_rolling_for_ip(&passes, "1.2.3.4", NOW));
}

#[test]
fn expired_rolling_passes_do_not_block_an_ip() {
    let passes = vec![GuestPass {
        expire_time: NOW - 1,
        ..pass("1", "[ROLLING]-20261008133540-192.0.2.1")
    }];
    assert!(!has_live_rolling_for_ip(&passes, "192.0.2.1", NOW));
}
````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test --test mapping`
Expected: compile error `unresolved import backend::unleashed::mapping`.

- [ ] **Step 3: Change the `Voucher` model**

In `backend/src/models.rs`, replace the whole `Voucher` struct (not `VouchersCreateRequest`, which ends with the same
four fields and changes in Task 5) with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voucher {
    pub id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub name: String,
    pub code: String,
    #[serde(rename = "authorizedGuestLimit")]
    pub authorized_guest_limit: Option<u64>,
    #[serde(rename = "authorizedGuestCount")]
    pub authorized_guest_count: u64,
    #[serde(rename = "activatedAt")]
    pub activated_at: Option<String>,
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<String>,
    pub expired: bool,
    #[serde(rename = "timeLimitMinutes")]
    pub time_limit_minutes: u64,
    #[serde(default)]
    pub remarks: String,
}
```

`unifi_api.rs` still compiles, because it only deserialises `Voucher` and the new field has a default. It is removed in Task 5.

- [ ] **Step 4: Implement the mapping**

Add `pub mod mapping;` to `backend/src/unleashed/mod.rs` (alphabetical order), then `backend/src/unleashed/mapping.rs`:

````rust
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
````

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd backend && cargo test`
Expected: `mapping` reports `10 passed`, and every other test binary still passes.

- [ ] **Step 6: Commit**

```bash
git add backend/src/models.rs backend/src/unleashed backend/tests/mapping.rs
git commit -m "feat(backend): map guest passes to vouchers and rolling rules"
```

---

### Task 5: Switch the backend to Unleashed

**Files:**
- Create: `backend/src/unleashed_api.rs`
- Create: `backend/tests/api.rs`
- Create: `backend/tests/forwarded.rs`
- Delete: `backend/src/unifi_api.rs`
- Modify (full replacements below): `backend/src/environment.rs`, `backend/src/models.rs`, `backend/src/handlers.rs`, `backend/src/main.rs`, `backend/src/lib.rs`
- Modify: `backend/Cargo.toml`, `backend/Cargo.lock` (drop `percent-encoding`)

`backend/src/tasks.rs` does not change: it calls `delete_expired_handler` and `delete_expired_rolling_handler`, which keep their names.

**Interfaces:**
- Consumes: everything from Tasks 1 to 4.
- Produces: `backend::unleashed_api::{ApiConfig, UnleashedAPI, UNLEASHED_API}`. `ApiConfig { url, username, password, ssid: String, verify_tls: bool, timezone: Tz, rolling_voucher_duration_minutes: u64 }` with `ApiConfig::from_environment(&Environment)`. `UnleashedAPI::try_new(ApiConfig) -> Result<Self, String>` and the handler-facing methods `get_all_vouchers`, `get_vouchers(&VouchersGetRequest)`, `get_voucher_details(String)`, `get_newest_voucher`, `get_rolling_voucher -> Option<Voucher>`, `create_voucher(&VouchersCreateRequest) -> VouchersCreateResponse`, `check_rolling_voucher_ip(&str) -> bool`, `create_rolling_voucher(&str) -> Voucher`, `delete_vouchers_by_ids(Vec<String>)`, `delete_expired_vouchers`, `delete_expired_rolling_vouchers`, all returning `Result<_, reqwest::StatusCode>`. `VouchersCreateRequest` gains `code: Option<String>` and `remarks: Option<String>` and loses the data and rate fields. `backend::handlers::first_forwarded_ip(&str) -> Option<&str>`. The HTTP routes and their JSON do not change apart from those fields.

- [ ] **Step 1: Write the failing tests**

`backend/tests/forwarded.rs`:

````rust
use backend::handlers::first_forwarded_ip;

#[test]
fn takes_the_first_forwarded_address() {
    assert_eq!(first_forwarded_ip("192.0.2.7"), Some("192.0.2.7"));
    assert_eq!(first_forwarded_ip("192.0.2.7, 10.0.0.1"), Some("192.0.2.7"));
    assert_eq!(
        first_forwarded_ip(" 192.0.2.7 ,10.0.0.1"),
        Some("192.0.2.7")
    );
    assert_eq!(first_forwarded_ip(""), None);
    assert_eq!(first_forwarded_ip(" , 10.0.0.1"), None);
}
````

`backend/tests/api.rs`:

````rust
mod support;

use backend::{
    models::VouchersCreateRequest,
    unleashed_api::{ApiConfig, UnleashedAPI},
};
use reqwest::StatusCode;
use support::{Mock, PASSWORD, USERNAME};

async fn api(mock: &Mock) -> UnleashedAPI {
    UnleashedAPI::try_new(ApiConfig {
        url: mock.url.clone(),
        username: USERNAME.into(),
        password: PASSWORD.into(),
        ssid: "Guest".into(),
        verify_tls: true,
        timezone: chrono_tz::Tz::UTC,
        rolling_voucher_duration_minutes: 480,
    })
    .await
    .unwrap()
}

fn request(count: u32, name: &str) -> VouchersCreateRequest {
    VouchersCreateRequest {
        count,
        name: name.into(),
        authorized_guest_limit: None,
        time_limit_minutes: 1440,
        code: None,
        remarks: None,
    }
}

#[tokio::test]
async fn startup_fails_on_bad_credentials() {
    let mock = Mock::start().await;
    let result = UnleashedAPI::try_new(ApiConfig {
        url: mock.url.clone(),
        username: USERNAME.into(),
        password: "wrong".into(),
        ssid: "Guest".into(),
        verify_tls: true,
        timezone: chrono_tz::Tz::UTC,
        rolling_voucher_duration_minutes: 480,
    })
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn creates_a_single_pass_with_a_spaced_name() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let created = api
        .create_voucher(&request(1, "Quick Voucher"))
        .await
        .unwrap();
    assert_eq!(created.vouchers.len(), 1);
    let v = &created.vouchers[0];
    assert_eq!(v.name, "Quick-Voucher");
    assert_eq!(v.time_limit_minutes, 1440);
    let form = &mock.state.lock().unwrap().create_forms[0];
    assert_eq!(form["duration"], "1");
    assert_eq!(form["duration-unit"], "day");
    assert_eq!(form["guest-wlan"], "Guest");
}

#[tokio::test]
async fn duplicate_names_are_fine() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    api.create_voucher(&request(1, "Quick Voucher"))
        .await
        .unwrap();
    api.create_voucher(&request(1, "Quick Voucher"))
        .await
        .unwrap();
    assert_eq!(api.get_all_vouchers().await.unwrap().data.len(), 2);
}

#[tokio::test]
async fn creates_a_batch() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    mock.add_pass("existing", false, 3600);
    let created = api.create_voucher(&request(3, "ignored")).await.unwrap();
    assert_eq!(
        created.vouchers.len(),
        3,
        "only the new passes are returned"
    );
}

#[tokio::test]
async fn custom_key_and_remarks() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let created = api
        .create_voucher(&VouchersCreateRequest {
            code: Some("front1".into()),
            remarks: Some("desk".into()),
            authorized_guest_limit: Some(3),
            ..request(1, "desk")
        })
        .await
        .unwrap();
    let v = &created.vouchers[0];
    assert_eq!(v.code, "FRONT1");
    assert_eq!(v.remarks, "desk");
    assert_eq!(v.authorized_guest_limit, Some(3));
}

#[tokio::test]
async fn duplicate_key_is_a_conflict() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let with_key = VouchersCreateRequest {
        code: Some("SAME1".into()),
        ..request(1, "a")
    };
    api.create_voucher(&with_key).await.unwrap();
    assert_eq!(
        api.create_voucher(&with_key).await.unwrap_err(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn invalid_requests_are_bad_requests() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    for bad in [
        request(0, "a"),
        request(101, "a"),
        VouchersCreateRequest {
            time_limit_minutes: 0,
            ..request(1, "a")
        },
        VouchersCreateRequest {
            code: Some("AB".into()),
            ..request(2, "a")
        },
        VouchersCreateRequest {
            code: Some("a b".into()),
            ..request(1, "a")
        },
    ] {
        assert_eq!(
            api.create_voucher(&bad).await.unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }
    assert!(mock.state.lock().unwrap().create_forms.is_empty());
}

#[tokio::test]
async fn an_ok_that_creates_nothing_is_an_error() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    mock.state.lock().unwrap().ignore_creates = true;
    assert_eq!(
        api.create_voucher(&request(1, "a")).await.unwrap_err(),
        StatusCode::BAD_GATEWAY
    );
}

#[tokio::test]
async fn lists_newest_first_and_filters_by_name() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    api.create_voucher(&request(1, "alpha")).await.unwrap();
    api.create_voucher(&request(1, "Beta")).await.unwrap();
    let all = api.get_all_vouchers().await.unwrap();
    assert_eq!(all.total_count, 2);
    let filtered = api
        .get_vouchers(&backend::models::VouchersGetRequest {
            offset: 0,
            limit: 10,
            filter: Some("BET".into()),
        })
        .await
        .unwrap();
    assert_eq!(filtered.data.len(), 1);
    assert_eq!(filtered.data[0].name, "Beta");
}

#[tokio::test]
async fn details_and_newest() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let id = mock.add_pass("one", false, 3600).to_string();
    assert_eq!(api.get_voucher_details(id).await.unwrap().name, "one");
    assert_eq!(
        api.get_voucher_details("999".into()).await.unwrap_err(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(api.get_newest_voucher().await.unwrap().name, "one");
}

#[tokio::test]
async fn deletes_selected_passes() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let a = mock.add_pass("a", false, 3600);
    let b = mock.add_pass("b", false, 3600);
    let c = mock.add_pass("c", false, 3600);
    let deleted = api
        .delete_vouchers_by_ids(vec![a.to_string(), c.to_string(), "999".into()])
        .await
        .unwrap();
    assert_eq!(deleted.vouchers_deleted, 2, "missing ids are not counted");
    assert_eq!(mock.pass_ids(), vec![b]);
}

#[tokio::test]
async fn rejects_non_numeric_ids() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let a = mock.add_pass("a", false, 3600);
    let injection = "1'></guest><guest id='2".to_string();
    assert_eq!(
        api.delete_vouchers_by_ids(vec![injection])
            .await
            .unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(mock.pass_ids(), vec![a]);
}

#[tokio::test]
async fn deleting_nothing_is_fine() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    assert_eq!(
        api.delete_vouchers_by_ids(vec!["".into()])
            .await
            .unwrap()
            .vouchers_deleted,
        0
    );
    assert_eq!(
        api.delete_expired_vouchers()
            .await
            .unwrap()
            .vouchers_deleted,
        0
    );
    assert_eq!(
        api.delete_expired_rolling_vouchers()
            .await
            .unwrap()
            .vouchers_deleted,
        0
    );
}

#[tokio::test]
async fn rolling_vouchers_roll() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    assert!(api.get_rolling_voucher().await.unwrap().is_none());

    let first = api.create_rolling_voucher("192.0.2.10").await.unwrap();
    assert!(first.name.starts_with("[ROLLING]-"));
    assert!(first.name.ends_with("-192.0.2.10"));
    assert_eq!(first.time_limit_minutes, 480);
    assert_eq!(
        api.get_rolling_voucher().await.unwrap().unwrap().id,
        first.id
    );

    assert!(api.check_rolling_voucher_ip("192.0.2.10").await.unwrap());
    assert!(!api.check_rolling_voucher_ip("192.0.2.1").await.unwrap());

    // A guest uses the first pass; the next one becomes current.
    {
        let mut s = mock.state.lock().unwrap();
        let pass = s
            .passes
            .iter_mut()
            .find(|p| p.id.to_string() == first.id)
            .unwrap();
        pass.client_macs.push("00:00:00:00:00:09".into());
    }
    assert!(api.get_rolling_voucher().await.unwrap().is_none());
    let second = api.create_rolling_voucher("192.0.2.11").await.unwrap();
    assert_eq!(
        api.get_rolling_voucher().await.unwrap().unwrap().id,
        second.id
    );
}

#[tokio::test]
async fn survives_a_controller_reboot() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    mock.expire_sessions();
    assert!(api.get_all_vouchers().await.is_ok());
    assert_eq!(mock.logins(), 2);
}
````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd backend && cargo test --test api --test forwarded`
Expected: compile errors `unresolved import backend::unleashed_api` and `unresolved import backend::handlers::first_forwarded_ip`.

- [ ] **Step 3: Replace the environment**

`backend/src/environment.rs`:

````rust
use std::{env, sync::OnceLock};

use chrono_tz::Tz;
use tracing::{error, info};

const DEFAULT_BACKEND_BIND_HOST: &str = "127.0.0.1";
const DEFAULT_BACKEND_BIND_PORT: u16 = 8080;
const DEFAULT_ROLLING_VOUCHER_DURATION_MINUTES: u64 = 480;

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
    pub purge_all_expired_vouchers: bool,
    pub rolling_voucher_duration_minutes: u64,
    pub timezone: Tz,
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
````

- [ ] **Step 4: Replace the models**

`backend/src/models.rs` (UniFi-only `Site`, `GetSitesResponse` and `ErrorResponse` are gone):

````rust
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voucher {
    pub id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub name: String,
    pub code: String,
    #[serde(rename = "authorizedGuestLimit")]
    pub authorized_guest_limit: Option<u64>,
    #[serde(rename = "authorizedGuestCount")]
    pub authorized_guest_count: u64,
    #[serde(rename = "activatedAt")]
    pub activated_at: Option<String>,
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<String>,
    pub expired: bool,
    #[serde(rename = "timeLimitMinutes")]
    pub time_limit_minutes: u64,
    #[serde(default)]
    pub remarks: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VouchersCreateRequest {
    pub count: u32,
    pub name: String,
    #[serde(rename = "authorizedGuestLimit")]
    pub authorized_guest_limit: Option<u64>,
    #[serde(rename = "timeLimitMinutes")]
    pub time_limit_minutes: u64,
    /// A custom pass key; only for a count of 1.
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub remarks: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VouchersCreateResponse {
    #[serde(alias = "data")]
    pub vouchers: Vec<Voucher>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VouchersGetResponse {
    pub offset: u64,
    pub limit: u32,
    pub count: u32,
    #[serde(rename = "totalCount")]
    pub total_count: u64,
    pub data: Vec<Voucher>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteResponse {
    #[serde(rename = "vouchersDeleted")]
    pub vouchers_deleted: u32,
}

#[derive(Debug, Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VouchersGetRequest {
    pub offset: u32,
    pub limit: u32,
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VouchersDeleteRequest {
    pub ids: String,
}

#[derive(Debug, Deserialize)]
pub struct VoucherDetailsRequest {
    pub id: String,
}
````

- [ ] **Step 5: Add the API and remove the UniFi client**

Run: `git rm backend/src/unifi_api.rs && cd backend && cargo remove percent-encoding`

`backend/src/unleashed_api.rs`:

````rust
//! The operations the HTTP handlers need, implemented on Unleashed guest
//! passes.

use std::{collections::HashSet, sync::OnceLock};

use chrono::Utc;
use chrono_tz::Tz;
use reqwest::StatusCode;
use tracing::{error, info, warn};

use crate::{
    environment::Environment,
    models::*,
    unleashed::{
        create::{CreateOutcome, CreateParams, build_form, parse_create_response},
        guest::{GuestPass, parse_guest_list},
        mapping::{
            current_rolling, has_live_rolling_for_ip, is_expired, is_rolling, rolling_name,
            to_voucher,
        },
        session::{Session, SessionError},
    },
};

pub static UNLEASHED_API: OnceLock<UnleashedAPI> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub ssid: String,
    pub verify_tls: bool,
    pub timezone: Tz,
    pub rolling_voucher_duration_minutes: u64,
}

impl ApiConfig {
    pub fn from_environment(env: &Environment) -> Self {
        Self {
            url: env.unleashed_url.clone(),
            username: env.unleashed_username.clone(),
            password: env.unleashed_password.clone(),
            ssid: env.unleashed_ssid.clone(),
            verify_tls: env.unleashed_has_valid_cert,
            timezone: env.timezone,
            rolling_voucher_duration_minutes: env.rolling_voucher_duration_minutes,
        }
    }
}

pub struct UnleashedAPI {
    session: Session,
    config: ApiConfig,
}

impl UnleashedAPI {
    /// Connect and log in, so bad settings fail at startup.
    pub async fn try_new(config: ApiConfig) -> Result<Self, String> {
        let session = Session::new(
            &config.url,
            &config.username,
            &config.password,
            config.verify_tls,
        )
        .map_err(|e| e.to_string())?;
        session.login().await.map_err(|e| e.to_string())?;
        Ok(Self { session, config })
    }

    fn now() -> i64 {
        Utc::now().timestamp()
    }

    async fn list_passes(&self) -> Result<Vec<GuestPass>, StatusCode> {
        let request = format!(
            "<ajax-request action='getconf' DECRYPT_X='true' updater='guest-list.{}' \
             comp='guest-list'><guest self-service='!true'/></ajax-request>",
            Utc::now().timestamp_millis()
        );
        let body = self.session.conf(&request).await.map_err(gateway)?;
        parse_guest_list(&body).map_err(|e| {
            error!("Could not read the guest list: {e}");
            StatusCode::BAD_GATEWAY
        })
    }

    /// Newest first.
    fn to_vouchers(&self, passes: &[GuestPass]) -> Vec<Voucher> {
        let now = Self::now();
        let mut sorted: Vec<&GuestPass> = passes.iter().collect();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.create_time));
        sorted
            .into_iter()
            .map(|p| to_voucher(p, self.config.timezone, now))
            .collect()
    }

    pub async fn get_all_vouchers(&self) -> Result<VouchersGetResponse, StatusCode> {
        let data = self.to_vouchers(&self.list_passes().await?);
        Ok(VouchersGetResponse {
            offset: 0,
            limit: data.len() as u32,
            count: data.len() as u32,
            total_count: data.len() as u64,
            data,
        })
    }

    /// `filter` matches names case-insensitively.
    pub async fn get_vouchers(
        &self,
        request: &VouchersGetRequest,
    ) -> Result<VouchersGetResponse, StatusCode> {
        let mut data = self.to_vouchers(&self.list_passes().await?);
        if let Some(filter) = request.filter.as_deref().filter(|f| !f.is_empty()) {
            let filter = filter.to_lowercase();
            data.retain(|v| v.name.to_lowercase().contains(&filter));
        }
        let total_count = data.len() as u64;
        let data: Vec<Voucher> = data
            .into_iter()
            .skip(request.offset as usize)
            .take(request.limit as usize)
            .collect();
        Ok(VouchersGetResponse {
            offset: request.offset as u64,
            limit: request.limit,
            count: data.len() as u32,
            total_count,
            data,
        })
    }

    pub async fn get_voucher_details(&self, id: String) -> Result<Voucher, StatusCode> {
        let passes = self.list_passes().await?;
        let pass = passes
            .iter()
            .find(|p| p.id == id)
            .ok_or(StatusCode::NOT_FOUND)?;
        Ok(to_voucher(pass, self.config.timezone, Self::now()))
    }

    pub async fn get_newest_voucher(&self) -> Result<Voucher, StatusCode> {
        self.to_vouchers(&self.list_passes().await?)
            .into_iter()
            .next()
            .ok_or(StatusCode::NOT_FOUND)
    }

    pub async fn get_rolling_voucher(&self) -> Result<Option<Voucher>, StatusCode> {
        let passes = self.list_passes().await?;
        let now = Self::now();
        Ok(current_rolling(&passes, now).map(|p| to_voucher(p, self.config.timezone, now)))
    }

    pub async fn create_voucher(
        &self,
        request: &VouchersCreateRequest,
    ) -> Result<VouchersCreateResponse, StatusCode> {
        let params = CreateParams {
            count: request.count,
            name: request.name.clone(),
            minutes: request.time_limit_minutes,
            share_limit: request.authorized_guest_limit,
            key: request.code.clone().filter(|c| !c.is_empty()),
            remarks: request.remarks.clone(),
            ssid: self.config.ssid.clone(),
        };
        let form = build_form(&params).map_err(|e| {
            warn!("Rejected create request: {e}");
            StatusCode::BAD_REQUEST
        })?;

        let before: HashSet<String> = self
            .list_passes()
            .await?
            .into_iter()
            .map(|p| p.id)
            .collect();
        let body = self
            .session
            .form("mon_createguest.jsp", &form)
            .await
            .map_err(gateway)?;
        match parse_create_response(&body) {
            CreateOutcome::Accepted => {}
            CreateOutcome::KeyDuplicated(message) => {
                warn!("Create rejected: {message}");
                return Err(StatusCode::CONFLICT);
            }
            CreateOutcome::Failed(message) => {
                error!("Create failed: {message}");
                return Err(StatusCode::BAD_GATEWAY);
            }
        }

        // The response alone is not proof: the controller can answer OK and
        // create nothing.
        let created: Vec<GuestPass> = self
            .list_passes()
            .await?
            .into_iter()
            .filter(|p| !before.contains(&p.id))
            .collect();
        if created.is_empty() {
            error!("The controller accepted the create request but no new pass appeared");
            return Err(StatusCode::BAD_GATEWAY);
        }
        Ok(VouchersCreateResponse {
            vouchers: self.to_vouchers(&created),
        })
    }

    pub async fn check_rolling_voucher_ip(&self, ip: &str) -> Result<bool, StatusCode> {
        let passes = self.list_passes().await?;
        Ok(has_live_rolling_for_ip(&passes, ip, Self::now()))
    }

    pub async fn create_rolling_voucher(&self, ip: &str) -> Result<Voucher, StatusCode> {
        let request = VouchersCreateRequest {
            count: 1,
            name: rolling_name(Utc::now().with_timezone(&self.config.timezone), ip),
            authorized_guest_limit: None,
            time_limit_minutes: self.config.rolling_voucher_duration_minutes,
            code: None,
            remarks: None,
        };
        self.create_voucher(&request)
            .await?
            .vouchers
            .into_iter()
            .next()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Deletes the passes among `ids` that exist. Ids are the controller's
    /// numeric pass ids; anything else is rejected before it reaches the XML.
    pub async fn delete_vouchers_by_ids(
        &self,
        ids: Vec<String>,
    ) -> Result<DeleteResponse, StatusCode> {
        let ids: Vec<String> = ids.into_iter().filter(|id| !id.is_empty()).collect();
        if ids.iter().any(|id| !id.chars().all(|c| c.is_ascii_digit())) {
            warn!("Rejected delete request with a non-numeric id");
            return Err(StatusCode::BAD_REQUEST);
        }
        let existing: HashSet<String> = self
            .list_passes()
            .await?
            .into_iter()
            .map(|p| p.id)
            .collect();
        let targets: Vec<String> = ids.into_iter().filter(|id| existing.contains(id)).collect();
        self.delete_ids(&targets).await
    }

    pub async fn delete_expired_vouchers(&self) -> Result<DeleteResponse, StatusCode> {
        let now = Self::now();
        let targets: Vec<String> = self
            .list_passes()
            .await?
            .into_iter()
            .filter(|p| is_expired(p, now))
            .map(|p| p.id)
            .collect();
        self.delete_ids(&targets).await
    }

    pub async fn delete_expired_rolling_vouchers(&self) -> Result<DeleteResponse, StatusCode> {
        let now = Self::now();
        let targets: Vec<String> = self
            .list_passes()
            .await?
            .into_iter()
            .filter(|p| is_rolling(p) && is_expired(p, now))
            .map(|p| p.id)
            .collect();
        self.delete_ids(&targets).await
    }

    async fn delete_ids(&self, ids: &[String]) -> Result<DeleteResponse, StatusCode> {
        if ids.is_empty() {
            return Ok(DeleteResponse {
                vouchers_deleted: 0,
            });
        }
        let guests: String = ids
            .iter()
            .map(|id| format!("<guest id='{id}'></guest>"))
            .collect();
        let request = format!(
            "<ajax-request action='delobj' updater='guest-list.{}' comp='guest-list'>{guests}\
             </ajax-request>",
            Utc::now().timestamp_millis()
        );
        self.session.conf(&request).await.map_err(gateway)?;
        info!("Deleted {} guest passes", ids.len());
        Ok(DeleteResponse {
            vouchers_deleted: ids.len() as u32,
        })
    }
}

fn gateway(e: SessionError) -> StatusCode {
    error!("{e}");
    StatusCode::BAD_GATEWAY
}
````

`backend/src/lib.rs`:

````rust
pub mod environment;
pub mod handlers;
pub mod models;
pub mod tasks;
pub mod unleashed;
pub mod unleashed_api;
````

- [ ] **Step 6: Update the handlers and main**

`backend/src/handlers.rs` (only the import, the `UNLEASHED_API` lookups, the `X-Forwarded-For` parsing and the new helper differ from UVM):

````rust
use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode},
    response::Json,
};
use tracing::{debug, error, info};

use crate::{models::*, unleashed_api::UNLEASHED_API};

pub async fn get_vouchers_filtered_handler(
    Query(params): Query<VouchersGetRequest>,
) -> Result<Json<VouchersGetResponse>, StatusCode> {
    debug!("Received request to get vouchers");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_vouchers(&params).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to get vouchers: {}", e);
            Err(e)
        }
    }
}

pub async fn get_all_vouchers_handler() -> Result<Json<VouchersGetResponse>, StatusCode> {
    debug!("Received request to get all vouchers");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_all_vouchers().await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to get vouchers: {}", e);
            Err(e)
        }
    }
}

pub async fn get_rolling_voucher_handler() -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request to get rolling voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_rolling_voucher().await {
        Ok(Some(voucher)) => Ok(Json(voucher)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get rolling voucher: {}", e);
            Err(e)
        }
    }
}

pub async fn get_newest_voucher_handler() -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request to get newest voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_newest_voucher().await {
        Ok(voucher) => Ok(Json(voucher)),
        Err(e) => {
            error!("Failed to get newest voucher: {}", e);
            Err(e)
        }
    }
}

pub async fn get_voucher_details_handler(
    Query(params): Query<VoucherDetailsRequest>,
) -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request to get voucher details");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_voucher_details(params.id).await {
        Ok(voucher) => Ok(Json(voucher)),
        Err(e) => {
            error!("Failed to get voucher details: {}", e);
            Err(e)
        }
    }
}

pub async fn create_voucher_handler(
    Json(request): Json<VouchersCreateRequest>,
) -> Result<Json<VouchersCreateResponse>, StatusCode> {
    debug!("Received request to create voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.create_voucher(&request).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to create voucher: {}", e);
            Err(e)
        }
    }
}

pub async fn create_rolling_voucher_handler(
    headers: HeaderMap,
) -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request to create voucher");

    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");

    if let Some(forwarded) = headers.get("x-forwarded-for")
        && let Ok(forwarded) = forwarded.to_str()
        && let Some(ip) = first_forwarded_ip(forwarded)
    {
        debug!("Client IP from x-forwarded-for: {}", ip);

        // Check if user already rotated the rolling voucher
        if client.check_rolling_voucher_ip(ip).await? {
            info!("Rolling voucher already rotated for IP: {}", ip);
            return Err(StatusCode::FORBIDDEN);
        }

        // Voucher rotation allowed, create a new rolling voucher
        match client.create_rolling_voucher(ip).await {
            Ok(response) => return Ok(Json(response)),
            Err(e) => {
                error!("Failed to create rolling voucher: {}", e);
                return Err(e);
            }
        }
    }

    error!("Invalid x-forwarded-for header");
    Err(StatusCode::BAD_REQUEST)
}

pub async fn delete_selected_handler(
    Query(params): Query<VouchersDeleteRequest>,
) -> Result<Json<DeleteResponse>, StatusCode> {
    debug!("Received request to delete selected vouchers");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    let ids = params.ids.split(',').map(|s| s.to_string()).collect();
    match client.delete_vouchers_by_ids(ids).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to delete selected vouchers: {}", e);
            Err(e)
        }
    }
}

pub async fn delete_expired_handler() -> Result<Json<DeleteResponse>, StatusCode> {
    debug!("Received request to delete expired vouchers");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.delete_expired_vouchers().await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to delete expired vouchers: {}", e);
            Err(e)
        }
    }
}

pub async fn delete_expired_rolling_handler() -> Result<Json<DeleteResponse>, StatusCode> {
    debug!("Received request to delete expired rolling voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.delete_expired_rolling_vouchers().await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to delete expired rolling voucher: {}", e);
            Err(e)
        }
    }
}

pub async fn health_check_handler() -> Result<Json<HealthCheckResponse>, StatusCode> {
    debug!("Received health check request");
    let response = HealthCheckResponse {
        status: "ok".to_string(),
    };
    Ok(Json(response))
}

/// The client address from an `X-Forwarded-For` value: the first entry of a
/// comma-separated list, trimmed.
pub fn first_forwarded_ip(header: &str) -> Option<&str> {
    header
        .split(',')
        .next()
        .map(str::trim)
        .filter(|ip| !ip.is_empty())
}
````

`backend/src/main.rs`:

````rust
use axum::{
    Router,
    http::{self, Method},
    routing::{delete, get, post},
};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, level_filters::LevelFilter, warn};
use tracing_subscriber::EnvFilter;

use backend::{
    environment::{ENVIRONMENT, Environment},
    handlers::*,
    tasks::run_daily_purge,
    unleashed_api::{ApiConfig, UNLEASHED_API, UnleashedAPI},
};

#[tokio::main]
async fn main() {
    // =================================
    // Initialize tracing
    // =================================
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_env_var("BACKEND_LOG_LEVEL")
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    // =================================
    // Setup environment variables manager
    // =================================
    let env = match Environment::try_new() {
        Ok(env) => env,
        Err(e) => {
            error!("Failed to load environment variables: {e}");
            std::process::exit(1);
        }
    };
    ENVIRONMENT
        .set(env)
        .expect("Failed to set environment variables");
    let environment = ENVIRONMENT.get().expect("Environment not set");

    // =================================
    // Connect to the Unleashed controller
    // =================================
    loop {
        match UnleashedAPI::try_new(ApiConfig::from_environment(environment)).await {
            Ok(api) => {
                if UNLEASHED_API.set(api).is_err() {
                    panic!("UnleashedAPI already initialized");
                }
                info!("Connected to the Unleashed controller");
                break;
            }
            Err(e) => {
                error!("Failed to connect to the Unleashed controller: {}", e);
                warn!("Retrying connection in 5 seconds...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    // =================================
    // Start scheduled tasks
    // =================================
    tokio::spawn(run_daily_purge(
        environment.timezone,
        environment.purge_all_expired_vouchers,
    ));

    // =================================
    // Setup Axum server
    // =================================
    let cors = CorsLayer::new()
        .allow_headers([http::header::CONTENT_TYPE])
        .allow_methods([Method::POST, Method::GET, Method::DELETE])
        .allow_origin(Any);

    let app = Router::new()
        .route("/api/health", get(health_check_handler))
        .route("/api/vouchers", get(get_all_vouchers_handler))
        .route("/api/vouchers", post(create_voucher_handler))
        .route("/api/vouchers/details", get(get_voucher_details_handler))
        .route("/api/vouchers/expired", delete(delete_expired_handler))
        .route("/api/vouchers/filtered", get(get_vouchers_filtered_handler))
        .route(
            "/api/vouchers/expired/rolling",
            delete(delete_expired_rolling_handler),
        )
        .route("/api/vouchers/newest", get(get_newest_voucher_handler))
        .route("/api/vouchers/rolling", get(get_rolling_voucher_handler))
        .route(
            "/api/vouchers/rolling",
            post(create_rolling_voucher_handler),
        )
        .route("/api/vouchers/selected", delete(delete_selected_handler))
        .layer(cors);

    let bind_address = format!(
        "{}:{}",
        environment.backend_bind_host, environment.backend_bind_port
    );

    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("Could not bind listener");

    info!("Server running on http://{}", bind_address);

    axum::serve(listener, app)
        .await
        .expect("Axum server should never error");
}
````

- [ ] **Step 7: Run the whole backend suite**

Run: `cd backend && cargo test`
Expected: `api` 15 passed, `forwarded` 1 passed, and `guest` 7, `create` 13, `mapping` 10, `session` 8 still passing.

Run: `cd backend && cargo clippy --all-targets && cargo fmt --check`
Expected: no warnings, no diff.

- [ ] **Step 8: Commit**

```bash
git add -A backend
git commit -m "feat(backend)!: replace the UniFi client with Unleashed"
```

---

### Task 6: Port the frontend

**Files:**
- Modify: `frontend/src/types/voucher.ts` (full replacement)
- Modify: `frontend/src/components/tabs/CustomCreateTab.tsx` (full replacement)
- Modify: `frontend/public/logo.svg` (full replacement)
- Modify (diffs): `frontend/src/utils/api.ts`, `frontend/src/types/print.ts`, `frontend/src/types/config.ts`, `frontend/src/utils/format.ts`, `frontend/src/app/print/page.tsx`, `frontend/src/components/modals/VoucherModal.tsx`, `frontend/src/components/VoucherCard.tsx`, `frontend/src/components/tabs/TestTab.tsx`, `frontend/src/app/layout.tsx`, `frontend/src/components/Header.tsx`, `frontend/package.json`, `frontend/package-lock.json`

**Interfaces:**
- Consumes: the backend JSON from Task 5: `Voucher` has `remarks` and no data or rate fields; create accepts `code` and `remarks`; `409` means the key is taken.
- Produces: `Voucher` and `VoucherCreateData` types as below; constants `MAX_VOUCHER_DURATION_HOURS`, `MAX_VOUCHER_COUNT = 100`, `MIN_VOUCHER_KEY_LENGTH`, `MAX_VOUCHER_KEY_LENGTH` in `utils/api.ts`; `PrintConfig` without `showDataUsageLimit`, `showRxRateLimit`, `showTxRateLimit`.

The frontend has no test runner; the typecheck and build are the tests, and the compiler finds every remaining use of a removed field.

- [ ] **Step 1: Change the types and watch the typecheck fail**

`frontend/src/types/voucher.ts`:

````ts
export interface Voucher {
  id: string;
  createdAt: string;
  name: string;
  code: string;
  authorizedGuestLimit?: number | null;
  authorizedGuestCount: number;
  activatedAt?: string | null;
  expiresAt?: string | null;
  expired: boolean;
  timeLimitMinutes: number;
  remarks: string;
}

export interface VoucherCreateData {
  count: number;
  name: string;
  timeLimitMinutes: number;
  authorizedGuestLimit?: number | null;
  /** A custom pass key; only allowed when count is 1. */
  code?: string | null;
  remarks?: string | null;
}

export interface VoucherGetResponse {
  offset: number;
  limit: number;
  count: number;
  totalCount: number;
  data: Voucher[];
}

export interface VoucherDeletedResponse {
  vouchersDeleted: number;
}

export interface VoucherCreatedResponse {
  vouchers: Voucher[];
}
````

Run: `cd frontend && npm ci && npx tsc --noEmit`
Expected: errors in `CustomCreateTab.tsx`, `VoucherModal.tsx`, `print/page.tsx` and `TestTab.tsx` naming the removed fields.

- [ ] **Step 2: Replace the limits in `utils/api.ts`**

````diff
diff --git a/frontend/src/utils/api.ts b/frontend/src/utils/api.ts
index ddb8674..c357d9c 100644
--- a/frontend/src/utils/api.ts
+++ b/frontend/src/utils/api.ts
@@ -28,23 +28,18 @@ async function call<T>(endpoint: string, opts: RequestInit = {}) {
   return res.json() as Promise<T>;
 }
 
-export const MIN_VOUCHER_DURATION_MINUTES = 1;
-export const MAX_VOUCHER_DURATION_MINUTES = 525_600;
+// The controller only accepts whole hours, days or weeks.
+export const MAX_VOUCHER_DURATION_HOURS = 8_760;
 
+// Unleashed creates at most 100 passes in one batch.
 export const MIN_VOUCHER_COUNT = 1;
-export const MAX_VOUCHER_COUNT = 1000;
+export const MAX_VOUCHER_COUNT = 100;
 
 export const MIN_VOUCHER_GUESTS = 1;
 export const MAX_VOUCHER_GUESTS = 1000;
 
-export const MIN_VOUCHER_DATA_MB = 1;
-export const MAX_VOUCHER_DATA_MB = 1_048_576;
-
-export const MIN_VOUCHER_DOWNLOAD_KBPS = 2;
-export const MAX_VOUCHER_DOWNLOAD_KBPS = 100_000;
-
-export const MIN_VOUCHER_UPLOAD_KBPS = 2;
-export const MAX_VOUCHER_UPLOAD_KBPS = 100_000;
+export const MIN_VOUCHER_KEY_LENGTH = 2;
+export const MAX_VOUCHER_KEY_LENGTH = 16;
 
 export const api = {
   getAllVouchers: () => call<VoucherGetResponse>("/vouchers"),
````

- [ ] **Step 3: Rewrite Custom Create**

Durations in hours, days or weeks; no data or rate inputs; optional key and remarks; the name and key are disabled for batches because the controller names batch vouchers and generates their keys. Disabled inputs are not in `FormData`, so the name falls back to `""`.

`frontend/src/components/tabs/CustomCreateTab.tsx`:

````tsx
"use client";

import SuccessModal from "@/components/modals/SuccessModal";
import { Voucher, VoucherCreateData } from "@/types/voucher";
import {
  api,
  MAX_VOUCHER_COUNT,
  MAX_VOUCHER_DURATION_HOURS,
  MAX_VOUCHER_GUESTS,
  MAX_VOUCHER_KEY_LENGTH,
  MIN_VOUCHER_COUNT,
  MIN_VOUCHER_GUESTS,
  MIN_VOUCHER_KEY_LENGTH,
} from "@/utils/api";
import { map } from "@/utils/functional";
import { notify } from "@/utils/notifications";
import { useCallback, useState, SubmitEvent } from "react";

// The controller only accepts whole hours, days or weeks.
type TimeUnit = "hours" | "days" | "weeks";

const HOURS_PER_UNIT: Record<TimeUnit, number> = {
  hours: 1,
  days: 24,
  weeks: 168,
};

export default function CustomCreateTab() {
  const [loading, setLoading] = useState(false);
  const [newVouchers, setNewVouchers] = useState<Voucher[] | null>(null);
  const [durationUnit, setDurationUnit] = useState<TimeUnit>("hours");
  const [count, setCount] = useState(MIN_VOUCHER_COUNT);

  const handleSubmit = async (e: SubmitEvent) => {
    e.preventDefault();
    setLoading(true);

    const parseNumber = (x: FormDataEntryValue) =>
      x !== "" ? Number(x) : null;
    const parseText = (x: FormDataEntryValue) =>
      String(x).trim() !== "" ? String(x).trim() : null;

    const form = e.currentTarget as HTMLFormElement;
    const data = new FormData(form);

    const rawDuration = Number(data.get("duration"));
    const unit = String(data.get("durationUnit") || "hours") as TimeUnit;

    if (!Number.isInteger(rawDuration) || rawDuration <= 0) {
      notify("Duration must be a whole number above zero", "error");
      setLoading(false);
      return;
    }

    const durationHours = rawDuration * HOURS_PER_UNIT[unit];
    if (durationHours > MAX_VOUCHER_DURATION_HOURS) {
      notify(
        `Duration too long. Maximum allowed is ${MAX_VOUCHER_DURATION_HOURS} hours`,
        "error",
      );
      setLoading(false);
      return;
    }

    const payload: VoucherCreateData = {
      count: Number(data.get("count")),
      name: String(data.get("name") ?? ""),
      timeLimitMinutes: durationHours * 60,
      authorizedGuestLimit: map(data.get("guests"), parseNumber),
      code: map(data.get("code"), parseText),
      remarks: map(data.get("remarks"), parseText),
    };

    try {
      const res = await api.createVoucher(payload);
      setNewVouchers(res.vouchers);
      notify(`Successfully created ${res.vouchers.length} vouchers`, "success");
      form.reset();
      setCount(MIN_VOUCHER_COUNT);
    } catch (error: any) {
      if (error?.status === 409) {
        notify("That key is already in use", "error");
      } else if (error?.status === 400) {
        notify("The controller cannot create that voucher", "error");
      } else {
        notify("Failed to create voucher", "error");
      }
    }
    setLoading(false);
  };

  const closeModal = useCallback(() => {
    setNewVouchers(null);
  }, []);

  const isBatch = count > 1;

  return (
    <div>
      <form onSubmit={handleSubmit} className="card max-w-lg mx-auto space-y-6">
        <div>
          <label className="block font-medium mb-1">Number</label>
          <input
            name="count"
            type="number"
            required
            min={MIN_VOUCHER_COUNT}
            max={MAX_VOUCHER_COUNT}
            value={count}
            onChange={(e) => setCount(Number(e.target.value) || 0)}
          />
        </div>

        <div>
          <label className="block font-medium mb-1">Name</label>
          <input
            name="name"
            type="text"
            required={!isBatch}
            disabled={isBatch}
            defaultValue="Custom Voucher"
          />
          {isBatch && (
            <p className="text-sm text-secondary mt-1">
              The controller names batch vouchers itself (Guest-1, Guest-2,
              ...).
            </p>
          )}
        </div>

        <div>
          <label className="block font-medium mb-1">Duration</label>
          <div className="flex-center gap-2">
            <input
              name="duration"
              type="number"
              required
              min={1}
              step={1}
              max={Math.floor(
                MAX_VOUCHER_DURATION_HOURS / HOURS_PER_UNIT[durationUnit],
              )}
              defaultValue={24}
            />
            <select
              name="durationUnit"
              onChange={(e) => setDurationUnit(e.target.value as TimeUnit)}
              className="w-auto"
              defaultValue="hours"
            >
              <option value="hours">Hours</option>
              <option value="days">Days</option>
              <option value="weeks">Weeks</option>
            </select>
          </div>
        </div>

        <div>
          <label className="block font-medium mb-1">Guest Limit</label>
          <input
            name="guests"
            type="number"
            min={MIN_VOUCHER_GUESTS}
            max={MAX_VOUCHER_GUESTS}
            placeholder="Unlimited"
          />
        </div>

        <div>
          <label className="block font-medium mb-1">Key</label>
          <input
            name="code"
            type="text"
            disabled={isBatch}
            minLength={MIN_VOUCHER_KEY_LENGTH}
            maxLength={MAX_VOUCHER_KEY_LENGTH}
            pattern={"[^\\s#&+\"'<>,]+"}
            title={`${MIN_VOUCHER_KEY_LENGTH} to ${MAX_VOUCHER_KEY_LENGTH} characters, no spaces or # & + " ' < > ,`}
            placeholder={isBatch ? "Generated per voucher" : "Generated"}
          />
        </div>

        <div>
          <label className="block font-medium mb-1">Remarks</label>
          <input name="remarks" type="text" placeholder="None" />
        </div>

        <button type="submit" disabled={loading} className="btn-primary w-full">
          {loading ? "Creating…" : "Create Custom Voucher"}
        </button>
      </form>
      {newVouchers && (
        <SuccessModal vouchers={newVouchers} onClose={closeModal} />
      )}
    </div>
  );
}
````

- [ ] **Step 4: Remove the data and rate fields everywhere else, and show remarks**

````diff
diff --git a/frontend/src/types/print.ts b/frontend/src/types/print.ts
index acb61f3..08ebb3e 100644
--- a/frontend/src/types/print.ts
+++ b/frontend/src/types/print.ts
@@ -12,9 +12,6 @@ export type PrintConfig = {
   showLogo: boolean;
   showDuration: boolean;
   showMaxGuests: boolean;
-  showDataUsageLimit: boolean;
-  showRxRateLimit: boolean;
-  showTxRateLimit: boolean;
   showId: boolean;
   showPrintTime: boolean;
 };
````

````diff
diff --git a/frontend/src/types/config.ts b/frontend/src/types/config.ts
index 1a655b4..af667f6 100644
--- a/frontend/src/types/config.ts
+++ b/frontend/src/types/config.ts
@@ -15,9 +15,6 @@ export const DEFAULT_RUNTIME_CONFIG: RuntimeConfig = {
     showLogo: true,
     showDuration: true,
     showMaxGuests: true,
-    showDataUsageLimit: true,
-    showRxRateLimit: true,
-    showTxRateLimit: true,
     showId: true,
     showPrintTime: true,
   },
````

````diff
diff --git a/frontend/src/utils/format.ts b/frontend/src/utils/format.ts
index ed3ca03..31d8032 100644
--- a/frontend/src/utils/format.ts
+++ b/frontend/src/utils/format.ts
@@ -31,25 +31,6 @@ export function formatDuration(m: number | null | undefined) {
   );
 }
 
-export function formatBytes(b: number | null | undefined) {
-  if (!b) return "Unlimited";
-  const units = ["B", "KB", "MB", "GB", "TB"];
-  let size = b,
-    i = 0;
-  while (size >= 1024 && i < units.length - 1) {
-    size /= 1024;
-    i++;
-  }
-  return `${size.toFixed(size < 10 ? 1 : 0)} ${units[i]}`;
-}
-
-export function formatSpeed(kbps: number | null | undefined) {
-  if (!kbps) return "Unlimited";
-  return kbps >= 1024
-    ? `${(kbps / 1024).toFixed(kbps < 10240 ? 1 : 0)} Mbps`
-    : `${kbps} Kbps`;
-}
-
 export function formatGuestUsage(
   usage: number,
   limit: number | null | undefined,
````

````diff
diff --git a/frontend/src/app/print/page.tsx b/frontend/src/app/print/page.tsx
index 93a1304..53d3512 100644
--- a/frontend/src/app/print/page.tsx
+++ b/frontend/src/app/print/page.tsx
@@ -4,12 +4,7 @@ import "./styles.css";
 import { useRouter, useSearchParams } from "next/navigation";
 import { Suspense, useEffect, useState } from "react";
 import { Voucher } from "@/types/voucher";
-import {
-  formatBytes,
-  formatDuration,
-  formatMaxGuests,
-  formatSpeed,
-} from "@/utils/format";
+import { formatDuration, formatMaxGuests } from "@/utils/format";
 import { useGlobal } from "@/contexts/GlobalContext";
 import { formatCode } from "@/utils/format";
 import Spinner from "@/components/utils/Spinner";
@@ -33,23 +28,6 @@ function VoucherPrintCard({ voucher }: { voucher: Voucher }) {
       value: formatMaxGuests(voucher.authorizedGuestLimit),
       enabled: printConfig.showMaxGuests,
     },
-    {
-      label: "Data Limit",
-      value: voucher.dataUsageLimitMBytes
-        ? formatBytes(voucher.dataUsageLimitMBytes * 1024 * 1024)
-        : "Unlimited",
-      enabled: printConfig.showDataUsageLimit,
-    },
-    {
-      label: "Down Speed",
-      value: formatSpeed(voucher.rxRateLimitKbps),
-      enabled: printConfig.showRxRateLimit,
-    },
-    {
-      label: "Up Speed",
-      value: formatSpeed(voucher.txRateLimitKbps),
-      enabled: printConfig.showTxRateLimit,
-    },
   ];
 
   return (
````

````diff
diff --git a/frontend/src/components/modals/VoucherModal.tsx b/frontend/src/components/modals/VoucherModal.tsx
index 1395e11..616e7a8 100644
--- a/frontend/src/components/modals/VoucherModal.tsx
+++ b/frontend/src/components/modals/VoucherModal.tsx
@@ -5,10 +5,8 @@ import Spinner from "@/components/utils/Spinner";
 import { api } from "@/utils/api";
 import { useCallback, useEffect, useRef, useState } from "react";
 import {
-  formatBytes,
   formatDuration,
   formatGuestUsage,
-  formatSpeed,
   formatStatus,
 } from "@/utils/format";
 import VoucherCode from "@/components/utils/VoucherCode";
@@ -79,14 +77,7 @@ export default function VoucherModal({ voucher, onClose }: Props) {
                     details.authorizedGuestLimit,
                   ),
                 ],
-                [
-                  "Data Limit",
-                  details.dataUsageLimitMBytes
-                    ? formatBytes(details.dataUsageLimitMBytes * 1024 * 1024)
-                    : "Unlimited",
-                ],
-                ["Download Speed", formatSpeed(details.rxRateLimitKbps)],
-                ["Upload Speed", formatSpeed(details.txRateLimitKbps)],
+                ...(details.remarks ? [["Remarks", details.remarks]] : []),
                 ["ID", details.id],
               ] as [string, any][]
             ).map(([label, value]) => (
````

````diff
diff --git a/frontend/src/components/VoucherCard.tsx b/frontend/src/components/VoucherCard.tsx
index 2bb2d0f..e0f9879 100644
--- a/frontend/src/components/VoucherCard.tsx
+++ b/frontend/src/components/VoucherCard.tsx
@@ -47,6 +47,11 @@ const VoucherCard = ({ voucher, selected, editMode, onClick }: Props) => {
       <div className="mb-2">
         <div className="text-xl voucher-code">{formatCode(voucher.code)}</div>
         <div className="text-lg font-semibold truncate">{voucher.name}</div>
+        {voucher.remarks && (
+          <div className="text-sm text-secondary truncate">
+            {voucher.remarks}
+          </div>
+        )}
       </div>
 
       <div className="space-y-1 text-sm text-secondary">
````

````diff
diff --git a/frontend/src/components/tabs/TestTab.tsx b/frontend/src/components/tabs/TestTab.tsx
index 236065a..3097a29 100644
--- a/frontend/src/components/tabs/TestTab.tsx
+++ b/frontend/src/components/tabs/TestTab.tsx
@@ -33,9 +33,7 @@ export default function TestTab() {
     timeLimitMinutes: 1440,
     activatedAt: null,
     expiresAt: "2025-12-31",
-    dataUsageLimitMBytes: null,
-    rxRateLimitKbps: null,
-    txRateLimitKbps: null,
+    remarks: "Test remarks",
   };
 
   return (
@@ -78,6 +76,7 @@ export default function TestTab() {
               authorizedGuestCount: 0,
               expired: false,
               timeLimitMinutes: 1440,
+              remarks: "",
             }}
             editMode={false}
             selected={false}
@@ -93,6 +92,7 @@ export default function TestTab() {
               authorizedGuestCount: 0,
               expired: false,
               timeLimitMinutes: 1440,
+              remarks: "",
             }}
             editMode={true}
             selected={true}
@@ -108,6 +108,7 @@ export default function TestTab() {
               authorizedGuestCount: 1,
               expired: true,
               timeLimitMinutes: 1440,
+              remarks: "",
               expiresAt: "2025-12-31",
             }}
             editMode={true}
````

- [ ] **Step 5: Rebrand**

````diff
diff --git a/frontend/src/app/layout.tsx b/frontend/src/app/layout.tsx
index 637b179..2d981bd 100644
--- a/frontend/src/app/layout.tsx
+++ b/frontend/src/app/layout.tsx
@@ -3,8 +3,8 @@ import "./globals.css";
 import type { Metadata } from "next";
 
 export const metadata: Metadata = {
-  title: "UniFi Voucher Manager",
-  description: "Manage WiFi vouchers with ease",
+  title: "Unleashed Voucher Manager",
+  description: "Manage Ruckus Unleashed guest passes with ease",
   authors: [{ name: "etiennecollin", url: "https://etiennecollin.com" }],
   creator: "Etienne Collin",
   robots: {
````

````diff
diff --git a/frontend/src/components/Header.tsx b/frontend/src/components/Header.tsx
index 09dc89c..8d12a1f 100644
--- a/frontend/src/components/Header.tsx
+++ b/frontend/src/components/Header.tsx
@@ -46,12 +46,12 @@ export default function Header() {
             width={35}
             height={35}
             loading="eager"
-            alt="UniFi Voucher Manager logo"
+            alt="Unleashed Voucher Manager logo"
             className={"shrink-0" + (isLogoInvertible ? " dark:invert" : "")}
           />
           <h1 className="text-xl md:text-2xl font-semibold text-brand">
             <span className="block sm:hidden">UVM</span>
-            <span className="hidden sm:block">UniFi Voucher Manager</span>
+            <span className="hidden sm:block">Unleashed Voucher Manager</span>
           </h1>
         </div>
         <div className="flex-center gap-3">
````

````diff
diff --git a/frontend/package.json b/frontend/package.json
index 5bee692..2f484a2 100644
--- a/frontend/package.json
+++ b/frontend/package.json
@@ -1,5 +1,5 @@
 {
-  "name": "unifi-voucher-manager",
+  "name": "unleashed-voucher-manager",
   "version": "0.0.0-git",
   "private": true,
   "scripts": {
````

````diff
diff --git a/frontend/package-lock.json b/frontend/package-lock.json
index 4174e2b..a252191 100644
--- a/frontend/package-lock.json
+++ b/frontend/package-lock.json
@@ -1,11 +1,11 @@
 {
-  "name": "unifi-voucher-manager",
+  "name": "unleashed-voucher-manager",
   "version": "0.0.0-git",
   "lockfileVersion": 3,
   "requires": true,
   "packages": {
     "": {
-      "name": "unifi-voucher-manager",
+      "name": "unleashed-voucher-manager",
       "version": "0.0.0-git",
       "dependencies": {
         "next": "16.2.12",
````

Replace the UniFi mark with a neutral Wi-Fi logo, same 32x32 rounded square so the header and QR layout are unchanged. `frontend/public/logo.svg`:

````xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  <rect width="32" height="32" rx="8" fill="url(#a)"/>
  <g fill="none" stroke="#fff" stroke-width="2.5" stroke-linecap="round">
    <path d="M7 13.5a12.5 12.5 0 0 1 18 0"/>
    <path d="M10.5 17.25a7.5 7.5 0 0 1 11 0"/>
  </g>
  <circle cx="16" cy="22" r="2.25" fill="#fff"/>
  <defs>
    <radialGradient id="a" cx="0" cy="0" r="1" gradientTransform="matrix(0 32 -32 0 16 0)" gradientUnits="userSpaceOnUse">
      <stop stop-color="#2A9D8F"/>
      <stop offset="1" stop-color="#1D6F65"/>
    </radialGradient>
  </defs>
</svg>
````

- [ ] **Step 6: Typecheck and build**

Run: `cd frontend && npx tsc --noEmit && npm run build`
Expected: `TypeScript: No errors found`, then a build listing `/`, `/kiosk`, `/print`, `/welcome`.

Run: `grep -rn -i -e unifi -e dataUsage -e RateLimit frontend/src frontend/package.json`
Expected: no output.

- [ ] **Step 7: Commit**

```bash
git add frontend
git commit -m "feat(frontend)!: adapt voucher creation and display to Unleashed"
```

---

### Task 7: Kiosk refresh and forwarded address

**Files:**
- Modify: `frontend/src/app/kiosk/page.tsx`
- Modify: `frontend/src/proxy.ts`

**Interfaces:**
- Consumes: `GET /rust-api/vouchers/rolling` (`404` when there is none) and `POST /rust-api/vouchers/rolling`, unchanged.
- Produces: nothing new for other tasks.

An unused rolling voucher keeps counting down on the controller and disappears when it expires, so the kiosk re-checks every minute without showing the spinner; when none is found it creates one, as it already does on load. The proxy takes the first `X-Forwarded-For` entry, matching the backend's `first_forwarded_ip`.

- [ ] **Step 1: Apply the kiosk change**

````diff
diff --git a/frontend/src/app/kiosk/page.tsx b/frontend/src/app/kiosk/page.tsx
index 7bc2a50..65644e7 100644
--- a/frontend/src/app/kiosk/page.tsx
+++ b/frontend/src/app/kiosk/page.tsx
@@ -9,15 +9,19 @@ import { api } from "@/utils/api";
 import { formatCode } from "@/utils/format";
 import { useGlobal } from "@/contexts/GlobalContext";
 
+// An unused voucher still counts down on the controller, which removes it
+// when it expires. Re-check regularly so the kiosk never shows a dead code.
+const KIOSK_REFRESH_MS = 60_000;
+
 export default function KioskPage() {
   const [voucher, setVoucher] = useState<Voucher | null>(null);
   const [state, setState] = useState<TriState | null>(null);
   const { wifiConfig, wifiString } = useGlobal();
 
-  const load = useCallback(async () => {
+  const load = useCallback(async (silent: boolean = false) => {
     if (state === "loading") return;
     try {
-      setState("loading");
+      if (!silent) setState("loading");
       await api.getRollingVoucher().then(setVoucher);
       setState("ok");
     } catch (error: any) {
@@ -35,9 +39,14 @@ export default function KioskPage() {
   }, []);
 
   useEffect(() => {
+    const refresh = () => load(true);
     load();
-    window.addEventListener("vouchersUpdated", load);
-    return () => window.removeEventListener("vouchersUpdated", load);
+    const timer = setInterval(refresh, KIOSK_REFRESH_MS);
+    window.addEventListener("vouchersUpdated", refresh);
+    return () => {
+      clearInterval(timer);
+      window.removeEventListener("vouchersUpdated", refresh);
+    };
   }, [load]);
 
   const renderContent = useCallback(() => {
````

- [ ] **Step 2: Apply the proxy change**

````diff
diff --git a/frontend/src/proxy.ts b/frontend/src/proxy.ts
index b272b81..0e9439f 100644
--- a/frontend/src/proxy.ts
+++ b/frontend/src/proxy.ts
@@ -20,8 +20,10 @@ const guestAllowedPaths = [
 export function proxy(request: NextRequest) {
   const { pathname } = request.nextUrl;
 
-  // Extract client IP
-  let clientIp = request.headers.get("x-forwarded-for") || "";
+  // Extract client IP: the first entry when proxies appended their own
+  let clientIp = (request.headers.get("x-forwarded-for") || "")
+    .split(",")[0]
+    .trim();
 
   // Strip IPv6 prefix if it's a mapped IPv4
   if (clientIp.startsWith(IPV6_IPV4_MAPPED_PREFIX)) {
````

- [ ] **Step 3: Typecheck and build**

Run: `cd frontend && npx tsc --noEmit && npm run build`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/app/kiosk/page.tsx frontend/src/proxy.ts
git commit -m "feat(frontend): refresh the kiosk and use the first forwarded address"
```

---

### Task 8: Container, CI and release

**Files:**
- Modify: `compose.yaml` (full replacement)
- Create: `docker-bake.hcl`
- Create: `.forgejo/workflows/ci.yaml`
- Create: `.forgejo/workflows/release.yaml`
- Create: `.renovaterc.json5`

`Dockerfile` and `scripts/*` need no change: they copy `backend/src` and run whatever binary and frontend are built, and `entrypoint.sh` already passes `PRINT_CONFIG` through.

**Interfaces:**
- Consumes: the repository builds from Tasks 5 to 7.
- Produces: `docker buildx bake image-local` builds `unleashed-voucher-manager:local`; pushing a `v*` tag publishes `ghcr.io/greyrock-labs/unleashed-voucher-manager`. The release job needs Forgejo secrets `GHCR_USERNAME` and `GHCR_TOKEN` (a GitHub classic PAT with `write:packages`).

- [ ] **Step 1: Write the files**

`compose.yaml`:

````yaml
---
services:
  unleashed-voucher-manager:
    image: "ghcr.io/greyrock-labs/unleashed-voucher-manager:latest"

    # To build the image yourself
    # build:
    #   context: "./"
    #   target: "runtime"
    #   dockerfile: "./Dockerfile"

    container_name: "unleashed-voucher-manager"
    restart: "unless-stopped"
    ports:
      - "3000:3000"

    # Display your custom SVG logo. The mount destination CANNOT BE CHANGED.
    # volumes:
    #   - "./assets/my_logo.svg:/app/frontend/public/logo.svg"

    # SEE README FOR ENVIRONMENT VARIABLES DOCUMENTATION
    environment:
      UNLEASHED_URL: "URL of your Unleashed controller with protocol (`https://`)."
      UNLEASHED_USERNAME: "Controller user whose role has read-write admin privilege."
      UNLEASHED_PASSWORD: "Password for that user."
      UNLEASHED_SSID: "The guest WLAN that vouchers are created on."
      UNLEASHED_HAS_VALID_CERT: "true" # Set to false only if the controller serves a self-signed certificate.
      WIFI_SSID: "Your guest WiFi SSID" # Optional, but recommended
      WIFI_PASSWORD: "Your guest WiFi password" # Optional, but recommended
      GUEST_SUBNETWORK: "Your guest subnetwork in IPv4 CIDR notation (X.X.X.X/X)" # Optional, but recommended
      TIMEZONE: "Your timezone (America/Toronto)" # Optional, but recommended
````

`docker-bake.hcl`:

````hcl
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
  context  = "."
  target   = "runtime"
  // GHCR links a package to its repository from the manifest annotation,
  // not the config label.
  annotations = [
    "index,manifest:org.opencontainers.image.source=https://github.com/greyrock-labs/unleashed-voucher-manager"
  ]
}

target "image-local" {
  inherits = ["image"]
  output   = ["type=docker"]
}

target "image-all" {
  inherits = ["image"]
  // amd64 only: the Forgejo runner cannot mount binfmt_misc, so QEMU
  // emulation is unavailable there. Do not add setup-qemu-action.
  platforms = [
    "linux/amd64"
  ]
}
````

`.forgejo/workflows/ci.yaml`:

````yaml
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
````

`.forgejo/workflows/release.yaml`:

````yaml
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
          # Must be the GitHub URL: this is what links the package to the
          # repository.
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
````

`.renovaterc.json5`:

````json5
{
  $schema: "https://docs.renovatebot.com/renovate-schema.json",
  extends: [
    "local>todd/renovate-config"
  ]
}
````

- [ ] **Step 2: Check the bake definition**

Run: `docker buildx bake --print image-local`
Expected: JSON with target `runtime`, tag `unleashed-voucher-manager:local` and the `org.opencontainers.image.source` annotation.

- [ ] **Step 3: Build the image**

Run: `docker buildx bake image-local`
Expected: the build finishes and `docker image ls unleashed-voucher-manager:local` lists it.

- [ ] **Step 4: Commit**

```bash
git add compose.yaml docker-bake.hcl .forgejo .renovaterc.json5
git commit -m "ci: build, test and publish the image from Forgejo"
```

---

### Task 9: Documentation and release skill

**Files:**
- Modify: `README.md` (full replacement)
- Modify: `LICENSE`
- Create: `.agents/skills/cutting-a-release/SKILL.md`

**Interfaces:**
- Consumes: the configuration, behaviour and release workflow from Tasks 5 to 8.
- Produces: nothing for other tasks.

- [ ] **Step 1: Write the README**

`README.md`:

````markdown
# Unleashed Voucher Manager

A touch-friendly web application for managing guest passes on Ruckus Unleashed
controllers: create them, print them, show the current one on a kiosk, and
roll a new one automatically each time a guest connects.

It is a port of
[unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager)
by Etienne Collin to Ruckus Unleashed, and keeps its interface and features
wherever the controller allows.

<!-- vim-markdown-toc GFM -->

- [Features](#features)
- [Quick start](#quick-start)
  - [Docker Compose](#docker-compose)
  - [Without Docker](#without-docker)
- [Configuration](#configuration)
  - [Controller account](#controller-account)
  - [Rolling vouchers and the kiosk page](#rolling-vouchers-and-the-kiosk-page)
  - [Custom SVG logo](#custom-svg-logo)
  - [Environment variables](#environment-variables)
- [Differences from UniFi Voucher Manager](#differences-from-unifi-voucher-manager)
- [Troubleshooting](#troubleshooting)
- [Credits](#credits)

<!-- vim-markdown-toc -->

## Features

- **Quick Create**: a voucher with a preset duration, from 1 hour to 30 days.
- **Custom Create**: choose the number of vouchers (up to 100 at once), name,
  duration in hours, days or weeks, guest limit, a custom key and remarks.
- **Browse vouchers**: search by name, view details, select and delete in
  bulk, delete expired vouchers.
- **Print vouchers**: list or grid layout, friendly to thermal printers.
- **WiFi QR code**: lets guests join the network by scanning.
- **Rolling vouchers**: a new voucher is created for the next guest whenever
  the current one is used.
- **Kiosk page** (`/kiosk`): the current rolling voucher and the WiFi QR code,
  updated live.
- **Interface**: touch-friendly, dark and light mode, notifications, custom
  logo.
- **Architecture**: a Next.js frontend and an Axum (Rust) backend in one
  container. Only the backend talks to the controller, so the controller
  credentials never reach the browser.

## Quick start

### Docker Compose

1. Download `compose.yaml` from this repository.
2. Set the environment variables in it (see
   [Environment variables](#environment-variables)).
3. Start it:

   ```bash
   docker compose up -d --force-recreate
   ```

4. Open `http://localhost:3000`.

### Without Docker

1. Install `rust >= 1.88.0`, `nodejs >= 24.3.0` and `npm >= 11.4.2`.
2. Clone this repository.
3. Set the environment variables in your shell, or in a `.env` file at the
   repository root and use the backend's `dotenv` feature.
4. Start the backend and frontend:

   ```bash
   # Backend (without a .env file)
   cd backend && cargo run --release

   # Backend (with a .env file)
   cd backend && cargo run --release --features dotenv

   # Frontend (development)
   cd frontend && npm install && npm run dev

   # Frontend (release)
   cd frontend && npm ci && npm run build && npm run start
   ```

5. Open `http://localhost:3000`.

## Configuration

### Controller account

The app logs in to the controller's admin interface with a username and
password. The account needs a role with **read-write admin privilege**.
Unleashed's built-in Guest Pass Manager role can list and create guest passes
but cannot delete them, so delete, cleanup and rolling vouchers would not
work with it.

In the Unleashed web interface, create a role with read-write admin
privilege, then a user in that role, and give that user's credentials to the
app.

### Rolling vouchers and the kiosk page

Rolling vouchers give each guest a fresh code: when a guest uses the current
rolling voucher, the app creates the next one, and the kiosk page shows it.

> [!IMPORTANT]
> Rolling vouchers need the guest WLAN to send guests to the app after they
> log in. In the Unleashed web interface, open the guest WLAN's guest access
> (captive portal) settings and set **Redirect to the following URL** to the
> app's `/welcome` page, for example `https://voucher.example.com/welcome`.
>
> Without this, vouchers do not roll when guests connect.

> [!CAUTION]
> Set `GUEST_SUBNETWORK` to the guest network's subnet. Guests on it can then
> reach only `/welcome`, not the voucher management pages. Without it, guests
> can create and delete vouchers themselves.

How it works:

1. The kiosk page shows the current rolling voucher, creating one if none
   exists.
2. A guest logs in with it and the captive portal sends them to `/welcome`.
3. `/welcome` asks the backend to create the next rolling voucher. Each IP
   address can create one at a time, so reloading the page does not create
   more.
4. Rolling vouchers are named `[ROLLING]-<timestamp>-<ip>`.
5. When the guest portal counts a pass's time from when it was issued, an
   unused rolling voucher can expire on the kiosk. The controller then
   removes it, and the kiosk, which re-checks every minute, creates a new
   one.
6. At midnight (in `TIMEZONE`) expired rolling vouchers are deleted, or all
   expired vouchers if `PURGE_ALL_EXPIRED_VOUCHERS` is set. Unleashed usually
   removes expired passes itself, so this rarely finds anything.

The reverse proxy in front of the app must pass the client's real address in
`X-Forwarded-For`. Both `GUEST_SUBNETWORK` and the one-voucher-per-IP rule
depend on it.

### Custom SVG logo

- With Docker, mount the SVG at `/app/frontend/public/logo.svg`. There is an
  example in `compose.yaml`. The mount destination, including the file name,
  **cannot be changed**.
- Without Docker, place the SVG at `./frontend/public/logo.svg`.

### Environment variables

Required variables come first.

- **`UNLEASHED_URL`: `string`** (_Required_)
  - URL of the Unleashed controller, with protocol.
  - Example: `https://unleashed.example.com`
- **`UNLEASHED_USERNAME`: `string`** (_Required_)
  - A controller user whose role has read-write admin privilege. See
    [Controller account](#controller-account).
- **`UNLEASHED_PASSWORD`: `string`** (_Required_)
  - That user's password.
- **`UNLEASHED_SSID`: `string`** (_Required_)
  - The guest WLAN that vouchers are created on, as named on the controller.
  - Example: `Guest WiFi`
- **`UNLEASHED_HAS_VALID_CERT`: `bool`** (_Optional_)
  - Whether the controller's certificate is trusted. Set to `false` if it
    serves its default self-signed certificate.
  - Default: `true`
- **`GUEST_SUBNETWORK`: `IPv4 CIDR`** (_Optional_)
  - Guests on this subnet can reach only `/welcome`. See
    [Rolling vouchers and the kiosk page](#rolling-vouchers-and-the-kiosk-page).
  - Example: `10.0.5.0/24`
- **`TIMEZONE`: [timezone identifier](https://en.wikipedia.org/wiki/List_of_tz_database_time_zones#List)**
  (_Optional_)
  - Used to show dates and to schedule the midnight cleanup.
  - Default: `UTC`
- **`ROLLING_VOUCHER_DURATION_MINUTES`: `minutes`** (_Optional_)
  - How long a rolling voucher grants access. Rounded up to whole hours,
    because the controller only accepts hours, days and weeks.
  - Default: `480`
- **`PURGE_ALL_EXPIRED_VOUCHERS`: `bool`** (_Optional_)
  - When `true`, the midnight cleanup deletes all expired vouchers; when
    `false`, only expired rolling vouchers.
  - Default: `false`
- **`WIFI_SSID`: `string`** (_Optional_)
  - SSID for the QR code. The QR code needs both `WIFI_SSID` and
    `WIFI_PASSWORD`.
- **`WIFI_PASSWORD`: `string`** (_Optional_)
  - Password for the QR code. Set to `""` for an open network.
- **`WIFI_TYPE`: `WPA|WEP|nopass`** (_Optional_)
  - Security type for the QR code. Defaults to `WPA` with a password and
    `nopass` without.
- **`WIFI_HIDDEN`: `bool`** (_Optional_)
  - Whether the SSID is hidden.
  - Default: `false`
- **`IS_LOGO_INVERTIBLE`: `bool`** (_Optional_)
  - Whether the logo may be inverted in dark mode.
  - Default: `false`
- **`PRINT_CONFIG`: `JSON object`** (_Optional_)
  - Which fields printed vouchers show. Omitted fields default to `true`.
  - Default:
    `{"showLogo":true,"showDuration":true,"showMaxGuests":true,"showId":true,"showPrintTime":true}`
- **`FRONTEND_BIND_HOST`: `IPv4`** (_Optional_), default `0.0.0.0`
- **`FRONTEND_BIND_PORT`: `u16`** (_Optional_), default `3000`
- **`FRONTEND_TO_BACKEND_URL`: `URL`** (_Optional_), default
  `http://127.0.0.1`
- **`BACKEND_BIND_HOST`: `IPv4`** (_Optional_), default `127.0.0.1`
- **`BACKEND_BIND_PORT`: `u16`** (_Optional_), default `8080`
- **`BACKEND_LOG_LEVEL`: `trace|debug|info|warn|error`** (_Optional_),
  default `info`

## Differences from UniFi Voucher Manager

- **No data or speed limits per voucher.** Unleashed sets bandwidth per WLAN
  or role, not per pass.
- **Durations in whole hours, days or weeks.** The controller cannot create a
  pass shorter than an hour; other values are rounded up to whole hours.
- **Batches of up to 100**, named by the controller (`Guest-1`, `Guest-2`,
  ...).
- **Custom keys and remarks** can be set when creating a voucher.
- **Usernames and passwords** instead of an API key, and an SSID instead of a
  site ID.

## Troubleshooting

- **The backend keeps retrying the connection at startup**
  - Check `UNLEASHED_URL` is correct and reachable from the container.
  - Check `UNLEASHED_HAS_VALID_CERT` matches the controller's certificate.
  - Check the username and password, and that the role has read-write admin
    privilege.
- **Creating a voucher fails**
  - Check `UNLEASHED_SSID` names an existing guest WLAN.
  - A custom key must be 2 to 16 characters, without spaces or
    `# & + " ' < > ,`, and not already in use.
- **Vouchers do not roll when guests connect**
  - Check the guest WLAN redirects to the app's `/welcome` page.
  - Check the reverse proxy passes `X-Forwarded-For`.
- **The WiFi QR code button is disabled**
  - Set `WIFI_SSID` and `WIFI_PASSWORD`, and check the browser console for
    configuration errors.

For more detail, run with `BACKEND_LOG_LEVEL="debug"` and check
`docker logs unleashed-voucher-manager`.

## Credits

Based on [unifi-voucher-manager](https://github.com/etiennecollin/unifi-voucher-manager)
by Etienne Collin, under the MIT License. The Unleashed protocol details were
worked out with help from [FetchPass](https://github.com/fmuffat/FetchPass)
and [aioruckus](https://github.com/ms264556/aioruckus).

This is an independent, unofficial project. It is not affiliated with,
endorsed by, or sponsored by Ruckus Networks or Ubiquiti Inc. Ruckus,
Unleashed, UniFi and all associated trademarks are the property of their
respective owners.
````

- [ ] **Step 2: Add the copyright line**

````diff
diff --git a/LICENSE b/LICENSE
index a60f276..74b69ff 100644
--- a/LICENSE
+++ b/LICENSE
@@ -1,6 +1,7 @@
 MIT License
 
 Copyright (c) 2025 Etienne Collin
+Copyright (c) 2026 Todd Punderson
 
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
````

- [ ] **Step 3: Write the release skill**

`.agents/skills/cutting-a-release/SKILL.md`:

````markdown
---
name: cutting-a-release
description: 'Use when releasing this repo: "cut a release", "tag a release", "ship it", "send it", "publish", "bump the version", or any request to get merged work onto GHCR. Covers committing to main, choosing the version, writing the signed annotated tag, and pushing it to fire the Forgejo release workflow.'
---

# Cutting a release

## Core principle

**The git tag is the only version input.** Nothing in the tree carries a real
version: `backend/Cargo.toml` and `frontend/package.json` both stay
`0.0.0-git`, and `docker/metadata-action` takes the image tags from the git
tag. There is nothing to bump before tagging; the tag is the bump.

## Standing preferences

- **Commit straight to `main`.** No feature branch or pull request unless
  asked.
- **"Send it" or "ship it" means commit, push `main`, and tag a release**, all
  in one go. Do not stop after the push and offer the tag as a next step.

## Procedure

```bash
# 1. Preflight: every one of these must pass before tagging
git status --short                      # clean tree
git rev-parse HEAD origin/main          # identical; push main first if not
(cd backend && cargo test --locked)     # what the release job runs
(cd frontend && npx tsc --noEmit && npm run build)

# 2. What is going into this release
git describe --tags --abbrev=0          # last tag
git log --oneline "$(git describe --tags --abbrev=0)"..main

# 3. Tag: annotated and signed, message from a heredoc
git tag -s v1.2.3 -F - <<'MSG'
...message, see the template below...
MSG

# 4. Fire the release
git push origin v1.2.3
```

Pushing the tag runs `.forgejo/workflows/release.yaml`, which tests the
backend and publishes the image to
`ghcr.io/greyrock-labs/unleashed-voucher-manager`. Published images are not
really retractable, so get the preflight right rather than planning to fix it
afterwards.

## Choosing the version

| Bump | When |
|---|---|
| Patch | Fixes, and changes with no effect on configuration or behaviour a deployment relies on |
| Minor | Features, or dependency major versions landing, even when the image behaves the same |
| Major | An existing deployment must change something: an environment variable renamed or removed, a route changed, a default changed |

**Every commit that changes code belongs to a release.** A dependency bump
gets a version too; otherwise a deployed commit has no version naming it. A
commit that changes only documentation, or only a comment in a code file,
does not need a release.

## Tag message template

The title is the bare version. Add a short lead paragraph only when the bump
level needs explaining. Then these sections in this order, leaving out any
that are empty: `Fixed`, `Added`, `Changed`, `Dependencies`, `Upgrade notes`.

```
v1.2.3

Fixed

- <What a user would notice, then why it happened and what the fix does.
  One bullet per change, wrapped at 79 columns.>

Dependencies

- <name> <old> -> <new>. <What changed where it is used, and why behaviour
  is unchanged.>

Upgrade notes

None. No environment variables or routes changed.
```

House style:

- Write for someone deciding whether to upgrade. Explain why, not just what.
  "Bump dependencies" is not a release note: name the dependency, the
  versions and the reason.
- Say what did not change, for example "no configuration changed".
- `Upgrade notes` ends the message and is never left out. When there is
  nothing to do, say so explicitly.
- Commits that cancel out (a change and its revert) get a line saying so.
- Wrap at 79 columns.

## Never

- **Never hand-edit a version field to cut a release.** If you catch yourself
  editing one, the tag is what you wanted.
- **Never use a lightweight or unsigned tag.** Every release tag is signed,
  annotated and carries the notes.
- **Never tag a commit that is not on `origin/main`.** The workflow builds
  the tag from the remote.
- **Never move or force-push an existing tag.** Cut the next patch instead.
````

- [ ] **Step 4: Check the docs against the code**

Every environment variable read in `backend/src/environment.rs` and `scripts/entrypoint.sh` appears in the README, and nothing else does:

Run: `grep -ho '"[A-Z_]\{6,\}"' backend/src/environment.rs | sort -u` and compare with the README's variable list.
Expected: `UNLEASHED_URL`, `UNLEASHED_USERNAME`, `UNLEASHED_PASSWORD`, `UNLEASHED_SSID`, `UNLEASHED_HAS_VALID_CERT`, `BACKEND_BIND_HOST`, `BACKEND_BIND_PORT`, `ROLLING_VOUCHER_DURATION_MINUTES`, `PURGE_ALL_EXPIRED_VOUCHERS`, `TIMEZONE`, all documented.

Run: `grep -n -i -e 'unifi_' -e 'UNIFI_' README.md compose.yaml`
Expected: no output.

- [ ] **Step 5: Commit**

```bash
git add README.md LICENSE .agents
git commit -m "docs: document the Unleashed port and the release process"
```

---

### Task 10: Live check against a controller

This needs a real Unleashed controller and an account whose role has read-write admin privilege, supplied by the person running the check. Nothing here is committed. Every pass it creates is deleted at the end; passes that existed before must not be touched.

**Files:** none.

**Interfaces:**
- Consumes: the image from Task 8.
- Produces: a pass or fail report for each check below.

- [ ] **Step 1: Record the existing passes**

Note the ids and names already in the guest list (the controller's admin UI, or the app's Browse tab once it is running). These must still exist, unchanged, at the end.

- [ ] **Step 2: Run the container**

```bash
docker run --rm -d --name uvm-check -p 3000:3000 \
  -e UNLEASHED_URL -e UNLEASHED_USERNAME -e UNLEASHED_PASSWORD -e UNLEASHED_SSID \
  -e TIMEZONE -e WIFI_SSID -e WIFI_PASSWORD="" -e WIFI_TYPE=nopass \
  unleashed-voucher-manager:local
docker logs -f uvm-check
```

with those variables exported in the shell first. Expected log line: `Connected to the Unleashed controller`.

- [ ] **Step 3: Exercise every feature at `http://localhost:3000`**

- Quick Create twice with the default name: two vouchers named `Quick-Voucher`.
- Custom Create: a single voucher with 2 days, guest limit 3, key `CHECK1` and remarks; then the same key again (expect "That key is already in use"); then a batch of 3 (`Guest-N` names).
- Browse: search by name, open details (remarks shown, no data or speed rows), select two and delete them.
- Print one voucher in list mode and one in grid mode.
- `/kiosk` shows a `[ROLLING]-...` voucher and the QR code.
- `curl -X POST -H 'X-Forwarded-For: 192.0.2.50, 10.0.0.1' http://localhost:3000/rust-api/vouchers/rolling` returns a voucher named `...-192.0.2.50`; repeating it returns `403`.
- "Delete expired" reports a count without an error.

Expected: every check behaves as described; the controller's admin UI shows the same passes as the app.

- [ ] **Step 4: Clean up**

Delete every voucher created in Step 3 from the Browse tab, then confirm the guest list matches Step 1 exactly. Stop the container with `docker stop uvm-check`.

- [ ] **Step 5: Release**

When every check passes, cut `v1.0.0` by following `.agents/skills/cutting-a-release/SKILL.md`.
