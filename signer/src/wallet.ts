// Key derivation and single-key payment scripts.
import { secp256k1 } from '@noble/curves/secp256k1.js';
import { HDKey } from '@scure/bip32';
import { mnemonicToSeedSync, validateMnemonic } from '@scure/bip39';
import { wordlist } from '@scure/bip39/wordlists/english.js';
import { NETWORK, TEST_NETWORK, p2pkh, p2sh, p2tr, p2wpkh } from '@scure/btc-signer';

export const SCRIPT_TYPES = ['p2pkh', 'p2sh-p2wpkh', 'p2wpkh', 'p2tr'] as const;
export type ScriptType = (typeof SCRIPT_TYPES)[number];

export const NETWORKS = {
  bitcoin: NETWORK,
  signet: TEST_NETWORK,
  regtest: { ...TEST_NETWORK, bech32: 'bcrt' },
} as const;
export type Network = keyof typeof NETWORKS;

export type Payment =
  | { type: 'p2pkh' | 'p2wpkh'; address: string; script: Uint8Array }
  | { type: 'p2sh-p2wpkh'; address: string; script: Uint8Array; redeemScript: Uint8Array }
  | { type: 'p2tr'; address: string; script: Uint8Array; tapInternalKey: Uint8Array };

// BIP-44, BIP-49, BIP-84, and BIP-86 purposes.
const PURPOSE: Record<ScriptType, number> = {
  p2pkh: 44,
  'p2sh-p2wpkh': 49,
  p2wpkh: 84,
  p2tr: 86,
};

export function defaultPath(
  type: ScriptType,
  network: Network,
  account: number,
  change: number,
  index: number,
): string {
  const coin = network === 'bitcoin' ? 0 : 1;
  return `m/${PURPOSE[type]}'/${coin}'/${account}'/${change}/${index}`;
}

export function seedFromMnemonic(mnemonic: string, passphrase: string): Uint8Array {
  const words = mnemonic.trim().split(/\s+/).join(' ');
  if (!validateMnemonic(words, wordlist)) {
    throw new Error('mnemonic is not a valid BIP-39 English mnemonic');
  }
  return mnemonicToSeedSync(words, passphrase);
}

export function seedFromHex(text: string): Uint8Array {
  const seed = text.trim();
  // BIP-32 seeds are 128 to 512 bits.
  if (!/^([0-9a-fA-F]{2}){16,64}$/.test(seed)) {
    throw new Error('seed must be 16 to 64 bytes of hex');
  }
  return Uint8Array.from(seed.match(/../g) ?? [], (byte) => parseInt(byte, 16));
}

export function deriveKey(seed: Uint8Array, path: string): Uint8Array {
  const key = HDKey.fromMasterSeed(seed).derive(path);
  if (!key.privateKey) {
    throw new Error('derived key has no private key');
  }
  return key.privateKey;
}

export function payment(type: ScriptType, privateKey: Uint8Array, network: Network): Payment {
  const publicKey = secp256k1.getPublicKey(privateKey, true);
  const net = NETWORKS[network];
  switch (type) {
    case 'p2pkh': {
      const { address, script } = p2pkh(publicKey, net);
      return { type, address: address!, script };
    }
    case 'p2sh-p2wpkh': {
      const { address, script, redeemScript } = p2sh(p2wpkh(publicKey, net), net);
      return { type, address: address!, script, redeemScript };
    }
    case 'p2wpkh': {
      const { address, script } = p2wpkh(publicKey, net);
      return { type, address: address!, script };
    }
    case 'p2tr': {
      // BIP-86 key path: no script tree.
      const { address, script, tapInternalKey } = p2tr(publicKey.subarray(1), undefined, net);
      return { type, address: address!, script, tapInternalKey };
    }
  }
}
