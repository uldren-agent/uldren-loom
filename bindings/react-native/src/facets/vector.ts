import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** Create vector set `name` of width `dim` and `metric` (1 cosine, 2 L2, 3 dot) in `workspace`
 * (created with the `vector` facet if absent). Conflicts if it already exists. */
export function vectorCreate(
  loomPath: string,
  workspace: string,
  name: string,
  dim: number,
  metric: number,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.vectorCreate(
    loomPath, workspace, name, dim, metric, passphrase, kek, authPrincipal, authPassphrase
  );
}

/** Insert or replace the embedding at `id`: `vector` is little-endian f32 bytes (4 per component);
 * `metadata` is a raw Loom Canonical CBOR map `text -> cell` (empty for none). */
export function vectorUpsert(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  vector: Uint8Array | number[],
  metadata: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.vectorUpsert(
    loomPath,
    workspace,
    name,
    id,
    Array.from(vector),
    Array.from(metadata),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Insert or replace an embedding with UTF-8 source text and optional embedding model profile. */
export function vectorUpsertSource(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  vector: Uint8Array | number[],
  metadata: Uint8Array | number[],
  sourceText: Uint8Array | number[],
  modelId?: string | null,
  weightsDigest?: string | null,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.vectorUpsertSource(
    loomPath,
    workspace,
    name,
    id,
    Array.from(vector),
    Array.from(metadata),
    Array.from(sourceText),
    modelId ?? null,
    weightsDigest ?? null,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch the embedding + metadata at `id` as raw Loom Canonical CBOR `[vector_bytes, metadata]`, or
 * null if absent. */
export async function vectorGet(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.vectorGet(
    loomPath, workspace, name, id, passphrase, kek, authPrincipal, authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Fetch UTF-8 source text bytes for `id`, or null if absent. */
export async function vectorSourceText(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.vectorSourceText(
    loomPath, workspace, name, id, passphrase, kek, authPrincipal, authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Fetch the embedding model profile as raw CBOR `[1, model_id, dimension, weights_digest]`. */
export async function vectorEmbeddingModel(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.vectorEmbeddingModel(
    loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Vector ids in set `name`, sorted ascending, as raw Loom Canonical CBOR text array. */
export async function vectorIds(
  loomPath: string,
  workspace: string,
  name: string,
  prefix?: string | null,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.vectorIds(
      loomPath, workspace, name, prefix ?? null, passphrase, kek, authPrincipal, authPassphrase
    )
  );
}

/** Declared metadata equality index keys in set `name`, sorted ascending, as raw Loom Canonical CBOR
 * text array. */
export async function vectorMetadataIndexKeys(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.vectorMetadataIndexKeys(
      loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase
    )
  );
}

/** Declare and build a metadata equality index for `key`; resolves whether a new index was declared. */
export function vectorCreateMetadataIndex(
  loomPath: string,
  workspace: string,
  name: string,
  key: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.vectorCreateMetadataIndex(
    loomPath, workspace, name, key, passphrase, kek, authPrincipal, authPassphrase
  );
}

/** Drop the metadata equality index for `key`; resolves whether an index was present. */
export function vectorDropMetadataIndex(
  loomPath: string,
  workspace: string,
  name: string,
  key: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.vectorDropMetadataIndex(
    loomPath, workspace, name, key, passphrase, kek, authPrincipal, authPassphrase
  );
}

/** Remove `id` from vector set `name`; resolves whether it was present. */
export function vectorDelete(
  loomPath: string,
  workspace: string,
  name: string,
  id: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.vectorDelete(loomPath, workspace, name, id, passphrase, kek, authPrincipal, authPassphrase);
}

/** Exact top-`k` nearest neighbours of `query` (little-endian f32 bytes) among vectors passing
 * `filter` (raw Loom Canonical CBOR; empty for all), as raw Loom Canonical CBOR (an array of
 * `[id, score_cell]`, highest score first). */
export async function vectorSearchCbor(
  loomPath: string,
  workspace: string,
  name: string,
  query: Uint8Array | number[],
  k: number,
  filter: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.vectorSearch(
      loomPath,
      workspace,
      name,
      Array.from(query),
      k,
      Array.from(filter),
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}

/** Top-`k` nearest neighbours with explicit accelerator policy over built-in PQ. Policy 0 is exact,
 * policy 1 is approximate-above-threshold. Result CBOR matches `vectorSearchCbor`. */
export async function vectorSearchPolicyCbor(
  loomPath: string,
  workspace: string,
  name: string,
  query: Uint8Array | number[],
  k: number,
  filter: Uint8Array | number[],
  policy: number,
  threshold: number,
  ef: number,
  pqM: number,
  pqK: number,
  pqIters: number,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.vectorSearchPolicy(
      loomPath,
      workspace,
      name,
      Array.from(query),
      k,
      Array.from(filter),
      policy,
      threshold,
      ef,
      pqM,
      pqK,
      pqIters,
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}
