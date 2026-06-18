import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** Create search index `name` in `workspace` (created with the `search` facet if absent). `mapping` is
 * raw Loom Canonical CBOR (a map `field -> [type_tag, stored, faceted]`). */
export function searchCreate(
  loomPath: string,
  workspace: string,
  name: string,
  mapping: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.searchCreate(
    loomPath,
    workspace,
    name,
    Array.from(mapping),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Insert or replace the document at `id` (opaque bytes); `doc` is raw Loom Canonical CBOR (a map
 * `field -> value`). */
export function searchIndex(
  loomPath: string,
  workspace: string,
  name: string,
  id: Uint8Array | number[],
  doc: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.searchIndex(
    loomPath,
    workspace,
    name,
    Array.from(id),
    Array.from(doc),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch the document at `id` as raw Loom Canonical CBOR (a map `field -> value`), or null if absent. */
export async function searchGet(
  loomPath: string,
  workspace: string,
  name: string,
  id: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.searchGet(
    loomPath,
    workspace,
    name,
    Array.from(id),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Remove the document at `id` from search index `name`; resolves whether it was present. */
export function searchDelete(
  loomPath: string,
  workspace: string,
  name: string,
  id: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.searchDelete(
    loomPath,
    workspace,
    name,
    Array.from(id),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Document ids of search index `name` as raw Loom Canonical CBOR (an array of byte strings); `prefix`
 * null/undefined returns every id, else only ids under that byte prefix. */
export async function searchIdsCbor(
  loomPath: string,
  workspace: string,
  name: string,
  prefix: Uint8Array | number[] | null | undefined,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const hasPrefix = prefix != null;
  return Uint8Array.from(
    await UldrenLoom.searchIds(
      loomPath,
      workspace,
      name,
      hasPrefix ? Array.from(prefix) : [],
      hasPrefix,
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}

/** Replace the mapping of search index `name` with `mapping` (raw Loom Canonical CBOR, a map
 * `field -> [type_tag, stored, faceted]`). */
export function searchRemap(
  loomPath: string,
  workspace: string,
  name: string,
  mapping: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.searchRemap(
    loomPath,
    workspace,
    name,
    Array.from(mapping),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Run `request` (raw Loom Canonical CBOR `[query, limit, offset]`) against search index `name`, as
 * raw Loom Canonical CBOR (the response). */
export async function searchQueryCbor(
  loomPath: string,
  workspace: string,
  name: string,
  request: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.searchQuery(
      loomPath,
      workspace,
      name,
      Array.from(request),
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}
