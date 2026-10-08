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
    /// Unix seconds; `None` when the controller left it empty or zero, which
    /// must never read as "expired".
    pub expire_time: Option<i64>,
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
                expire_time: Some(number("expire-time")?).filter(|t| *t > 0),
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
