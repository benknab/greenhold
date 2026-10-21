#[expect(
    dead_code,
    reason = "Electrum version details and header queries are retained for future operations"
)]
pub mod electrum;
mod header;
mod json_rpc;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "P2P client is retained and tested for future node operations"
    )
)]
pub mod peer;
pub mod socket;
pub mod utreexod_rpc;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);
