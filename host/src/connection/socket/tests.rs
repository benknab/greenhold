use super::*;

#[test]
fn endpoint_requires_host_port_and_no_request_data() -> Result<()> {
    let endpoint: SocketUrl = "tcp://[::1]:8335".parse()?;
    assert_eq!(
        endpoint.url().host(),
        Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
    );
    assert_eq!(endpoint.url().port(), Some(8335));
    for text in [
        "tcp://host",
        "tcp://host:0",
        "tcp://host:65536",
        "tcp://::1:123",
        "tcp://user:password@host:123",
        "tcp://user@host:123",
        "tcp://[bad]:123",
        "tcp://host:123/path",
        "tcp://host:123?query",
        "tcp://host:123#fragment",
        "tcp:host:123",
    ] {
        assert!(text.parse::<SocketUrl>().is_err(), "{text}");
    }
    Ok(())
}
