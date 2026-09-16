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
