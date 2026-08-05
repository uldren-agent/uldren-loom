import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** The engine version. */
export function version(): string {
  return UldrenLoom.version();
}

/** The content address ("algo:hex", e.g. "blake3:...") of `bytes` as an Uldren Loom blob. */
export function blobDigest(bytes: Uint8Array | number[]): string {
  return UldrenLoom.blobDigest(Array.from(bytes));
}

/**
 * Create a fresh `.loom` at `loomPath` under an identity `profile` ("default"/"blake3" or
 * "fips"/"sha256"), optionally encrypted. A non-empty `passphrase` encrypts the store; the DEK is
 * wrapped under it with `suite`, or the profile default when `suite` is omitted;
 * otherwise the store is unencrypted. Rejects on failure (e.g. ALREADY_EXISTS).
 */
export async function create(
  loomPath: string,
  profile: string,
  suite = '',
  passphrase = ''
): Promise<void> {
  return UldrenLoom.create(loomPath, profile, suite, passphrase);
}

/**
 * Create a fresh **encrypted** `.loom` whose DEK is wrapped under a host-supplied 256-bit `kek`.
 * `profile` selects the content-address algorithm and `suite` the object AEAD (profile default when
 * omitted). `kek` must be 32 bytes.
 */
export async function createWithKek(
  loomPath: string,
  profile: string,
  kek: Uint8Array | number[],
  suite = ''
): Promise<void> {
  return UldrenLoom.createWithKek(loomPath, profile, suite, Array.from(kek));
}

/**
 * The capability registry as Loom Canonical CBOR. Handle-free: it reports the bindings layer's static
 * catalog and does not open a loom.
 */
export async function capabilities(): Promise<Uint8Array> {
  return Uint8Array.from(await UldrenLoom.capabilities());
}

/** The runtime provider/profile report as Loom Canonical CBOR. */
export async function runtimeProfile(): Promise<Uint8Array> {
  return Uint8Array.from(await UldrenLoom.runtimeProfile());
}

export async function studioSurfaceCatalogJson(workspace: string, set = 'all'): Promise<string> {
  return UldrenLoom.studioSurfaceCatalogJson(workspace, set);
}

export function lifecycleDefineStandardJson(
  loomPath: string,
  workspace: string,
  kind: string,
  version: string,
  completionPredicateDigest: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.lifecycleDefineStandardJson(
    loomPath,
    workspace,
    kind,
    version,
    completionPredicateDigest,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function lifecycleDefineJson(
  loomPath: string,
  workspace: string,
  definition: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.lifecycleDefineJson(
    loomPath,
    workspace,
    Array.from(definition),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function lifecycleInstantiateJson(
  loomPath: string,
  workspace: string,
  instanceId: string,
  definitionId: string,
  subjectRefsJson: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.lifecycleInstantiateJson(
    loomPath,
    workspace,
    instanceId,
    definitionId,
    subjectRefsJson,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function lifecycleTransitionJson(
  loomPath: string,
  workspace: string,
  instanceId: string,
  transitionId: string,
  toStageId: string,
  actorPrincipalId: string | null | undefined,
  gateEvaluationsJson: string,
  snapshotDigest?: string | null,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.lifecycleTransitionJson(
    loomPath,
    workspace,
    instanceId,
    transitionId,
    toStageId,
    actorPrincipalId ?? null,
    gateEvaluationsJson,
    snapshotDigest ?? null,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function refsReconcileJson(
  loomPath: string,
  workspace: string,
  max: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.refsReconcileJson(
    loomPath,
    workspace,
    max,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}
