use super::*;
use risc0_zkvm::{FakeReceipt, ReceiptClaim};

#[test]
fn verify_rejects_a_fake_receipt_for_this_guest() {
    // A fake receipt names this guest and a successful run, but has no proof.
    let journal = vec![0; 195];
    let claim = ReceiptClaim::ok(image_id(), journal.clone());
    let receipt = Receipt::new(InnerReceipt::Fake(FakeReceipt::new(claim)), journal);
    let error = verify(&receipt).err();
    assert_eq!(
        error.map(|error| error.to_string()).as_deref(),
        Some("receipt is not succinct")
    );
}
