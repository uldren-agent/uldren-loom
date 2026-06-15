import UldrenLoom from './NativeUldrenLoom';

/** The engine version. */
export function version(): string {
  return UldrenLoom.version();
}

/** The content address ("algo:hex", e.g. "blake3:...") of `bytes` as an Uldren Loom blob. */
export function blobDigest(bytes: Uint8Array | number[]): string {
  return UldrenLoom.blobDigest(Array.from(bytes));
}
