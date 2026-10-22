use super::*;

#[test]
fn endpoint_requires_explicit_transport() -> Result<()> {
    let endpoint: Endpoint = "tcp://[::1]:8335".parse()?;
    assert_eq!(endpoint.transport, TransportKind::Tcp);
    let endpoint: Endpoint = "bip324://[::1]:8335".parse()?;
    assert_eq!(endpoint.transport, TransportKind::Bip324);
    for text in [
        "ssl://localhost:8335",
        "tls://localhost:8335",
        "https://localhost:8335",
    ] {
        assert!(text.parse::<Endpoint>().is_err(), "{text}");
    }
    Ok(())
}
