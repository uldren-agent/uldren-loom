import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, jsSafeU64, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export function importTableCsv(loomPath: string, workspace: string, sourceScope: string, csvPayload: Uint8Array | number[], database: string, table: string, schema: string, primaryKey: string, mode: string, commit: boolean, author?: string, message?: string, dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importTableCsv(loomPath, workspace, sourceScope, Array.from(csvPayload), database, table, schema, primaryKey, mode, commit, author ?? null, message ?? null, dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function importRedmine(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array | number[], fieldPolicy: string, dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importRedmine(loomPath, workspace, profile, sourceScope, Array.from(snapshotPayload), fieldPolicy, dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function importAsana(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array | number[], fieldPolicy: string, dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importAsana(loomPath, workspace, profile, sourceScope, Array.from(snapshotPayload), fieldPolicy, dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function importJira(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array | number[], fieldPolicy: string, dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importJira(loomPath, workspace, profile, sourceScope, Array.from(snapshotPayload), fieldPolicy, dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function importConfluence(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array | number[], defaultSpace: string, dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importConfluence(loomPath, workspace, profile, sourceScope, Array.from(snapshotPayload), defaultSpace, dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function importSlack(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array | number[], dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importSlack(loomPath, workspace, profile, sourceScope, Array.from(snapshotPayload), dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function importDrive(loomPath: string, workspace: string, profile: string, sourceScope: string, archivePayload: Uint8Array | number[], dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importDrive(loomPath, workspace, profile, sourceScope, Array.from(archivePayload), dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function importMarkdown(loomPath: string, workspace: string, profile: string, sourceScope: string, archivePayload: Uint8Array | number[], space: string, dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importMarkdown(loomPath, workspace, profile, sourceScope, Array.from(archivePayload), space, dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function importNotion(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array | number[], defaultSpace: string, dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.importNotion(loomPath, workspace, profile, sourceScope, Array.from(snapshotPayload), defaultSpace, dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function studioReindexJson(loomPath: string, workspace: string, profile: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.studioReindexJson(loomPath, workspace, profile, passphrase, kek, authPrincipal, authPassphrase);
}
export function studioRevisionsRebuildJson(loomPath: string, workspace: string, profile: string, dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.studioRevisionsRebuildJson(loomPath, workspace, profile, dryRun, passphrase, kek, authPrincipal, authPassphrase);
}
export function storeBundleImport(loomPath: string, bundle: Uint8Array | number[], dryRun = false, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.storeBundleImport(loomPath, Array.from(bundle), dryRun, passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function auditCompact(loomPath: string, throughSeq: number, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.auditCompact(loomPath, jsSafeU64(throughSeq, 'throughSeq'), passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function storeMaintenanceStatus(loomPath: string, request: Uint8Array | number[], key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.storeMaintenanceStatus(loomPath, Array.from(request), passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function storeMaintenancePolicySet(loomPath: string, update: Uint8Array | number[], key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.storeMaintenancePolicySet(loomPath, Array.from(update), passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function storeMaintenanceRun(loomPath: string, request: Uint8Array | number[], key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.storeMaintenanceRun(loomPath, Array.from(request), passphrase, kek, authPrincipal, authPassphrase).then((bytes) => Uint8Array.from(bytes));
}
export function inferenceInstanceCreateJson(loomPath: string, workspace: string, name: string, model: string, kind: string, runtime: string, preset?: string, settingsJson?: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.inferenceInstanceCreateJson(loomPath, workspace, name, model, kind, runtime, preset ?? null, settingsJson ?? null, passphrase, kek, authPrincipal, authPassphrase);
}
export function inferenceInstanceUpdateJson(loomPath: string, workspace: string, name: string, preset?: string, settingsJson?: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.inferenceInstanceUpdateJson(loomPath, workspace, name, preset ?? null, settingsJson ?? null, passphrase, kek, authPrincipal, authPassphrase);
}
export function inferenceInstanceDeleteJson(loomPath: string, workspace: string, name: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.inferenceInstanceDeleteJson(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase);
}
export function serveListenerConfigureJson(loomPath: string, requestJson: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.serveListenerConfigureJson(loomPath, requestJson, passphrase, kek, authPrincipal, authPassphrase);
}
export function serveListenerListJson(loomPath: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.serveListenerListJson(loomPath, passphrase, kek, authPrincipal, authPassphrase);
}
export function serveListenerSetEnabledJson(loomPath: string, listenerId: string, enabled: boolean, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.serveListenerSetEnabledJson(loomPath, listenerId, enabled, passphrase, kek, authPrincipal, authPassphrase);
}
export function serveListenerRemoveJson(loomPath: string, listenerId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.serveListenerRemoveJson(loomPath, listenerId, passphrase, kek, authPrincipal, authPassphrase);
}
export function serveWebRouteListJson(loomPath: string, listenerId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.serveWebRouteListJson(loomPath, listenerId, passphrase, kek, authPrincipal, authPassphrase);
}
export function serveWebRouteSetJson(loomPath: string, requestJson: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.serveWebRouteSetJson(loomPath, requestJson, passphrase, kek, authPrincipal, authPassphrase);
}
export function serveWebRouteRemoveJson(loomPath: string, listenerId: string, routeId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.serveWebRouteRemoveJson(loomPath, listenerId, routeId, passphrase, kek, authPrincipal, authPassphrase);
}



















