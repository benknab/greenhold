// Signer CLI: derive addresses and sign BIP-322 messages from a mnemonic or seed file.
import { readFileSync } from 'node:fs';
import { parseArgs } from 'node:util';
import { FORMATS, type Signing, sign } from './bip322.ts';
import {
  NETWORKS,
  type Network,
  SCRIPT_TYPES,
  type ScriptType,
  defaultPath,
  deriveKey,
  payment,
  seedFromHex,
  seedFromMnemonic,
} from './wallet.ts';

const USAGE = `Usage:
  bun src/main.ts address --type TYPE (--mnemonic-file FILE | --seed-file FILE) [options]
  bun src/main.ts sign --type TYPE --message TEXT (--mnemonic-file FILE | --seed-file FILE)
                       [options]

Types and formats (the first format is the default):
${SCRIPT_TYPES.map((type) => `  ${type.padEnd(12)} ${FORMATS[type].join(', ')}`).join('\n')}

Options:
  --format FORMAT         Signature format, for sign only
  --network NAME          ${Object.keys(NETWORKS).join(', ')} (default bitcoin)
  --path PATH             BIP-32 path, such as m/84'/0'/0'/0/0
  --account N, --change N, --index N
                          Default path parts when --path is absent (default 0)
  --mnemonic-file FILE    BIP-39 English mnemonic
  --passphrase-file FILE  BIP-39 passphrase, with --mnemonic-file only
  --seed-file FILE        BIP-32 root seed in hex

Secrets are read only from files. Output is JSON.`;

type Secret =
  | { kind: 'mnemonic'; file: string; passphraseFile?: string }
  | { kind: 'seed'; file: string };

/** Where the key comes from. The path already includes the script type's default. */
interface Key {
  network: Network;
  path: string;
  secret: Secret;
}

type Request =
  | { command: 'help' }
  | { command: 'address'; type: ScriptType; key: Key }
  | { command: 'sign'; signing: Signing; message: string; key: Key };

const OPTIONS = {
  type: { type: 'string' },
  network: { type: 'string' },
  path: { type: 'string' },
  account: { type: 'string' },
  change: { type: 'string' },
  index: { type: 'string' },
  'mnemonic-file': { type: 'string' },
  'passphrase-file': { type: 'string' },
  'seed-file': { type: 'string' },
  message: { type: 'string' },
  format: { type: 'string' },
  help: { type: 'boolean' },
} as const;

type Values = ReturnType<typeof parseArgs<{ options: typeof OPTIONS }>>['values'];

function choice<T extends string>(
  value: string | undefined,
  options: readonly T[],
  name: string,
): T {
  if (value === undefined || !(options as readonly string[]).includes(value)) {
    throw new Error(`${name} must be one of: ${options.join(', ')}`);
  }
  return value as T;
}

function number(value: string | undefined, name: string): number {
  const text = value ?? '0';
  // BIP-32 non-hardened index range.
  if (!/^\d+$/.test(text) || Number(text) >= 2 ** 31) {
    throw new Error(`${name} must be a whole number below 2^31`);
  }
  return Number(text);
}

function parseSigning(type: ScriptType, format: string | undefined): Signing {
  const allowed: readonly string[] = FORMATS[type];
  const chosen = format ?? allowed[0];
  if (chosen === undefined || !allowed.includes(chosen)) {
    throw new Error(`--format for ${type} must be one of: ${allowed.join(', ')}`);
  }
  // FORMATS is checked against Signing, so this pair is one of its variants.
  return { type, format: chosen } as Signing;
}

function parseSecret(values: Values): Secret {
  const mnemonic = values['mnemonic-file'];
  const seed = values['seed-file'];
  const passphraseFile = values['passphrase-file'];
  if (mnemonic !== undefined && seed === undefined) {
    return passphraseFile === undefined
      ? { kind: 'mnemonic', file: mnemonic }
      : { kind: 'mnemonic', file: mnemonic, passphraseFile };
  }
  if (seed !== undefined && mnemonic === undefined) {
    if (passphraseFile !== undefined) {
      throw new Error('--passphrase-file needs --mnemonic-file');
    }
    return { kind: 'seed', file: seed };
  }
  throw new Error('provide exactly one of --mnemonic-file or --seed-file');
}

function parseKey(type: ScriptType, values: Values): Key {
  const networks = Object.keys(NETWORKS) as Network[];
  const network = choice(values.network ?? 'bitcoin', networks, '--network');
  const parts = [values.account, values.change, values.index];
  if (values.path !== undefined && parts.some((part) => part !== undefined)) {
    throw new Error('use --path or --account/--change/--index, not both');
  }
  const path =
    values.path ??
    defaultPath(
      type,
      network,
      number(values.account, '--account'),
      number(values.change, '--change'),
      number(values.index, '--index'),
    );
  return { network, path, secret: parseSecret(values) };
}

/** Check the whole public request before any secret file is read. */
function parseRequest(argv: string[]): Request {
  const { values, positionals } = parseArgs({
    args: argv,
    allowPositionals: true,
    strict: true,
    options: OPTIONS,
  });
  const [command, ...rest] = positionals;
  if (values.help || command === undefined) {
    return { command: 'help' };
  }
  if (rest.length > 0) {
    throw new Error('expected one command: address or sign');
  }
  const type = choice(values.type, SCRIPT_TYPES, '--type');
  switch (command) {
    case 'address':
      if (values.message !== undefined || values.format !== undefined) {
        throw new Error('--message and --format are for sign only');
      }
      return { command, type, key: parseKey(type, values) };
    case 'sign':
      if (values.message === undefined) {
        throw new Error('sign needs --message');
      }
      return {
        command,
        signing: parseSigning(type, values.format),
        message: values.message,
        key: parseKey(type, values),
      };
    default:
      throw new Error('expected one command: address or sign');
  }
}

function readSecret(path: string): string {
  try {
    return readFileSync(path, 'utf8');
  } catch {
    throw new Error(`cannot read secret file ${path}`);
  }
}

function privateKey({ path, secret }: Key): Uint8Array {
  switch (secret.kind) {
    case 'mnemonic': {
      const passphrase =
        secret.passphraseFile === undefined
          ? ''
          : readSecret(secret.passphraseFile).replace(/\r?\n$/, '');
      return deriveKey(seedFromMnemonic(readSecret(secret.file), passphrase), path);
    }
    case 'seed':
      return deriveKey(seedFromHex(readSecret(secret.file)), path);
  }
}

function run(request: Request): void {
  switch (request.command) {
    case 'help':
      console.log(USAGE);
      return;
    case 'address': {
      const { network, path } = request.key;
      const { address } = payment(request.type, privateKey(request.key), network);
      console.log(JSON.stringify({ type: request.type, network, path, address }, null, 2));
      return;
    }
    case 'sign': {
      const { network, path } = request.key;
      const { type, format } = request.signing;
      const signed = sign(request.signing, privateKey(request.key), network, request.message);
      console.log(JSON.stringify({ type, format, network, path, ...signed }, null, 2));
      return;
    }
  }
}

try {
  run(parseRequest(process.argv.slice(2)));
} catch (error) {
  console.error(`signer: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
}
