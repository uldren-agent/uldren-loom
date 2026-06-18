import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/**
 * Create (or replace the metadata of) mailbox `mailbox` under `principal` in `workspace` (UUID or name,
 * created with the `mail` facet if absent).
 */
export function mailCreateMailbox(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  displayName: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.mailCreateMailbox(loomPath, workspace, principal, mailbox, displayName, passphrase, kek, authPrincipal, authPassphrase);
}

/** Delete mailbox `mailbox` under `principal` and its message indexes/flags; resolves whether it existed. */
export function mailDeleteMailbox(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.mailDeleteMailbox(loomPath, workspace, principal, mailbox, passphrase, kek, authPrincipal, authPassphrase);
}

/** The mailbox ids under `principal` as raw Loom Canonical CBOR (an array of text strings). */
export async function mailListMailboxesCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.mailListMailboxes(loomPath, workspace, principal, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/**
 * Ingest the raw RFC 5322 message `raw` into `mailbox` under `uid` (CAS the body, index the headers);
 * resolves the body's content address ("algo:hex").
 */
export function mailIngestMessage(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  uid: string,
  raw: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.mailIngestMessage(
    loomPath,
    workspace,
    principal,
    mailbox,
    uid,
    Array.from(raw),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch the structured index of the message at `uid` as its `MailMessage` canonical CBOR, or null. */
export async function mailGetMessage(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.mailGetMessage(loomPath, workspace, principal, mailbox, uid, passphrase, kek, authPrincipal, authPassphrase);
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Fetch the raw RFC 5322 body (`.eml` bytes) of the message at `uid`, digest-verified, or null. */
export async function mailToEml(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.mailToEml(loomPath, workspace, principal, mailbox, uid, passphrase, kek, authPrincipal, authPassphrase);
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Remove the message index and its flags at `uid` (body stays in the CAS); resolves whether it existed. */
export function mailDeleteMessage(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.mailDeleteMessage(loomPath, workspace, principal, mailbox, uid, passphrase, kek, authPrincipal, authPassphrase);
}

/** List `mailbox` as raw Loom Canonical CBOR (an array of per-message `MailMessage` CBOR byte strings). */
export async function mailListMessagesCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.mailListMessages(loomPath, workspace, principal, mailbox, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** The flags/labels on the message at `uid` as raw Loom Canonical CBOR (sorted, deduplicated text strings). */
export async function mailGetFlagsCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.mailGetFlags(loomPath, workspace, principal, mailbox, uid, passphrase, kek, authPrincipal, authPassphrase);
  return Uint8Array.from(bytes);
}

/** Replace the flags/labels on the message at `uid` with `flags` (a Loom Canonical CBOR `Array(Text)`). */
export function mailSetFlags(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  uid: string,
  flags: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.mailSetFlags(
    loomPath,
    workspace,
    principal,
    mailbox,
    uid,
    Array.from(flags),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/**
 * Search `mailbox` by a case-insensitive substring `text` over subject and from; resolves raw Loom
 * Canonical CBOR (an array of per-message `MailMessage` CBOR byte strings).
 */
export async function mailSearchCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  mailbox: string,
  text: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.mailSearch(loomPath, workspace, principal, mailbox, text, passphrase, kek, authPrincipal, authPassphrase);
  return Uint8Array.from(bytes);
}
