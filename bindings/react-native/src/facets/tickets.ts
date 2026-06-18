import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

function args(key?: LoomKey, auth?: LoomAuth): [string, number[], string, string] {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return [passphrase, kek, authPrincipal, authPassphrase];
}

export function ticketsProjectCreateJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, keyPrefix: string, name: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsProjectCreateJson(loomPath, workspace, ticketWorkspaceId, projectId, keyPrefix, name, expectedRoot, ...args(key, auth));
}

export function ticketsProjectRekeyJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, keyPrefix: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsProjectRekeyJson(loomPath, workspace, ticketWorkspaceId, projectId, keyPrefix, expectedRoot, ...args(key, auth));
}

export function ticketsProjectSettingsGetJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsProjectSettingsGetJson(loomPath, workspace, ticketWorkspaceId, projectId, ...args(key, auth));
}

export function ticketsProjectSettingsSetJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, defaultProjection: string | null | undefined, enableProjectionsJson: string, disableProjectionsJson: string, actorEnforcement: string | null | undefined, projectOwnerPrincipal: string | null | undefined, clearProjectOwnerPrincipal: boolean, acceptanceAuthoritiesJson: string | null | undefined, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsProjectSettingsSetJson(loomPath, workspace, ticketWorkspaceId, projectId, defaultProjection ?? '', enableProjectionsJson, disableProjectionsJson, actorEnforcement ?? '', projectOwnerPrincipal ?? '', clearProjectOwnerPrincipal, acceptanceAuthoritiesJson ?? '', expectedRoot, ...args(key, auth));
}

export function ticketsFieldsJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, projection = 'native', operation = 'create', key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsFieldsJson(loomPath, workspace, ticketWorkspaceId, projectId, projection, operation, ...args(key, auth));
}

export function ticketsFieldPutJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, fieldId: string, fieldKey: string, name: string, description: string | null | undefined, fieldType: string, optionSet: string | null | undefined, maxLength: number | null | undefined, required: boolean, searchable: boolean, orderable: boolean, cardinality: string, applicableTypeIdsJson: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsFieldPutJson(loomPath, workspace, ticketWorkspaceId, projectId, fieldId, fieldKey, name, description ?? '', fieldType, optionSet ?? '', maxLength ?? 0, maxLength != null, required, searchable, orderable, cardinality, applicableTypeIdsJson, expectedRoot, ...args(key, auth));
}

export function ticketsFieldRetireJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, fieldId: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsFieldRetireJson(loomPath, workspace, ticketWorkspaceId, projectId, fieldId, expectedRoot, ...args(key, auth));
}

export function ticketsCreateJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, ticketType: string, externalSource: string | null | undefined, externalId: string | null | undefined, fieldsJson: string, policyLabelsJson: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsCreateJson(loomPath, workspace, ticketWorkspaceId, projectId, ticketType, externalSource ?? '', externalId ?? '', fieldsJson, policyLabelsJson, expectedRoot, ...args(key, auth));
}

export function ticketsUpdateJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, setFieldsJson: string, deleteFieldsJson: string, action: string | null | undefined, targetStatus: string | null | undefined, observedSourceStatus: string | null | undefined, observedWorkflowVersion: string | null | undefined, assignee: string | null | undefined, expectedRoot: string, key?: LoomKey, auth?: LoomAuth, commentId?: string | null, commentType?: string | null, commentBody?: string | null, commentsJson?: string | null, relationSetsJson?: string | null, relationRemovesJson?: string | null): Promise<string> {
  return UldrenLoom.ticketsUpdateJson(loomPath, workspace, ticketWorkspaceId, ticketId, setFieldsJson, deleteFieldsJson, action ?? '', targetStatus ?? '', observedSourceStatus ?? '', observedWorkflowVersion ?? '', assignee ?? '', expectedRoot, ...args(key, auth), commentId ?? '', commentType ?? '', commentBody ?? '', commentsJson ?? '', relationSetsJson ?? '', relationRemovesJson ?? '');
}

export function ticketsDeleteJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsDeleteJson(loomPath, workspace, ticketWorkspaceId, ticketId, expectedRoot, ...args(key, auth));
}

export function ticketsCommentsJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsCommentsJson(loomPath, workspace, ticketWorkspaceId, ticketId, ...args(key, auth));
}

export function ticketsCommentAddJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string | null | undefined, commentType: string | null | undefined, body: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsCommentAddJson(loomPath, workspace, ticketWorkspaceId, ticketId, commentId ?? '', commentType ?? '', body, expectedRoot, ...args(key, auth));
}

export function ticketsCommentUpdateJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string, commentType: string | null | undefined, body: string | null | undefined, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsCommentUpdateJson(loomPath, workspace, ticketWorkspaceId, ticketId, commentId, commentType ?? '', body ?? '', expectedRoot, ...args(key, auth));
}

export function ticketsCommentDeleteJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsCommentDeleteJson(loomPath, workspace, ticketWorkspaceId, ticketId, commentId, expectedRoot, ...args(key, auth));
}

export function ticketsRelationSetJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, relationId: string, kind: string, targetId: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsRelationSetJson(loomPath, workspace, ticketWorkspaceId, ticketId, relationId, kind, targetId, expectedRoot, ...args(key, auth));
}

export function ticketsRelationRemoveJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, relationId: string, expectedRoot: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsRelationRemoveJson(loomPath, workspace, ticketWorkspaceId, ticketId, relationId, expectedRoot, ...args(key, auth));
}

export function ticketsGetJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, projection = 'native', key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsGetJson(loomPath, workspace, ticketWorkspaceId, ticketId, projection, ...args(key, auth));
}

export function ticketsListJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projection = 'native', key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsListJson(loomPath, workspace, ticketWorkspaceId, projection, ...args(key, auth));
}

export function ticketsHistoryJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.ticketsHistoryJson(loomPath, workspace, ticketWorkspaceId, ticketId, ...args(key, auth));
}
