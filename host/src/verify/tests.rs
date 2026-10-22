use super::*;
use crate::cli::{Cli, Command};
use bitcoin::{Network, blockdata::constants};
use clap::Parser as _;
use greenhold_core::{
    context::{AccumulatorState, BlockEndpoint, ChainContext},
    eligibility::MinimumAmount,
};

#[test]
fn expected_nonce_is_required_even_when_the_flag_is_omitted() -> Result<()> {
    let block = BlockEndpoint::try_new(
        Network::Bitcoin,
        0,
        constants::genesis_block(Network::Bitcoin).header,
    )?;
    let context =
        ChainContext::try_new(block.clone(), block, AccumulatorState::try_new(0, vec![])?)?;
    let challenge = " Challenge café 🔑 ";
    for original in ["", challenge] {
        let claim = Claim::new(
            &context,
            MinimumAmount::try_from(1)?,
            Nonce::from_text(original),
        );
        let hash = claim.nonce().to_string();
        for expected in [
            None,
            Some(""),
            Some(challenge),
            Some(challenge.trim()),
            Some(hash.as_str()),
        ] {
            let mut args = vec!["host", "verify", "--receipt", "claim.receipt"];
            if let Some(text) = expected {
                args.extend(["--nonce", text]);
            }
            let Command::Verify(options) = Cli::try_parse_from(args)?.command else {
                bail!("expected verify command");
            };
            let result = check_expected(&claim, options.network, options.nonce);
            if expected.unwrap_or_default() == original {
                result?;
            } else {
                let error = result
                    .err()
                    .ok_or_else(|| anyhow::anyhow!("a different nonce must fail"))?;
                assert_eq!(
                    error.to_string(),
                    "receipt nonce does not match the expected nonce"
                );
            }
        }
    }
    Ok(())
}
