export type QueueSeq = string;

/** A lossless bridge cell: a JSON primitive, a nested array (a SQL `LIST`), or a single-key tagged
 * object for a type the bridge can't carry bare - `{ $i64 | $u64 | $i128 | $u128: string }`,
 * `{ $f32: number }` (raw bits), `{ $f64: string }` (raw bits; NaN/Inf/-0.0), `{ $decimal: { mantissa,
 * scale } }`, `{ $bytes: string }` (base64), `{ $uuid | $inet: string }`, `{ $date: number }`,
 * `{ $time | $timestamp: string }`, `{ $interval: { months, micros } }`, `{ $point: { x, y } }` (raw
 * bits), or `{ $map: object }`. */
export type LoomCell =
  | null
  | boolean
  | number
  | string
  | LoomCell[]
  | { [tag: string]: unknown };

/**
 * Unlock material for an **encrypted** store. Supply a `passphrase` or a 32-byte `kek` (keychain /
 * Secure Enclave / passkey-PRF / KMS). Omit for an unencrypted store. If both are given, `kek` wins.
 * Held only for the single op (each call reopens the loom).
 */
export interface LoomKey {
  passphrase?: string;
  kek?: Uint8Array | number[];
}

export interface LoomAuth {
  principal: string;
  passphrase: string;
}

export function keyArgs(key?: LoomKey): [string, number[]] {
  return [key?.passphrase ?? '', key?.kek ? Array.from(key.kek) : []];
}

export function authArgs(auth?: LoomAuth): [string, string] {
  return [auth?.principal ?? '', auth?.passphrase ?? ''];
}

/** One typed statement result from {@link sqlExec}. */
export interface LoomStatement {
  kind: string;
  columns?: Array<{ name: string; type?: string }>;
  rows?: LoomCell[][];
  count?: number;
  variable?: string;
  values?: string[];
  value?: string;
}
