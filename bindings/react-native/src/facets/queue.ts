import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey, QueueSeq } from '../internal';

/**
 * Append `entry` to `stream` in `workspace` (UUID or name, created with the queue facet if absent);
 * resolves the assigned zero-based sequence as an unsigned 64-bit decimal string.
 */
export async function queueAppend(
  loomPath: string,
  workspace: string,
  stream: string,
  entry: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<QueueSeq> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.queueAppend(loomPath, workspace, stream, Array.from(entry), passphrase, kek, authPrincipal, authPassphrase);
}

/** Fetch the entry at decimal-string `seq` in `stream`, or null if out of range. */
export async function queueGet(
  loomPath: string,
  workspace: string,
  stream: string,
  seq: QueueSeq,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.queueGet(loomPath, workspace, stream, seq, passphrase, kek, authPrincipal, authPassphrase);
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** The half-open range `[lo, hi)` of `stream` as raw Loom Canonical CBOR (an array of byte strings). */
export async function queueRangeCbor(
  loomPath: string,
  workspace: string,
  stream: string,
  lo: QueueSeq,
  hi: QueueSeq,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.queueRange(loomPath, workspace, stream, lo, hi, passphrase, kek, authPrincipal, authPassphrase);
  return Uint8Array.from(bytes);
}

/** The number of entries in `stream`. */
export function queueLen(
  loomPath: string,
  workspace: string,
  stream: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<QueueSeq> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.queueLen(loomPath, workspace, stream, passphrase, kek, authPrincipal, authPassphrase);
}

/** The named consumer's next decimal-string sequence for `stream`; "0" when none is stored. */
export function queueConsumerPosition(
  loomPath: string,
  workspace: string,
  stream: string,
  consumerId: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<QueueSeq> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.queueConsumerPosition(
    loomPath, workspace, stream, consumerId, passphrase, kek, authPrincipal, authPassphrase
  );
}

/**
 * Up to `max` entries from the consumer's stored next sequence as raw Loom Canonical CBOR; does not
 * advance the consumer.
 */
export async function queueConsumerReadCbor(
  loomPath: string,
  workspace: string,
  stream: string,
  consumerId: string,
  max: number,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.queueConsumerRead(
    loomPath, workspace, stream, consumerId, max, passphrase, kek, authPrincipal, authPassphrase
  );
  return Uint8Array.from(bytes);
}

/** Advance the named consumer's next sequence for `stream` to `nextSeq` (monotonic). */
export function queueConsumerAdvance(
  loomPath: string,
  workspace: string,
  stream: string,
  consumerId: string,
  nextSeq: QueueSeq,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.queueConsumerAdvance(
    loomPath, workspace, stream, consumerId, nextSeq, passphrase, kek, authPrincipal, authPassphrase
  );
}

/** Set the named consumer's next sequence for `stream` to `nextSeq` (may move backward). */
export function queueConsumerReset(
  loomPath: string,
  workspace: string,
  stream: string,
  consumerId: string,
  nextSeq: QueueSeq,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.queueConsumerReset(
    loomPath, workspace, stream, consumerId, nextSeq, passphrase, kek, authPrincipal, authPassphrase
  );
}
