import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

type U64 = string;

function args(key?: LoomKey, auth?: LoomAuth): [string, number[], string, string] {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return [passphrase, kek, authPrincipal, authPassphrase];
}

export function driveListJson(loomPath: string, workspace: string, driveWorkspaceId: string, folderId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveListJson(loomPath, workspace, driveWorkspaceId, folderId, ...args(key, auth));
}

export function driveStatJson(loomPath: string, workspace: string, driveWorkspaceId: string, folderId: string, name: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveStatJson(loomPath, workspace, driveWorkspaceId, folderId, name, ...args(key, auth));
}

export async function driveReadFile(loomPath: string, workspace: string, driveWorkspaceId: string, fileId: string, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const bytes = await UldrenLoom.driveReadFile(loomPath, workspace, driveWorkspaceId, fileId, ...args(key, auth));
  return Uint8Array.from(bytes);
}

export function driveListVersionsJson(loomPath: string, workspace: string, driveWorkspaceId: string, fileId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveListVersionsJson(loomPath, workspace, driveWorkspaceId, fileId, ...args(key, auth));
}

export function driveListConflictsJson(loomPath: string, workspace: string, driveWorkspaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveListConflictsJson(loomPath, workspace, driveWorkspaceId, ...args(key, auth));
}

export function driveListSharesJson(loomPath: string, workspace: string, driveWorkspaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveListSharesJson(loomPath, workspace, driveWorkspaceId, ...args(key, auth));
}

export function driveListRetentionJson(loomPath: string, workspace: string, driveWorkspaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveListRetentionJson(loomPath, workspace, driveWorkspaceId, ...args(key, auth));
}

export function driveCreateFolderJson(loomPath: string, workspace: string, driveWorkspaceId: string, parentFolderId: string, folderId: string, name: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveCreateFolderJson(loomPath, workspace, driveWorkspaceId, parentFolderId, folderId, name, expectedRoot, ...args(key, auth));
}

export function driveCreateUploadJson(loomPath: string, workspace: string, driveWorkspaceId: string, uploadId: string, parentFolderId: string, name: string, fileId: string, expectedRoot: string, createdAtMs: U64, replaceFile: boolean, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveCreateUploadJson(loomPath, workspace, driveWorkspaceId, uploadId, parentFolderId, name, fileId, expectedRoot, createdAtMs, replaceFile, ...args(key, auth));
}

export function driveUploadChunkJson(loomPath: string, workspace: string, driveWorkspaceId: string, uploadId: string, chunk: Uint8Array | number[], key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveUploadChunkJson(loomPath, workspace, driveWorkspaceId, uploadId, Array.from(chunk), ...args(key, auth));
}

export function driveCommitUploadJson(loomPath: string, workspace: string, driveWorkspaceId: string, uploadId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveCommitUploadJson(loomPath, workspace, driveWorkspaceId, uploadId, ...args(key, auth));
}

export function driveRenameJson(loomPath: string, workspace: string, driveWorkspaceId: string, folderId: string, nodeId: string, newName: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveRenameJson(loomPath, workspace, driveWorkspaceId, folderId, nodeId, newName, expectedRoot, ...args(key, auth));
}

export function driveMoveJson(loomPath: string, workspace: string, driveWorkspaceId: string, sourceFolderId: string, targetFolderId: string, nodeId: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveMoveJson(loomPath, workspace, driveWorkspaceId, sourceFolderId, targetFolderId, nodeId, expectedRoot, ...args(key, auth));
}

export function driveDeleteJson(loomPath: string, workspace: string, driveWorkspaceId: string, folderId: string, nodeId: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveDeleteJson(loomPath, workspace, driveWorkspaceId, folderId, nodeId, expectedRoot, ...args(key, auth));
}

export function driveResolveConflictJson(loomPath: string, workspace: string, driveWorkspaceId: string, conflictId: string, resolution: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveResolveConflictJson(loomPath, workspace, driveWorkspaceId, conflictId, resolution, ...args(key, auth));
}

export function driveGrantShareJson(loomPath: string, workspace: string, driveWorkspaceId: string, grantId: string, targetKind: string, targetId: string, principal: string, role: string, grantedAtMs: U64, expiresAtMs?: U64 | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveGrantShareJson(loomPath, workspace, driveWorkspaceId, grantId, targetKind, targetId, principal, role, grantedAtMs, expiresAtMs ?? '', ...args(key, auth));
}

export function driveRevokeShareJson(loomPath: string, workspace: string, driveWorkspaceId: string, grantId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveRevokeShareJson(loomPath, workspace, driveWorkspaceId, grantId, ...args(key, auth));
}

export function driveApplyShareExpiryJson(loomPath: string, workspace: string, driveWorkspaceId: string, nowMs: U64, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveApplyShareExpiryJson(loomPath, workspace, driveWorkspaceId, nowMs, ...args(key, auth));
}

export function drivePinRetentionJson(loomPath: string, workspace: string, driveWorkspaceId: string, pinId: string, kind: string, root: string, targetEntityId: string | null | undefined, addedAtMs: U64, expiresAtMs?: U64 | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.drivePinRetentionJson(loomPath, workspace, driveWorkspaceId, pinId, kind, root, targetEntityId ?? '', addedAtMs, expiresAtMs ?? '', ...args(key, auth));
}

export function driveUnpinRetentionJson(loomPath: string, workspace: string, driveWorkspaceId: string, pinId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveUnpinRetentionJson(loomPath, workspace, driveWorkspaceId, pinId, ...args(key, auth));
}

export function driveApplyRetentionJson(loomPath: string, workspace: string, driveWorkspaceId: string, nowMs: U64, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.driveApplyRetentionJson(loomPath, workspace, driveWorkspaceId, nowMs, ...args(key, auth));
}
