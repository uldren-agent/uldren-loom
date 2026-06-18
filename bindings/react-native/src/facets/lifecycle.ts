import UldrenLoom from '../NativeUldrenLoom';

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
