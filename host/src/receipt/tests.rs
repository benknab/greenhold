use super::*;
use risc0_zkvm::{FakeReceipt, InnerReceipt, ReceiptClaim, sha::Digest};

/// Format checks only: a fake receipt has the receipt structure but proves nothing.
fn receipt(journal: Vec<u8>) -> Receipt {
    let claim = ReceiptClaim::ok(Digest::ZERO, journal.clone());
    Receipt::new(InnerReceipt::Fake(FakeReceipt::new(claim)), journal)
}

#[test]
fn receipt_files_round_trip_and_reject_other_formats() -> Result<()> {
    let bytes = encode(&receipt(b"journal".to_vec()))?;
    assert_eq!(&bytes[..TAG.len()], b"greenhold/receipt/v1\0");
    let decoded = decode(&bytes)?;
    assert_eq!(decoded.journal.bytes, b"journal");
    assert_eq!(encode(&decoded)?, bytes);

    let mut trailing = bytes.clone();
    trailing.push(0);
    let mut version = bytes.clone();
    version[TAG.len() - 2] = b'2';
    for (bytes, message) in [
        (trailing, "receipt decoding"),
        (bytes[..bytes.len() - 1].to_vec(), "receipt decoding"),
        (version, "receipt tag or version is not supported"),
    ] {
        let error = decode(&bytes)
            .err()
            .context("invalid receipt file accepted")?;
        assert_eq!(error.to_string(), message);
    }
    Ok(())
}

#[test]
fn encoding_fills_but_never_exceeds_the_file_limit() -> Result<()> {
    let size = |length: usize| -> Result<usize> { Ok(encode(&receipt(vec![0; length]))?.len()) };
    // Each journal byte adds the same number of file bytes.
    let empty = size(0)?;
    let per_byte = size(1)? - empty;
    let limit = usize::try_from(MAX_FILE_BYTES)?;
    let largest = (limit - empty) / per_byte;
    assert!(size(largest)? > limit - per_byte);
    assert!(size(largest)? <= limit);
    let error = encode(&receipt(vec![0; largest + 1])).err();
    assert_eq!(
        error.map(|error| error.to_string()).as_deref(),
        Some("receipt encoding")
    );
    Ok(())
}
