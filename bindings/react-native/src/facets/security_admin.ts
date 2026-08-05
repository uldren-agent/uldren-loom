import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export function auditConfigShowJson(loomPath: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.auditConfigShowJson(loomPath, passphrase, kek, authPrincipal, authPassphrase);
}

export function auditConfigSetJson(
  loomPath: string,
  retentionDays?: number | null,
  legalHold?: boolean | null,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.auditConfigSetJson(
    loomPath, retentionDays ?? 0, retentionDays != null, legalHold ?? false, legalHold != null,
    passphrase, kek, authPrincipal, authPassphrase
  );
}

export function auditListJson(loomPath: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.auditListJson(loomPath, passphrase, kek, authPrincipal, authPassphrase);
}

export function auditViewJson(loomPath: string, record: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.auditViewJson(loomPath, record, passphrase, kek, authPrincipal, authPassphrase);
}

export function certificateListJson(loomPath: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.certificateListJson(loomPath, passphrase, kek, authPrincipal, authPassphrase);
}

export function certificateImportJson(
  loomPath: string,
  name: string,
  certChainPem: Uint8Array | number[],
  privateKeyPem: Uint8Array | number[],
  trustBundlePem?: Uint8Array | number[] | null,
  force = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const hasTrustBundlePem = trustBundlePem != null;
  return UldrenLoom.certificateImportJson(
    loomPath, name, Array.from(certChainPem), Array.from(privateKeyPem),
    hasTrustBundlePem ? Array.from(trustBundlePem) : [], hasTrustBundlePem, force, passphrase,
    kek, authPrincipal, authPassphrase
  );
}

export async function certificateExport(
  loomPath: string,
  name: string,
  includeCertChain: boolean,
  includePrivateKey: boolean,
  includeTrustBundle: boolean,
  force = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.certificateExport(
    loomPath, name, includeCertChain, includePrivateKey, includeTrustBundle, force, passphrase,
    kek, authPrincipal, authPassphrase
  );
  return Uint8Array.from(bytes);
}

export function certificateGenerateSelfSignedJson(
  loomPath: string,
  name: string,
  dnsNamesJson: string,
  ipAddressesJson: string,
  cn: string | null | undefined,
  days: number,
  algorithm: string,
  force = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const hasCn = cn != null;
  return UldrenLoom.certificateGenerateSelfSignedJson(
    loomPath, name, dnsNamesJson, ipAddressesJson, hasCn ? cn : '', hasCn, days, algorithm, force,
    passphrase, kek, authPrincipal, authPassphrase
  );
}

export function certificateRemoveJson(loomPath: string, name: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.certificateRemoveJson(loomPath, name, passphrase, kek, authPrincipal, authPassphrase);
}

export function certificateAuditJson(loomPath: string, name: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.certificateAuditJson(loomPath, name, passphrase, kek, authPrincipal, authPassphrase);
}

export function networkAccessListJson(loomPath: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.networkAccessListJson(loomPath, passphrase, kek, authPrincipal, authPassphrase);
}

export function networkAccessSetJson(
  loomPath: string,
  name: string,
  description: string | null | undefined,
  defaultAction: string,
  rulesJson: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const hasDescription = description != null;
  return UldrenLoom.networkAccessSetJson(
    loomPath, name, hasDescription ? description : '', hasDescription, defaultAction, rulesJson,
    passphrase, kek, authPrincipal, authPassphrase
  );
}

export function networkAccessRemoveJson(loomPath: string, name: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.networkAccessRemoveJson(loomPath, name, passphrase, kek, authPrincipal, authPassphrase);
}

export function networkAccessAuditJson(loomPath: string, name: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.networkAccessAuditJson(loomPath, name, passphrase, kek, authPrincipal, authPassphrase);
}
