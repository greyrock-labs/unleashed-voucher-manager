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
