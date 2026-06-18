import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export function authenticatePassphrase(
  loomPath: string,
  principal: string,
  principalPassphrase: string,
  key?: LoomKey
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  return UldrenLoom.authenticatePassphrase(loomPath, principal, principalPassphrase, passphrase, kek);
}

export function identityListJson(loomPath: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityListJson(loomPath, passphrase, kek, authPrincipal, authPassphrase);
}

export function identityAddPrincipal(
  loomPath: string,
  principalHandle: string,
  name: string,
  kind = 'user',
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityAddPrincipal(
    loomPath,
    principalHandle,
    name,
    kind,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function identityRenamePrincipalHandle(
  loomPath: string,
  principal: string,
  principalHandle: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityRenamePrincipalHandle(
    loomPath,
    principal,
    principalHandle,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function identitySetPassphrase(
  loomPath: string,
  principal: string,
  principalPassphrase: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identitySetPassphrase(
    loomPath,
    principal,
    principalPassphrase,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function identityRemovePrincipal(
  loomPath: string,
  principal: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityRemovePrincipal(loomPath, principal, passphrase, kek, authPrincipal, authPassphrase);
}

export function identityAssignRole(
  loomPath: string,
  principal: string,
  role: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityAssignRole(loomPath, principal, role, passphrase, kek, authPrincipal, authPassphrase);
}

export function identityRevokeRole(
  loomPath: string,
  principal: string,
  role: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityRevokeRole(loomPath, principal, role, passphrase, kek, authPrincipal, authPassphrase);
}

export function identityCreateExternalCredential(
  loomPath: string,
  principal: string,
  kind: string,
  label: string,
  issuer: string,
  subject: string,
  materialDigest?: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityCreateExternalCredential(
    loomPath,
    principal,
    kind,
    label,
    issuer,
    subject,
    materialDigest ?? '',
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function identityRevokeExternalCredential(
  loomPath: string,
  credential: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityRevokeExternalCredential(
    loomPath,
    credential,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function identityAddPublicKey(
  loomPath: string,
  principal: string,
  label: string,
  algorithm: string,
  publicKeyHex: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityAddPublicKey(
    loomPath,
    principal,
    label,
    algorithm,
    publicKeyHex,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function identityRevokePublicKey(
  loomPath: string,
  publicKey: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.identityRevokePublicKey(loomPath, publicKey, passphrase, kek, authPrincipal, authPassphrase);
}

export function aclListJson(loomPath: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.aclListJson(loomPath, passphrase, kek, authPrincipal, authPassphrase);
}

export function aclGrant(
  loomPath: string,
  effect: number,
  subject: string,
  rightsMask: number,
  workspace = '',
  domain = '',
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.aclGrant(
    loomPath,
    effect,
    subject,
    workspace,
    domain,
    rightsMask,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function aclGrantScoped(
  loomPath: string,
  effect: number,
  subject: string,
  rightsMask: number,
  workspace = '',
  domain = '',
  refGlob = '',
  scopes: string[] = [],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.aclGrantScoped(
    loomPath,
    effect,
    subject,
    workspace,
    domain,
    rightsMask,
    refGlob,
    scopes,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function aclGrantScopedPredicate(
  loomPath: string,
  effect: number,
  subject: string,
  rightsMask: number,
  workspace = '',
  domain = '',
  refGlob = '',
  scopes: string[] = [],
  predicateCel = '',
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.aclGrantScopedPredicate(
    loomPath,
    effect,
    subject,
    workspace,
    domain,
    rightsMask,
    refGlob,
    scopes,
    predicateCel,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function aclRevoke(
  loomPath: string,
  effect: number,
  subject: string,
  rightsMask: number,
  workspace = '',
  domain = '',
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.aclRevoke(
    loomPath,
    effect,
    subject,
    workspace,
    domain,
    rightsMask,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function aclRevokeScoped(
  loomPath: string,
  effect: number,
  subject: string,
  rightsMask: number,
  workspace = '',
  domain = '',
  refGlob = '',
  scopes: string[] = [],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.aclRevokeScoped(
    loomPath,
    effect,
    subject,
    workspace,
    domain,
    rightsMask,
    refGlob,
    scopes,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function aclRevokeScopedPredicate(
  loomPath: string,
  effect: number,
  subject: string,
  rightsMask: number,
  workspace = '',
  domain = '',
  refGlob = '',
  scopes: string[] = [],
  predicateCel = '',
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.aclRevokeScopedPredicate(
    loomPath,
    effect,
    subject,
    workspace,
    domain,
    rightsMask,
    refGlob,
    scopes,
    predicateCel,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function protectedRefListJson(
  loomPath: string,
  workspace: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.protectedRefListJson(loomPath, workspace, passphrase, kek, authPrincipal, authPassphrase);
}

export function protectedRefGetJson(
  loomPath: string,
  workspace: string,
  refName: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.protectedRefGetJson(loomPath, workspace, refName, passphrase, kek, authPrincipal, authPassphrase);
}

export function protectedRefSet(
  loomPath: string,
  workspace: string,
  refName: string,
  fastForwardOnly: boolean,
  signedCommitsRequired: boolean,
  signedRefAdvanceRequired: boolean,
  requiredReviewCount: number,
  retentionLock: boolean,
  governanceLock: boolean,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.protectedRefSet(
    loomPath,
    workspace,
    refName,
    fastForwardOnly,
    signedCommitsRequired,
    signedRefAdvanceRequired,
    requiredReviewCount,
    retentionLock,
    governanceLock,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export function protectedRefRemove(
  loomPath: string,
  workspace: string,
  refName: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.protectedRefRemove(loomPath, workspace, refName, passphrase, kek, authPrincipal, authPassphrase);
}
