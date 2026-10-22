use anyhow::{Context as _, Result, bail};
use bitcoin::{BlockHash, Network, blockdata::constants};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// A supported network, using the Bitcoin library's names and wire representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Network", into = "Network")]
pub struct SupportedNetwork(Network);

impl SupportedNetwork {
    /// Find the supported network with this genesis block.
    ///
    /// # Errors
    /// Rejects genesis hashes of unsupported or unknown networks.
    pub fn from_genesis(hash: BlockHash) -> Result<Self> {
        [Network::Bitcoin, Network::Signet, Network::Regtest]
            .into_iter()
            .find(|network| constants::genesis_block(*network).block_hash() == hash)
            .map(Self)
            .context("genesis block is not a supported network")
    }
}

impl TryFrom<Network> for SupportedNetwork {
    type Error = anyhow::Error;

    fn try_from(network: Network) -> Result<Self> {
        match network {
            Network::Bitcoin | Network::Signet | Network::Regtest => Ok(Self(network)),
            _ => bail!("unsupported network; expected bitcoin, signet, or regtest"),
        }
    }
}

impl From<SupportedNetwork> for Network {
    fn from(network: SupportedNetwork) -> Self {
        network.0
    }
}

impl FromStr for SupportedNetwork {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        Self::try_from(text.parse::<Network>()?)
    }
}

impl fmt::Display for SupportedNetwork {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}
