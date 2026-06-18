import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** Insert or replace node `id` (props as raw Loom Canonical CBOR `text -> bytes`, empty for none) in
 * graph `name` of `workspace` (created with the `graph` facet if absent). */
export function graphUpsertNode(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  props: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.graphUpsertNode(
    loomPath,
    workspace,
    name,
    id,
    Array.from(props),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch node `id`'s props as raw Loom Canonical CBOR (`text -> bytes`), or null if absent. */
export async function graphGetNode(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.graphGetNode(
    loomPath,
    workspace,
    name,
    id,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Remove node `id` from graph `name`; `cascade` also removes incident edges (else it conflicts while
 * any exist). */
export function graphRemoveNode(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  cascade: boolean,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.graphRemoveNode(
    loomPath,
    workspace,
    name,
    id,
    cascade,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Insert or replace edge `id` from `src` to `dst` (both must exist) with `label` and props as raw
 * Loom Canonical CBOR `text -> bytes` (empty for none). */
export function graphUpsertEdge(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  src: string,
  dst: string,
  label: string,
  props: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.graphUpsertEdge(
    loomPath,
    workspace,
    name,
    id,
    src,
    dst,
    label,
    Array.from(props),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch edge `id` as raw Loom Canonical CBOR `[src, dst, label, props]`, or null if absent. */
export async function graphGetEdge(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.graphGetEdge(
    loomPath,
    workspace,
    name,
    id,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Remove edge `id` from graph `name`; resolves whether it was present. */
export function graphRemoveEdge(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.graphRemoveEdge(
    loomPath,
    workspace,
    name,
    id,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Distinct adjacent node ids of `id` (sorted) as raw Loom Canonical CBOR (an array of text). */
export async function graphNeighborsCbor(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.graphNeighbors(
      loomPath,
      workspace,
      name,
      id,
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}

/** Out-edges of `id` as raw Loom Canonical CBOR (an array of `[edge_id, edge]` in edge-id order). */
export async function graphOutEdgesCbor(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.graphOutEdges(
      loomPath,
      workspace,
      name,
      id,
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}

/** In-edges of `id` as raw Loom Canonical CBOR (an array of `[edge_id, edge]` in edge-id order). */
export async function graphInEdgesCbor(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.graphInEdges(
      loomPath,
      workspace,
      name,
      id,
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}

/** Node ids reachable from `start` as raw Loom Canonical CBOR (an array of text). `maxDepth < 0` is no
 * limit; `viaLabel` null/undefined follows every edge, else only edges with that label. */
export async function graphReachableCbor(
  loomPath: string,
  workspace: string,
  name: string,
  start: string,
  maxDepth: number,
  viaLabel: string | null | undefined,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.graphReachable(
      loomPath,
      workspace,
      name,
      start,
      maxDepth,
      viaLabel ?? null,
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}

/** A shortest path from `from` to `to` as raw Loom Canonical CBOR (an array of node-id text), or null
 * when none exists. `viaLabel` null/undefined follows every edge, else only edges with that label. */
export async function graphShortestPathCbor(
  loomPath: string,
  workspace: string,
  name: string,
  from: string,
  to: string,
  viaLabel: string | null | undefined,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.graphShortestPath(
    loomPath,
    workspace,
    name,
    from,
    to,
    viaLabel ?? null,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}
