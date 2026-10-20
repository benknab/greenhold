use super::*;

#[test]
fn distinguishes_null_missing_and_error_results() -> Result<()> {
    assert_eq!(
        Response::parse(br#"{"id":1,"result":null}"#)?.into_result::<Option<u64>>()?,
        None
    );
    assert_eq!(
        Response::parse(br#"{"id":1,"result":7,"error":null,"extra":true}"#)?
            .into_result::<u64>()?,
        7
    );
    for bytes in [
        br#"{"id":1}"#.as_slice(),
        br#"{"id":1,"error":null}"#,
        br#"{"id":null,"result":7}"#,
        br#"{"id":"1","result":7}"#,
        br#"{"result":7}"#,
        br#"{"id":1,"id":1,"result":7}"#,
        br#"{"id":1,"error":{"message":"missing code"}}"#,
        br#"{"id":1,"error":{"code":"secret"}}"#,
    ] {
        assert!(Response::parse(bytes).is_err());
    }
    // A server error must not attempt to deserialize a success schema.
    for bytes in [
        br#"{"id":1,"error":{"code":-1,"message":"secret"}}"#.as_slice(),
        br#"{"id":1,"result":null,"error":{"code":-1,"message":"secret"}}"#,
    ] {
        let error = Response::parse(bytes)?
            .into_result::<u64>()
            .err()
            .context("expected server error")?;
        assert_eq!(format!("{error:#}"), "node rejected request");
        assert!(error.is::<ServerError>());
        assert_eq!(
            error
                .downcast_ref::<ServerError>()
                .context("server error")?
                .code,
            -1
        );
    }
    Ok(())
}

#[test]
fn schema_errors_do_not_leak_remote_values() -> Result<()> {
    let error = Response::parse(br#"{"id":1,"result":"secret"}"#)?
        .into_result::<u64>()
        .err()
        .context("expected schema error")?;
    assert_eq!(format!("{error:#}"), "invalid JSON-RPC result");
    assert!(!error.is::<ServerError>());
    let error = Response::parse(br#"{"id":"secret","result":7}"#)
        .err()
        .context("expected envelope error")?;
    assert_eq!(format!("{error:#}"), "invalid JSON-RPC response");
    Ok(())
}
