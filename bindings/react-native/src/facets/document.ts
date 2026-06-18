import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export interface DocumentText {
  text: string;
  digest: string;
  entity_tag: string;
}

export interface DocumentBinary {
  bytes: Uint8Array;
  digest: string;
  entity_tag: string;
}

export interface DocumentPutResult {
  digest: string;
  entity_tag: string;
}

/** Put UTF-8 text document `text` at string `id` and resolve the resulting document tags. */
export function docPutText(
  loomPath: string,
  workspace: string,
  collection: string,
  id: string,
  text: string,
  expectedEntityTag?: string | null,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<DocumentPutResult> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docPutText(
    loomPath,
    workspace,
    collection,
    id,
    text,
    expectedEntityTag ?? '',
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch UTF-8 text document at `id`, or null if absent. */
export function docGetText(
  loomPath: string,
  workspace: string,
  collection: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<DocumentText | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docGetText(
    loomPath,
    workspace,
    collection,
    id,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Put binary document `bytes` at string `id` and resolve the resulting document tags. */
export function docPutBinary(
  loomPath: string,
  workspace: string,
  collection: string,
  id: string,
  bytes: Uint8Array | number[],
  expectedEntityTag?: string | null,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<DocumentPutResult> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docPutBinary(
    loomPath,
    workspace,
    collection,
    id,
    Array.from(bytes),
    expectedEntityTag ?? '',
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch binary document at `id`, or null if absent. */
export async function docGetBinary(
  loomPath: string,
  workspace: string,
  collection: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<DocumentBinary | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const doc = await UldrenLoom.docGetBinary(
    loomPath,
    workspace,
    collection,
    id,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return doc == null ? null : { bytes: Uint8Array.from(doc.bytes), digest: doc.digest };
}

/** Remove `id` from collection `collection`; resolves whether it was present. */
export function docDelete(
  loomPath: string,
  workspace: string,
  collection: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docDelete(
    loomPath,
    workspace,
    collection,
    id,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Collection `collection` of `workspace` as Loom Canonical CBOR binary. */
export async function docListBinary(
  loomPath: string,
  workspace: string,
  collection: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.docListBinary(loomPath, workspace, collection, passphrase, kek, authPrincipal, authPassphrase)
  );
}

export function docIndexCreate(
  loomPath: string,
  workspace: string,
  collection: string,
  name: string,
  fieldPath: string,
  unique = false,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docIndexCreate(
    loomPath,
    workspace,
    collection,
    name,
    fieldPath,
    unique,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function docIndexCreateJson(
  loomPath: string,
  workspace: string,
  collection: string,
  declarationJson: Uint8Array,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docIndexCreateJson(
    loomPath,
    workspace,
    collection,
    Array.from(declarationJson),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function docIndexDrop(
  loomPath: string,
  workspace: string,
  collection: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docIndexDrop(
    loomPath,
    workspace,
    collection,
    name,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function docIndexRebuild(
  loomPath: string,
  workspace: string,
  collection: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docIndexRebuild(
    loomPath,
    workspace,
    collection,
    name,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function docIndexListJson(
  loomPath: string,
  workspace: string,
  collection: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docIndexListJson(
    loomPath,
    workspace,
    collection,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function docIndexStatusJson(
  loomPath: string,
  workspace: string,
  collection: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docIndexStatusJson(
    loomPath,
    workspace,
    collection,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function docFindJson(
  loomPath: string,
  workspace: string,
  collection: string,
  index: string,
  valueJson: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docFindJson(
    loomPath,
    workspace,
    collection,
    index,
    valueJson,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function docQueryJson(
  loomPath: string,
  workspace: string,
  collection: string,
  queryJson: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.docQueryJson(
    loomPath,
    workspace,
    collection,
    queryJson,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}
