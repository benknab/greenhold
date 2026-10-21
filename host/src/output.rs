use crate::guest;
use anyhow::Result;
use bitcoin::Address;
use greenhold_core::claim::Claim;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Prepared,
    Executed,
    Proved,
    Verified,
    Inspected,
}

#[derive(Debug, Serialize)]
pub struct Report<'a> {
    status: Status,
    claim: &'a Claim,
    message: String,
    image_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_file: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub witness: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycles: Option<u64>,
}

impl<'a> Report<'a> {
    pub fn new(claim: &'a Claim, status: Status) -> Self {
        Self {
            status,
            claim,
            message: claim.signing_message(),
            image_id: guest::image_id().to_string(),
            claim_file: None,
            witness: None,
            address: None,
            receipt: None,
            receipt_bytes: None,
            cycles: None,
        }
    }

    pub fn print(&self, json: bool) -> Result<()> {
        if json {
            println!("{}", serde_json::to_string_pretty(self)?);
            return Ok(());
        }
        let heading = match self.status {
            Status::Prepared => "Claim prepared",
            Status::Executed => "Dry run completed",
            Status::Proved => "Receipt created",
            Status::Verified => "Proof and chain state verified",
            Status::Inspected => "Proof checked; chain state not checked",
        };
        println!("{heading}\n");
        println!("Network: {}", self.claim.network());
        println!(
            "Minimum amount: {} sats",
            u64::from(self.claim.minimum_sats())
        );
        println!("First height: {}", self.claim.first_height());
        println!("Last height: {}", self.claim.last_height());
        println!("Nonce: {}", self.claim.nonce());
        for (label, path) in [
            ("Claim saved", self.claim_file),
            ("Private witness saved", self.witness),
            ("Receipt saved", self.receipt),
        ] {
            if let Some(path) = path {
                println!("{label}: {}", path.display());
            }
        }
        if let Some(bytes) = self.receipt_bytes {
            println!("Receipt size: {bytes} bytes");
        }
        if let Some(cycles) = self.cycles {
            println!("Cycles: {cycles}");
        }
        if let Some(address) = &self.address {
            println!(
                "\nSign this exact message with address:\n{address}\n\n{}",
                self.message
            );
        }
        Ok(())
    }
}
