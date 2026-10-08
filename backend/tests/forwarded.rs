use std::net::IpAddr;

use backend::handlers::forwarded_client_ip;

fn ip(s: &str) -> Option<IpAddr> {
    Some(s.parse().unwrap())
}

#[test]
fn reads_the_address_the_frontend_forwards() {
    assert_eq!(forwarded_client_ip("192.0.2.7"), ip("192.0.2.7"));
    assert_eq!(forwarded_client_ip(" 192.0.2.7 "), ip("192.0.2.7"));
    assert_eq!(forwarded_client_ip("2001:db8::7"), ip("2001:db8::7"));
}

#[test]
fn takes_the_first_entry_of_a_list() {
    assert_eq!(forwarded_client_ip("192.0.2.7, 10.0.0.1"), ip("192.0.2.7"));
}

#[test]
fn unwraps_ipv4_mapped_addresses() {
    assert_eq!(forwarded_client_ip("::ffff:192.0.2.7"), ip("192.0.2.7"));
}

#[test]
fn rejects_anything_that_is_not_an_address() {
    for bad in [
        "",
        " , 10.0.0.1",
        "unknown",
        "192.0.2.7-evil",
        "<guest id='1'>",
    ] {
        assert_eq!(forwarded_client_ip(bad), None, "{bad:?}");
    }
}
