import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/**
 * Create (or replace the metadata of) address book `book` under `principal` in `workspace` (UUID or name,
 * created with the `contacts` facet if absent).
 */
export function cardCreateBook(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  displayName: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.cardCreateBook(loomPath, workspace, principal, book, displayName, passphrase, kek, authPrincipal, authPassphrase);
}

/** Delete address book `book` under `principal` and its contacts; resolves whether it existed. */
export function cardDeleteBook(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.cardDeleteBook(loomPath, workspace, principal, book, passphrase, kek, authPrincipal, authPassphrase);
}

/** The address-book ids under `principal` as raw Loom Canonical CBOR (an array of text strings). */
export async function cardListBooksCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.cardListBooks(loomPath, workspace, principal, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** Put the contact `entry` (its `ContactEntry` canonical CBOR) into `book`, keyed by its UID. */
export function cardPutEntry(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  entry: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.cardPutEntry(loomPath, workspace, principal, book, Array.from(entry), passphrase, kek, authPrincipal, authPassphrase);
}

/** Fetch the contact at `uid` in `book` as its `ContactEntry` canonical CBOR, or null if absent. */
export async function cardGetEntry(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.cardGetEntry(loomPath, workspace, principal, book, uid, passphrase, kek, authPrincipal, authPassphrase);
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Remove the contact at `uid` in `book`; resolves whether it was present. */
export function cardDeleteEntry(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.cardDeleteEntry(loomPath, workspace, principal, book, uid, passphrase, kek, authPrincipal, authPassphrase);
}

/** List `book` as raw Loom Canonical CBOR (an array of per-contact `ContactEntry` CBOR byte strings). */
export async function cardListEntriesCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.cardListEntries(loomPath, workspace, principal, book, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/**
 * Search `book` by a case-insensitive substring `text` over name, organization, and email; resolves raw
 * Loom Canonical CBOR (an array of per-contact `ContactEntry` CBOR byte strings).
 */
export async function cardSearchCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  text: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.cardSearch(loomPath, workspace, principal, book, text, passphrase, kek, authPrincipal, authPassphrase);
  return Uint8Array.from(bytes);
}

/** The on-demand vCard (`.vcf`) projection of the contact at `uid`, or null if absent. */
export function cardEntryVcard(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string | null> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.cardEntryVcard(loomPath, workspace, principal, book, uid, passphrase, kek, authPrincipal, authPassphrase);
}

/** Parse vCard `vcf` and store it as a record in `book`; resolves the new ETag ("algo:hex"). */
export function cardPutVcard(
  loomPath: string,
  workspace: string,
  principal: string,
  book: string,
  vcf: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.cardPutVcard(loomPath, workspace, principal, book, vcf, passphrase, kek, authPrincipal, authPassphrase);
}
