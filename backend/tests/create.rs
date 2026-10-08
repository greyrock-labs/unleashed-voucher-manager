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

#[test]
fn rejects_name_characters_the_controller_refuses() {
    // Probed: these make the controller answer OK and create nothing
    for c in [
        '\'', '"', '<', '>', '&', '#', ';', '\\', '`', '|', '!', '$', '(', ')',
    ] {
        let request = CreateParams {
            name: format!("Bob{c}s"),
            ..params()
        };
        assert!(build_form(&request).is_err(), "{c:?} should be rejected");
    }
}

#[test]
fn allows_name_punctuation_the_controller_accepts() {
    let request = CreateParams {
        name: "a+b,c%d*e=f?g~h^i[j]k{l}m/n:o@p".into(),
        ..params()
    };
    assert!(build_form(&request).is_ok());
}

#[test]
fn remarks_only_reject_angle_brackets() {
    let ok = CreateParams {
        remarks: Some("Bob's \"phone\" & laptop; #2 (front desk)!".into()),
        ..params()
    };
    assert!(build_form(&ok).is_ok());
    for bad in ["a<b", "a>b"] {
        let request = CreateParams {
            remarks: Some(bad.into()),
            ..params()
        };
        assert!(build_form(&request).is_err(), "{bad:?} should be rejected");
    }
}

#[test]
fn rejects_text_the_controller_would_garble() {
    // Probed: the controller stores "é" as "Ã©"
    let name = CreateParams {
        name: "Café".into(),
        ..params()
    };
    assert!(build_form(&name).is_err());
    let remarks = CreateParams {
        remarks: Some("Café".into()),
        ..params()
    };
    assert!(build_form(&remarks).is_err());
}

#[test]
fn batch_names_are_not_checked() {
    // The controller names batch passes itself and ignores the name
    let request = CreateParams {
        count: 2,
        name: "Bob's".into(),
        ..params()
    };
    assert!(build_form(&request).is_ok());
}

#[test]
fn reads_an_invalid_characters_response() {
    let body = std::fs::read_to_string(format!(
        "{}/tests/fixtures/create_invalid.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    assert_eq!(
        parse_create_response(&body),
        CreateOutcome::InvalidCharacters("Invalid Characters detected in {1}.".into())
    );
}
