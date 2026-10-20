// BIP-322 message signing for single-key scripts.
import { secp256k1 } from '@noble/curves/secp256k1.js';
import { sha256 } from '@noble/hashes/sha2.js';
import { base64, hex } from '@scure/base';
import { OP, RawWitness, SigHash, Transaction } from '@scure/btc-signer';
import { RawOldTx } from '@scure/btc-signer/script.js';
import { type Network, type Payment, type ScriptType, payment as derivePayment } from './wallet.ts';

/** The formats BIP-322 allows for each script type. */
export type Signing =
  | { type: 'p2pkh'; format: 'full' | 'legacy' }
  | { type: 'p2sh-p2wpkh'; format: 'full' }
  | { type: 'p2wpkh'; format: 'simple' | 'full' }
  | { type: 'p2tr'; format: 'simple' | 'full' };

/** Allowed formats per script type. The first one is the default. */
export const FORMATS = {
  p2pkh: ['full', 'legacy'],
  'p2sh-p2wpkh': ['full'],
  p2wpkh: ['simple', 'full'],
  p2tr: ['simple', 'full'],
} as const satisfies { [T in ScriptType]: readonly Extract<Signing, { type: T }>['format'][] };

const encoder = new TextEncoder();
const TAG = sha256(encoder.encode('BIP0322-signed-message'));

function concat(...parts: Uint8Array[]): Uint8Array {
  const bytes = new Uint8Array(parts.reduce((length, part) => length + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    bytes.set(part, offset);
    offset += part.length;
  }
  return bytes;
}

function hash256(bytes: Uint8Array): Uint8Array {
  return sha256(sha256(bytes));
}

function messageHash(message: string): Uint8Array {
  return sha256(concat(TAG, TAG, encoder.encode(message)));
}

/** The serialized `to_spend` transaction. */
function toSpend(script: Uint8Array, message: string): Uint8Array {
  return RawOldTx.encode({
    version: 0,
    inputs: [
      {
        txid: new Uint8Array(32),
        index: 0xffffffff,
        // OP_0 PUSH32[message_hash]
        finalScriptSig: concat(Uint8Array.of(0x00, 0x20), messageHash(message)),
        sequence: 0,
      },
    ],
    outputs: [{ amount: 0n, script }],
    lockTime: 0,
  });
}

/** Display-order transaction ID. */
function txid(raw: Uint8Array): string {
  return hex.encode(hash256(raw).reverse());
}

/** The unsigned `to_sign` transaction, with no time locks. */
function toSign(payment: Payment, message: string): Transaction {
  const spend = toSpend(payment.script, message);
  const tx = new Transaction({ version: 0, lockTime: 0, allowUnknownOutputs: true });
  const prevout = { txid: txid(spend), index: 0, sequence: 0 };
  const witnessUtxo = { script: payment.script, amount: 0n };
  switch (payment.type) {
    case 'p2pkh':
      // Legacy inputs sign against the full previous transaction.
      tx.addInput({ ...prevout, nonWitnessUtxo: spend });
      break;
    case 'p2sh-p2wpkh':
      tx.addInput({ ...prevout, witnessUtxo, redeemScript: payment.redeemScript });
      break;
    case 'p2wpkh':
      tx.addInput({ ...prevout, witnessUtxo });
      break;
    case 'p2tr':
      tx.addInput({ ...prevout, witnessUtxo, tapInternalKey: payment.tapInternalKey });
      break;
  }
  tx.addOutput({ script: Uint8Array.of(OP.RETURN), amount: 0n });
  return tx;
}

/** Sign `message` for the key's script. Returns the address and the prefixed signature. */
export function sign(
  signing: Signing,
  privateKey: Uint8Array,
  network: Network,
  message: string,
): { address: string; signature: string } {
  const payment = derivePayment(signing.type, privateKey, network);
  const signature =
    signing.format === 'legacy'
      ? signLegacy(privateKey, message)
      : signTransaction(signing.format, payment, privateKey, message);
  return { address: payment.address, signature };
}

function signTransaction(
  format: 'simple' | 'full',
  payment: Payment,
  privateKey: Uint8Array,
  message: string,
): string {
  const tx = toSign(payment, message);
  // Taproot signs with SIGHASH_DEFAULT; ECDSA inputs sign with SIGHASH_ALL.
  const sighash = payment.type === 'p2tr' ? SigHash.DEFAULT : SigHash.ALL;
  if (tx.sign(privateKey, [sighash]) !== 1) {
    throw new Error('the key cannot sign for this script');
  }
  tx.finalize();
  if (format === 'full') {
    return `ful${base64.encode(tx.extract())}`;
  }
  const witness = tx.getInput(0).finalScriptWitness;
  if (witness === undefined) {
    throw new Error('signed input has no witness');
  }
  return `smp${base64.encode(RawWitness.encode(witness))}`;
}

/** BIP-137 signature with a compressed P2PKH key: header 31 to 34. */
function signLegacy(privateKey: Uint8Array, message: string): string {
  const prefix = encoder.encode('\x18Bitcoin Signed Message:\n');
  const body = encoder.encode(message);
  const hash = hash256(concat(prefix, compactSize(body.length), body));
  const recovered = secp256k1.sign(hash, privateKey, { prehash: false, format: 'recovered' });
  const header = 27 + 4 + recovered[0]!;
  return base64.encode(concat(Uint8Array.of(header), recovered.subarray(1)));
}

function compactSize(length: number): Uint8Array {
  if (length < 0xfd) {
    return Uint8Array.of(length);
  }
  if (length <= 0xffff) {
    return Uint8Array.of(0xfd, length & 0xff, length >> 8);
  }
  throw new Error('message is too long');
}
