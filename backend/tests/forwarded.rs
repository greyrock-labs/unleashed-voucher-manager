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
