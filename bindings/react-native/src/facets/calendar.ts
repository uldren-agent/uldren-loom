import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/**
 * Create (or replace the metadata of) calendar collection `collection` under `principal` in `workspace`
 * (UUID or name, created with the `calendar` facet if absent). `components` is a comma-separated component
 * set ("event,todo"; "" is the empty set).
 */
export function calCreateCollection(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  displayName: string,
  components: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.calCreateCollection(
    loomPath,
    workspace,
    principal,
    collection,
    displayName,
    components,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Delete calendar collection `collection` under `principal` and its entries; resolves whether it existed. */
export function calDeleteCollection(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.calDeleteCollection(loomPath, workspace, principal, collection, passphrase, kek, authPrincipal, authPassphrase);
}

/** The calendar collection ids under `principal` as raw Loom Canonical CBOR (an array of text strings). */
export async function calListCollectionsCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.calListCollections(loomPath, workspace, principal, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** Put the calendar `entry` (its `CalendarEntry` canonical CBOR) into `collection`, keyed by its UID. */
export function calPutEntry(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  entry: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.calPutEntry(
    loomPath,
    workspace,
    principal,
    collection,
    Array.from(entry),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch the calendar entry at `uid` in `collection` as its `CalendarEntry` canonical CBOR, or null. */
export async function calGetEntry(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.calGetEntry(loomPath, workspace, principal, collection, uid, passphrase, kek, authPrincipal, authPassphrase);
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Remove the calendar entry at `uid` in `collection`; resolves whether it was present. */
export function calDeleteEntry(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.calDeleteEntry(loomPath, workspace, principal, collection, uid, passphrase, kek, authPrincipal, authPassphrase);
}

/** List `collection` as raw Loom Canonical CBOR (an array of per-entry `CalendarEntry` CBOR byte strings). */
export async function calListEntriesCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.calListEntries(loomPath, workspace, principal, collection, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/**
 * Expand `collection` into occurrences within `[from, to)` (both `YYYYMMDDTHHMMSS`) as raw Loom Canonical
 * CBOR (an array of `[uid, "YYYYMMDDTHHMMSS"]` pairs).
 */
export async function calRangeCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  from: string,
  to: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.calRange(loomPath, workspace, principal, collection, from, to, passphrase, kek, authPrincipal, authPassphrase);
  return Uint8Array.from(bytes);
}

/**
 * Search `collection` by `component` ("" any, "event", or "todo") and case-insensitive `text`; resolves
 * raw Loom Canonical CBOR (an array of per-entry `CalendarEntry` CBOR byte strings).
 */
export async function calSearchCbor(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  component: string,
  text: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.calSearch(
    loomPath,
    workspace,
    principal,
    collection,
    component,
    text,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

/** The on-demand iCalendar (`.ics`) projection of the entry at `uid`, or null if absent. */
export function calEntryIcs(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  uid: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string | null> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.calEntryIcs(loomPath, workspace, principal, collection, uid, passphrase, kek, authPrincipal, authPassphrase);
}

/** Parse iCalendar `ics` and store it as a record in `collection`; resolves the new ETag ("algo:hex"). */
export function calPutIcs(
  loomPath: string,
  workspace: string,
  principal: string,
  collection: string,
  ics: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.calPutIcs(loomPath, workspace, principal, collection, ics, passphrase, kek, authPrincipal, authPassphrase);
}
