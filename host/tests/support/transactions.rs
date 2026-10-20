use super::{Node, WALLET_ADDRESS, WALLET_PATH, WALLET_SEED};
use anyhow::{Context, Result, bail};
use bitcoin::{
    Address, Amount, CompressedPublicKey, EcdsaSighashType, Network, OutPoint, PrivateKey,
    ScriptBuf, Transaction, TxIn, TxOut, Witness,
    absolute::LockTime,
    consensus::{deserialize, serialize},
    ecdsa,
    hashes::Hash,
    hex::{DisplayHex, FromHex},
    secp256k1::{Message, Secp256k1},
    sighash::SighashCache,
    transaction::Version,
};
use serde_json::json;

/// Spend a P2WSH `OP_TRUE` coin on regtest.
pub fn spend(outpoint: OutPoint, values: &[u64], script: &ScriptBuf) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            witness: Witness::from_slice(&[vec![0x51]]),
            ..TxIn::default()
        }],
        output: values
            .iter()
            .map(|value| TxOut {
                value: Amount::from_sat(*value),
                script_pubkey: script.clone(),
            })
            .collect(),
    }
}

/// Spend a test wallet P2WPKH coin worth `value` on regtest.
pub fn spend_signed(
    outpoint: OutPoint,
    value: Amount,
    values: &[u64],
    script: &ScriptBuf,
) -> Result<Transaction> {
    let secp = Secp256k1::new();
    let key = PrivateKey::new(
        greenhold_signer::derive_key(&WALLET_SEED, &WALLET_PATH.parse()?)?,
        Network::Regtest,
    );
    let public = CompressedPublicKey::from_private_key(&secp, &key)?;
    let address = Address::p2wpkh(&public, Network::Regtest);
    if address.to_string() != WALLET_ADDRESS {
        bail!("test wallet key does not match its address");
    }
    let mut transaction = spend(outpoint, values, script);
    let sighash = SighashCache::new(&transaction).p2wpkh_signature_hash(
        0,
        &address.script_pubkey(),
        value,
        EcdsaSighashType::All,
    )?;
    let signature = secp.sign_ecdsa(&Message::from_digest(sighash.to_byte_array()), &key.inner);
    transaction.input[0].witness =
        Witness::p2wpkh(&ecdsa::Signature::sighash_all(signature), &public.0);
    Ok(transaction)
}

pub fn mined(node: &Node, height: u32, script: &ScriptBuf) -> Result<OutPoint> {
    let hash = node.rpc("getblockhash", &json!([height]))?;
    let raw = node.rpc("getblock", &json!([hash, 0]))?;
    let block: bitcoin::Block =
        deserialize(&Vec::<u8>::from_hex(raw.as_str().context("raw block")?)?)?;
    let coinbase = &block.txdata[0];
    let vout = coinbase
        .output
        .iter()
        .position(|output| output.script_pubkey == *script)
        .context("mining output")?;
    Ok(OutPoint::new(coinbase.compute_txid(), u32::try_from(vout)?))
}

pub fn broadcast(node: &Node, transaction: &Transaction) -> Result<()> {
    node.rpc(
        "sendrawtransaction",
        &json!([serialize(transaction).as_hex().to_string()]),
    )?;
    Ok(())
}
