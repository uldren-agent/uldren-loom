import type { LoomAuth, LoomKey, LoomStatement } from './internal';
import {
  casGet,
  casHas,
  casList,
  casPut,
  casDelete,
} from './facets/cas';
import {
  meetingsImportSnapshot,
  meetingsSourceRead,
} from './facets/meetings';
import {
  driveApplyRetentionJson,
  driveApplyShareExpiryJson,
  driveCommitUploadJson,
  driveCreateFolderJson,
  driveCreateUploadJson,
  driveDeleteJson,
  driveGrantShareJson,
  driveListConflictsJson,
  driveListJson,
  driveListRetentionJson,
  driveListSharesJson,
  driveListVersionsJson,
  driveMoveJson,
  drivePinRetentionJson,
  driveReadFile,
  driveRenameJson,
  driveResolveConflictJson,
  driveRevokeShareJson,
  driveStatJson,
  driveUnpinRetentionJson,
  driveUploadChunkJson,
} from './facets/drive';
import {
  ticketsCommentAddJson,
  ticketsCommentDeleteJson,
  ticketsCommentUpdateJson,
  ticketsCommentsJson,
  ticketsCreateJson,
  ticketsDeleteJson,
  ticketsFieldPutJson,
  ticketsFieldRetireJson,
  ticketsFieldsJson,
  ticketsGetJson,
  ticketsHistoryJson,
  ticketsListJson,
  ticketsProjectCreateJson,
  ticketsProjectRekeyJson,
  ticketsProjectSettingsGetJson,
  ticketsProjectSettingsSetJson,
  ticketsRelationRemoveJson,
  ticketsRelationSetJson,
  ticketsUpdateJson,
} from './facets/tickets';
import {
  pagesCreateJson,
  pagesGetJson,
  pagesHistoryJson,
  pagesListJson,
  pagesPublishJson,
  pagesUpdateJson,
  spacesCreateJson,
  spacesGetJson,
  spacesListJson,
  structuresAddNodeJson,
  structuresBindJson,
  structuresCreateJson,
  structuresDecomposeToTicketsJson,
  structuresGetJson,
  structuresLinkNodeJson,
  structuresListJson,
  structuresMoveNodeJson,
  structuresUpdateNodeJson,
} from './facets/pages';
import {
  lanesCreate,
  lanesGet,
  lanesList,
  lanesUpdate,
  lanesTicketAdd,
  lanesTicketRemove,
} from './facets/lanes';
import {
  chatAddReactionJson,
  chatAgentReplyJson,
  chatClaimTaskJson,
  chatCompleteTaskJson,
  chatCreateChannelJson,
  chatCreateTaskJson,
  chatCreateThreadJson,
  chatCursorJson,
  chatEditMessageJson,
  chatEmojiListJson,
  chatEmojiRegisterJson,
  chatEmojiUnregisterJson,
  chatFetchEventsJson,
  chatInvokeAgentJson,
  chatListChannelsJson,
  chatMessagesJson,
  chatPostMessageJson,
  chatRedactMessageJson,
  chatRemoveReactionJson,
  chatRenameChannelJson,
  chatRequestHandoffJson,
  chatUpdateCursorJson,
} from './facets/chat';
import {
  archiveExport,
  archiveImport,
  carExport,
  carImport,
  fsExport,
  fsImport,
} from './facets/archive';
import {
  workspaceCreate,
  workspaceDelete,
  workspaceListJson,
  workspaceRename,
} from './facets/workspace';
import {
  queueAppend,
  queueConsumerAdvance,
  queueConsumerPosition,
  queueConsumerReadCbor,
  queueConsumerReset,
  queueGet,
  queueLen,
  queueRangeCbor,
} from './facets/queue';
import {
  vectorCreate,
  vectorCreateMetadataIndex,
  vectorDelete,
  vectorDropMetadataIndex,
  vectorEmbeddingModel,
  vectorGet,
  vectorIds,
  vectorMetadataIndexKeys,
  vectorSearchCbor,
  vectorSearchPolicyCbor,
  vectorSourceText,
  vectorUpsert,
  vectorUpsertSource,
} from './facets/vector';
import {
  graphGetEdge,
  graphGetNode,
  graphInEdgesCbor,
  graphNeighborsCbor,
  graphOutEdgesCbor,
  graphReachableCbor,
  graphRemoveEdge,
  graphRemoveNode,
  graphShortestPathCbor,
  graphUpsertEdge,
  graphUpsertNode,
} from './facets/graph';
import {
  columnarAggregateCbor,
  columnarAppend,
  columnarColumnsCbor,
  columnarCompact,
  columnarCreate,
  columnarInspectCbor,
  columnarRows,
  columnarScanCbor,
  columnarSelectCbor,
  columnarSourceDigestCbor,
} from './facets/columnar';
import {
  dataframeCollectCbor,
  dataframeCreate,
  dataframeMaterialize,
  dataframePlanDigest,
  dataframePreviewCbor,
  dataframeSourceDigestsCbor,
} from './facets/dataframe';
import {
  searchCreate,
  searchDelete,
  searchGet,
  searchIdsCbor,
  searchIndex,
  searchQueryCbor,
  searchRemap,
} from './facets/search';
import {
  vcsBlameCbor,
  vcsDiffCbor,
  watchSubscribe,
  watchPollCbor,
} from './facets/vcs';
import {
  sqlBlameCbor,
  sqlBatch,
  sqlDiffCbor,
  sqlExec,
  sqlExecBytes,
  sqlExecJson,
  sqlIndexScanAtCbor,
  sqlIndexScanCbor,
  sqlQueryBytes,
  sqlReadTableAtCbor,
  sqlReadTableCbor,
  sqlCommit,
  sqlTableDiffCbor,
} from './facets/sql';
import {
  calCreateCollection,
  calDeleteCollection,
  calDeleteEntry,
  calEntryIcs,
  calGetEntry,
  calListCollectionsCbor,
  calListEntriesCbor,
  calPutEntry,
  calPutIcs,
  calRangeCbor,
  calSearchCbor,
} from './facets/calendar';
import {
  cardCreateBook,
  cardDeleteBook,
  cardDeleteEntry,
  cardEntryVcard,
  cardGetEntry,
  cardListBooksCbor,
  cardListEntriesCbor,
  cardPutEntry,
  cardPutVcard,
  cardSearchCbor,
} from './facets/contacts';
import {
  mailCreateMailbox,
  mailDeleteMailbox,
  mailDeleteMessage,
  mailGetFlagsCbor,
  mailGetMessage,
  mailIngestMessage,
  mailListMailboxesCbor,
  mailListMessagesCbor,
  mailSearchCbor,
  mailSetFlags,
  mailToEml,
} from './facets/mail';
import {
  kvDelete,
  kvGet,
  kvListCbor,
  kvPut,
  kvRangeCbor,
} from './facets/kv';
import {
  type DocumentBinary,
  type DocumentPutResult,
  type DocumentText,
  docDelete,
  docFindJson,
  docGetBinary,
  docGetText,
  docIndexCreate,
  docIndexCreateJson,
  docIndexDrop,
  docIndexListJson,
  docIndexRebuild,
  docIndexStatusJson,
  docListBinary,
  docPutBinary,
  docPutText,
  docQueryJson,
} from './facets/document';
import {
  tsGet,
  tsLatest,
  tsPut,
  tsRangeCbor,
} from './facets/timeseries';
import {
  logsGetRecord,
  logsPutRecord,
  logsQuery,
  metricsGetDescriptor,
  metricsPutDescriptor,
  metricsPutObservation,
  metricsQuery,
  tracesGetSpan,
  tracesPutSpan,
  tracesQuery,
  tracesTraceSpans,
} from './facets/telemetry';
import {
  ledgerAppend,
  ledgerGet,
  ledgerHead,
  ledgerLen,
  ledgerVerify,
} from './facets/ledger';
import {
  execCbor,
} from './facets/execution';
import {
  aclGrant,
  aclGrantScoped,
  aclGrantScopedPredicate,
  aclListJson,
  aclRevoke,
  aclRevokeScoped,
  aclRevokeScopedPredicate,
  authenticatePassphrase,
  identityAddPrincipal,
  identityAssignRole,
  identityCreateExternalCredential,
  identityListJson,
  identityRenamePrincipalHandle,
  identityRemovePrincipal,
  identityRevokeExternalCredential,
  identityRevokeRole,
  identitySetPassphrase,
  protectedRefGetJson,
  protectedRefListJson,
  protectedRefRemove,
  protectedRefSet,
} from './facets/identity';
import type { QueueSeq } from './internal';

export class LoomSession {
  private auth?: LoomAuth;

  constructor(readonly loomPath: string, readonly key?: LoomKey) {}

  static open(loomPath: string, key?: LoomKey): LoomSession {
    return new LoomSession(loomPath, key);
  }

  async authenticatePassphrase(principal: string, principalPassphrase: string): Promise<void> {
    await authenticatePassphrase(this.loomPath, principal, principalPassphrase, this.key);
    this.auth = { principal, passphrase: principalPassphrase };
  }

  clearAuthentication(): void {
    this.auth = undefined;
  }

  identityListJson(): Promise<string> {
    return identityListJson(this.loomPath, this.key, this.auth);
  }

  identityAddPrincipal(handle: string, name: string, kind = 'user'): Promise<string> {
    return identityAddPrincipal(this.loomPath, handle, name, kind, this.key, this.auth);
  }

  identityRenamePrincipalHandle(principal: string, handle: string): Promise<void> {
    return identityRenamePrincipalHandle(this.loomPath, principal, handle, this.key, this.auth);
  }

  identitySetPassphrase(principal: string, principalPassphrase: string): Promise<void> {
    return identitySetPassphrase(this.loomPath, principal, principalPassphrase, this.key, this.auth);
  }

  identityRemovePrincipal(principal: string): Promise<void> {
    return identityRemovePrincipal(this.loomPath, principal, this.key, this.auth);
  }

  identityAssignRole(principal: string, role: string): Promise<void> {
    return identityAssignRole(this.loomPath, principal, role, this.key, this.auth);
  }

  identityRevokeRole(principal: string, role: string): Promise<boolean> {
    return identityRevokeRole(this.loomPath, principal, role, this.key, this.auth);
  }

  identityCreateExternalCredential(
    principal: string,
    kind: string,
    label: string,
    issuer: string,
    subject: string,
    materialDigest?: string
  ): Promise<string> {
    return identityCreateExternalCredential(
      this.loomPath,
      principal,
      kind,
      label,
      issuer,
      subject,
      materialDigest,
      this.key,
      this.auth
    );
  }

  identityRevokeExternalCredential(credential: string): Promise<void> {
    return identityRevokeExternalCredential(this.loomPath, credential, this.key, this.auth);
  }

  aclListJson(): Promise<string> {
    return aclListJson(this.loomPath, this.key, this.auth);
  }

  aclGrant(
    effect: number,
    subject: string,
    rightsMask: number,
    workspace = '',
    domain = ''
  ): Promise<void> {
    return aclGrant(this.loomPath, effect, subject, rightsMask, workspace, domain, this.key, this.auth);
  }

  aclGrantScoped(
    effect: number,
    subject: string,
    rightsMask: number,
    workspace = '',
    domain = '',
    refGlob = '',
    scopes: string[] = []
  ): Promise<void> {
    return aclGrantScoped(
      this.loomPath,
      effect,
      subject,
      rightsMask,
      workspace,
      domain,
      refGlob,
      scopes,
      this.key,
      this.auth
    );
  }

  aclGrantScopedPredicate(
    effect: number,
    subject: string,
    rightsMask: number,
    workspace = '',
    domain = '',
    refGlob = '',
    scopes: string[] = [],
    predicateCel = ''
  ): Promise<void> {
    return aclGrantScopedPredicate(
      this.loomPath,
      effect,
      subject,
      rightsMask,
      workspace,
      domain,
      refGlob,
      scopes,
      predicateCel,
      this.key,
      this.auth
    );
  }

  aclRevoke(
    effect: number,
    subject: string,
    rightsMask: number,
    workspace = '',
    domain = ''
  ): Promise<boolean> {
    return aclRevoke(this.loomPath, effect, subject, rightsMask, workspace, domain, this.key, this.auth);
  }

  aclRevokeScoped(
    effect: number,
    subject: string,
    rightsMask: number,
    workspace = '',
    domain = '',
    refGlob = '',
    scopes: string[] = []
  ): Promise<boolean> {
    return aclRevokeScoped(
      this.loomPath,
      effect,
      subject,
      rightsMask,
      workspace,
      domain,
      refGlob,
      scopes,
      this.key,
      this.auth
    );
  }

  aclRevokeScopedPredicate(
    effect: number,
    subject: string,
    rightsMask: number,
    workspace = '',
    domain = '',
    refGlob = '',
    scopes: string[] = [],
    predicateCel = ''
  ): Promise<boolean> {
    return aclRevokeScopedPredicate(
      this.loomPath,
      effect,
      subject,
      rightsMask,
      workspace,
      domain,
      refGlob,
      scopes,
      predicateCel,
      this.key,
      this.auth
    );
  }

  protectedRefListJson(workspace: string): Promise<string> {
    return protectedRefListJson(this.loomPath, workspace, this.key, this.auth);
  }

  protectedRefGetJson(workspace: string, refName: string): Promise<string> {
    return protectedRefGetJson(this.loomPath, workspace, refName, this.key, this.auth);
  }

  protectedRefSet(
    workspace: string,
    refName: string,
    fastForwardOnly: boolean,
    signedCommitsRequired: boolean,
    signedRefAdvanceRequired: boolean,
    requiredReviewCount: number,
    retentionLock: boolean,
    governanceLock: boolean
  ): Promise<void> {
    return protectedRefSet(
      this.loomPath,
      workspace,
      refName,
      fastForwardOnly,
      signedCommitsRequired,
      signedRefAdvanceRequired,
      requiredReviewCount,
      retentionLock,
      governanceLock,
      this.key,
      this.auth
    );
  }

  protectedRefRemove(workspace: string, refName: string): Promise<boolean> {
    return protectedRefRemove(this.loomPath, workspace, refName, this.key, this.auth);
  }

  workspaceCreate(name = '', facet = ''): Promise<string> {
    return workspaceCreate(this.loomPath, name, facet, this.key, this.auth);
  }

  workspaceListJson(): Promise<string> {
    return workspaceListJson(this.loomPath, this.key, this.auth);
  }

  workspaceRename(workspace: string, newName: string): Promise<void> {
    return workspaceRename(this.loomPath, workspace, newName, this.key, this.auth);
  }

  workspaceDelete(workspace: string): Promise<void> {
    return workspaceDelete(this.loomPath, workspace, this.key, this.auth);
  }

  casPut(workspace: string, content: Uint8Array | number[]): Promise<string> {
    return casPut(this.loomPath, workspace, content, this.key, this.auth);
  }

  casGet(workspace: string, digest: string): Promise<Uint8Array | null> {
    return casGet(this.loomPath, workspace, digest, this.key, this.auth);
  }

  casHas(workspace: string, digest: string): Promise<boolean> {
    return casHas(this.loomPath, workspace, digest, this.key, this.auth);
  }

  casList(workspace: string): Promise<string[]> {
    return casList(this.loomPath, workspace, this.key, this.auth);
  }

  casDelete(workspace: string, digest: string): Promise<boolean> {
    return casDelete(this.loomPath, workspace, digest, this.key, this.auth);
  }

  meetingsImportSnapshot(
    workspace: string,
    inputProfile: string,
    snapshot: Uint8Array | number[],
    dryRun = false
  ): Promise<string> {
    return meetingsImportSnapshot(this.loomPath, workspace, inputProfile, snapshot, dryRun, this.key, this.auth);
  }

  meetingsSourceRead(workspace: string, sourceId: string, leaf: string): Promise<Uint8Array> {
    return meetingsSourceRead(this.loomPath, workspace, sourceId, leaf, this.key, this.auth);
  }

  driveListJson(workspace: string, driveWorkspaceId: string, folderId: string): Promise<string> {
    return driveListJson(this.loomPath, workspace, driveWorkspaceId, folderId, this.key, this.auth);
  }

  driveStatJson(workspace: string, driveWorkspaceId: string, folderId: string, name: string): Promise<string> {
    return driveStatJson(this.loomPath, workspace, driveWorkspaceId, folderId, name, this.key, this.auth);
  }

  driveReadFile(workspace: string, driveWorkspaceId: string, fileId: string): Promise<Uint8Array> {
    return driveReadFile(this.loomPath, workspace, driveWorkspaceId, fileId, this.key, this.auth);
  }

  driveListVersionsJson(workspace: string, driveWorkspaceId: string, fileId: string): Promise<string> {
    return driveListVersionsJson(this.loomPath, workspace, driveWorkspaceId, fileId, this.key, this.auth);
  }

  driveListConflictsJson(workspace: string, driveWorkspaceId: string): Promise<string> {
    return driveListConflictsJson(this.loomPath, workspace, driveWorkspaceId, this.key, this.auth);
  }

  driveListSharesJson(workspace: string, driveWorkspaceId: string): Promise<string> {
    return driveListSharesJson(this.loomPath, workspace, driveWorkspaceId, this.key, this.auth);
  }

  driveListRetentionJson(workspace: string, driveWorkspaceId: string): Promise<string> {
    return driveListRetentionJson(this.loomPath, workspace, driveWorkspaceId, this.key, this.auth);
  }

  driveCreateFolderJson(workspace: string, driveWorkspaceId: string, parentFolderId: string, folderId: string, name: string, expectedRoot: string): Promise<string> {
    return driveCreateFolderJson(this.loomPath, workspace, driveWorkspaceId, parentFolderId, folderId, name, expectedRoot, this.key, this.auth);
  }

  driveCreateUploadJson(workspace: string, driveWorkspaceId: string, uploadId: string, parentFolderId: string, name: string, fileId: string, expectedRoot: string, createdAtMs: string, replaceFile: boolean): Promise<string> {
    return driveCreateUploadJson(this.loomPath, workspace, driveWorkspaceId, uploadId, parentFolderId, name, fileId, expectedRoot, createdAtMs, replaceFile, this.key, this.auth);
  }

  driveUploadChunkJson(workspace: string, driveWorkspaceId: string, uploadId: string, chunk: Uint8Array | number[]): Promise<string> {
    return driveUploadChunkJson(this.loomPath, workspace, driveWorkspaceId, uploadId, chunk, this.key, this.auth);
  }

  driveCommitUploadJson(workspace: string, driveWorkspaceId: string, uploadId: string): Promise<string> {
    return driveCommitUploadJson(this.loomPath, workspace, driveWorkspaceId, uploadId, this.key, this.auth);
  }

  driveRenameJson(workspace: string, driveWorkspaceId: string, folderId: string, nodeId: string, newName: string, expectedRoot: string): Promise<string> {
    return driveRenameJson(this.loomPath, workspace, driveWorkspaceId, folderId, nodeId, newName, expectedRoot, this.key, this.auth);
  }

  driveMoveJson(workspace: string, driveWorkspaceId: string, sourceFolderId: string, targetFolderId: string, nodeId: string, expectedRoot: string): Promise<string> {
    return driveMoveJson(this.loomPath, workspace, driveWorkspaceId, sourceFolderId, targetFolderId, nodeId, expectedRoot, this.key, this.auth);
  }

  driveDeleteJson(workspace: string, driveWorkspaceId: string, folderId: string, nodeId: string, expectedRoot: string): Promise<string> {
    return driveDeleteJson(this.loomPath, workspace, driveWorkspaceId, folderId, nodeId, expectedRoot, this.key, this.auth);
  }

  driveResolveConflictJson(workspace: string, driveWorkspaceId: string, conflictId: string, resolution: string): Promise<string> {
    return driveResolveConflictJson(this.loomPath, workspace, driveWorkspaceId, conflictId, resolution, this.key, this.auth);
  }

  driveGrantShareJson(workspace: string, driveWorkspaceId: string, grantId: string, targetKind: string, targetId: string, principal: string, role: string, grantedAtMs: string, expiresAtMs?: string | null): Promise<string> {
    return driveGrantShareJson(this.loomPath, workspace, driveWorkspaceId, grantId, targetKind, targetId, principal, role, grantedAtMs, expiresAtMs, this.key, this.auth);
  }

  driveRevokeShareJson(workspace: string, driveWorkspaceId: string, grantId: string): Promise<string> {
    return driveRevokeShareJson(this.loomPath, workspace, driveWorkspaceId, grantId, this.key, this.auth);
  }

  driveApplyShareExpiryJson(workspace: string, driveWorkspaceId: string, nowMs: string): Promise<string> {
    return driveApplyShareExpiryJson(this.loomPath, workspace, driveWorkspaceId, nowMs, this.key, this.auth);
  }

  drivePinRetentionJson(workspace: string, driveWorkspaceId: string, pinId: string, kind: string, root: string, targetEntityId: string | null | undefined, addedAtMs: string, expiresAtMs?: string | null): Promise<string> {
    return drivePinRetentionJson(this.loomPath, workspace, driveWorkspaceId, pinId, kind, root, targetEntityId, addedAtMs, expiresAtMs, this.key, this.auth);
  }

  driveUnpinRetentionJson(workspace: string, driveWorkspaceId: string, pinId: string): Promise<string> {
    return driveUnpinRetentionJson(this.loomPath, workspace, driveWorkspaceId, pinId, this.key, this.auth);
  }

  driveApplyRetentionJson(workspace: string, driveWorkspaceId: string, nowMs: string): Promise<string> {
    return driveApplyRetentionJson(this.loomPath, workspace, driveWorkspaceId, nowMs, this.key, this.auth);
  }

  ticketsProjectCreateJson(workspace: string, ticketWorkspaceId: string, projectId: string, keyPrefix: string, name: string, expectedRoot: string): Promise<string> {
    return ticketsProjectCreateJson(this.loomPath, workspace, ticketWorkspaceId, projectId, keyPrefix, name, expectedRoot, this.key, this.auth);
  }

  ticketsProjectRekeyJson(workspace: string, ticketWorkspaceId: string, projectId: string, keyPrefix: string, expectedRoot: string): Promise<string> {
    return ticketsProjectRekeyJson(this.loomPath, workspace, ticketWorkspaceId, projectId, keyPrefix, expectedRoot, this.key, this.auth);
  }

  ticketsProjectSettingsGetJson(workspace: string, ticketWorkspaceId: string, projectId: string): Promise<string> {
    return ticketsProjectSettingsGetJson(this.loomPath, workspace, ticketWorkspaceId, projectId, this.key, this.auth);
  }

  ticketsProjectSettingsSetJson(workspace: string, ticketWorkspaceId: string, projectId: string, defaultProjection: string | null | undefined, enableProjectionsJson: string, disableProjectionsJson: string, actorEnforcement: string | null | undefined, projectOwnerPrincipal: string | null | undefined, clearProjectOwnerPrincipal: boolean, acceptanceAuthoritiesJson: string | null | undefined, expectedRoot: string): Promise<string> {
    return ticketsProjectSettingsSetJson(this.loomPath, workspace, ticketWorkspaceId, projectId, defaultProjection, enableProjectionsJson, disableProjectionsJson, actorEnforcement, projectOwnerPrincipal, clearProjectOwnerPrincipal, acceptanceAuthoritiesJson, expectedRoot, this.key, this.auth);
  }

  ticketsFieldsJson(workspace: string, ticketWorkspaceId: string, projectId: string, projection = 'native', operation = 'create'): Promise<string> {
    return ticketsFieldsJson(this.loomPath, workspace, ticketWorkspaceId, projectId, projection, operation, this.key, this.auth);
  }

  ticketsFieldPutJson(workspace: string, ticketWorkspaceId: string, projectId: string, fieldId: string, fieldKey: string, name: string, description: string | null | undefined, fieldType: string, optionSet: string | null | undefined, maxLength: number | null | undefined, required: boolean, searchable: boolean, orderable: boolean, cardinality: string, applicableTypeIdsJson: string, expectedRoot: string): Promise<string> {
    return ticketsFieldPutJson(this.loomPath, workspace, ticketWorkspaceId, projectId, fieldId, fieldKey, name, description, fieldType, optionSet, maxLength, required, searchable, orderable, cardinality, applicableTypeIdsJson, expectedRoot, this.key, this.auth);
  }

  ticketsFieldRetireJson(workspace: string, ticketWorkspaceId: string, projectId: string, fieldId: string, expectedRoot: string): Promise<string> {
    return ticketsFieldRetireJson(this.loomPath, workspace, ticketWorkspaceId, projectId, fieldId, expectedRoot, this.key, this.auth);
  }

  ticketsCreateJson(workspace: string, ticketWorkspaceId: string, projectId: string, ticketType: string, externalSource: string | null | undefined, externalId: string | null | undefined, fieldsJson: string, policyLabelsJson: string, expectedRoot: string): Promise<string> {
    return ticketsCreateJson(this.loomPath, workspace, ticketWorkspaceId, projectId, ticketType, externalSource, externalId, fieldsJson, policyLabelsJson, expectedRoot, this.key, this.auth);
  }

  ticketsUpdateJson(workspace: string, ticketWorkspaceId: string, ticketId: string, setFieldsJson: string, deleteFieldsJson: string, action: string | null | undefined, targetStatus: string | null | undefined, observedSourceStatus: string | null | undefined, observedWorkflowVersion: string | null | undefined, assignee: string | null | undefined, expectedRoot: string, commentId?: string | null, commentType?: string | null, commentBody?: string | null, commentsJson?: string | null, relationSetsJson?: string | null, relationRemovesJson?: string | null): Promise<string> {
    return ticketsUpdateJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, setFieldsJson, deleteFieldsJson, action, targetStatus, observedSourceStatus, observedWorkflowVersion, assignee, expectedRoot, this.key, this.auth, commentId, commentType, commentBody, commentsJson, relationSetsJson, relationRemovesJson);
  }

  ticketsDeleteJson(workspace: string, ticketWorkspaceId: string, ticketId: string, expectedRoot: string): Promise<string> {
    return ticketsDeleteJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, expectedRoot, this.key, this.auth);
  }

  ticketsCommentsJson(workspace: string, ticketWorkspaceId: string, ticketId: string): Promise<string> {
    return ticketsCommentsJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, this.key, this.auth);
  }

  ticketsCommentAddJson(workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string | null | undefined, commentType: string | null | undefined, body: string, expectedRoot: string): Promise<string> {
    return ticketsCommentAddJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, commentId, commentType, body, expectedRoot, this.key, this.auth);
  }

  ticketsCommentUpdateJson(workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string, commentType: string | null | undefined, body: string | null | undefined, expectedRoot: string): Promise<string> {
    return ticketsCommentUpdateJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, commentId, commentType, body, expectedRoot, this.key, this.auth);
  }

  ticketsCommentDeleteJson(workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string, expectedRoot: string): Promise<string> {
    return ticketsCommentDeleteJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, commentId, expectedRoot, this.key, this.auth);
  }

  ticketsRelationSetJson(workspace: string, ticketWorkspaceId: string, ticketId: string, relationId: string, kind: string, targetId: string, expectedRoot: string): Promise<string> {
    return ticketsRelationSetJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, relationId, kind, targetId, expectedRoot, this.key, this.auth);
  }

  ticketsRelationRemoveJson(workspace: string, ticketWorkspaceId: string, ticketId: string, relationId: string, expectedRoot: string): Promise<string> {
    return ticketsRelationRemoveJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, relationId, expectedRoot, this.key, this.auth);
  }

  ticketsGetJson(workspace: string, ticketWorkspaceId: string, ticketId: string, projection = 'native'): Promise<string> {
    return ticketsGetJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, projection, this.key, this.auth);
  }

  ticketsListJson(workspace: string, ticketWorkspaceId: string, projection = 'native'): Promise<string> {
    return ticketsListJson(this.loomPath, workspace, ticketWorkspaceId, projection, this.key, this.auth);
  }

  ticketsHistoryJson(workspace: string, ticketWorkspaceId: string, ticketId: string): Promise<string> {
    return ticketsHistoryJson(this.loomPath, workspace, ticketWorkspaceId, ticketId, this.key, this.auth);
  }

  spacesCreateJson(workspace: string, pageWorkspaceId: string, spaceId: string, title: string, expectedRoot?: string | null): Promise<string> {
    return spacesCreateJson(this.loomPath, workspace, pageWorkspaceId, spaceId, title, expectedRoot, this.key, this.auth);
  }

  spacesListJson(workspace: string, pageWorkspaceId: string): Promise<string> {
    return spacesListJson(this.loomPath, workspace, pageWorkspaceId, this.key, this.auth);
  }

  spacesGetJson(workspace: string, pageWorkspaceId: string, spaceId: string): Promise<string> {
    return spacesGetJson(this.loomPath, workspace, pageWorkspaceId, spaceId, this.key, this.auth);
  }

  pagesCreateJson(workspace: string, pageWorkspaceId: string, pageId: string, spaceId: string, parentPageId: string | null | undefined, title: string, expectedRoot?: string | null): Promise<string> {
    return pagesCreateJson(this.loomPath, workspace, pageWorkspaceId, pageId, spaceId, parentPageId, title, expectedRoot, this.key, this.auth);
  }

  pagesUpdateJson(workspace: string, pageWorkspaceId: string, pageId: string, bodyText: string, expectedRoot?: string | null): Promise<string> {
    return pagesUpdateJson(this.loomPath, workspace, pageWorkspaceId, pageId, bodyText, expectedRoot, this.key, this.auth);
  }

  pagesPublishJson(workspace: string, pageWorkspaceId: string, pageId: string, expectedRoot?: string | null): Promise<string> {
    return pagesPublishJson(this.loomPath, workspace, pageWorkspaceId, pageId, expectedRoot, this.key, this.auth);
  }

  pagesGetJson(workspace: string, pageWorkspaceId: string, pageId: string): Promise<string> {
    return pagesGetJson(this.loomPath, workspace, pageWorkspaceId, pageId, this.key, this.auth);
  }

  pagesListJson(workspace: string, pageWorkspaceId: string): Promise<string> {
    return pagesListJson(this.loomPath, workspace, pageWorkspaceId, this.key, this.auth);
  }

  pagesHistoryJson(workspace: string, pageWorkspaceId: string, pageId: string): Promise<string> {
    return pagesHistoryJson(this.loomPath, workspace, pageWorkspaceId, pageId, this.key, this.auth);
  }

  structuresCreateJson(workspace: string, pageWorkspaceId: string, structureId: string, spaceId: string, kind: string, title: string, expectedRoot?: string | null): Promise<string> {
    return structuresCreateJson(this.loomPath, workspace, pageWorkspaceId, structureId, spaceId, kind, title, expectedRoot, this.key, this.auth);
  }

  structuresAddNodeJson(workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, kind: string, label: string, bodyDigest?: string | null, entityRef?: string | null, expectedRoot?: string | null): Promise<string> {
    return structuresAddNodeJson(this.loomPath, workspace, pageWorkspaceId, structureId, nodeId, kind, label, bodyDigest, entityRef, expectedRoot, this.key, this.auth);
  }

  structuresUpdateNodeJson(workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, kind: string, label: string, bodyDigest?: string | null, entityRef?: string | null, expectedRoot?: string | null): Promise<string> {
    return structuresUpdateNodeJson(this.loomPath, workspace, pageWorkspaceId, structureId, nodeId, kind, label, bodyDigest, entityRef, expectedRoot, this.key, this.auth);
  }

  structuresBindJson(workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, entityRef?: string | null, expectedRoot?: string | null): Promise<string> {
    return structuresBindJson(this.loomPath, workspace, pageWorkspaceId, structureId, nodeId, entityRef, expectedRoot, this.key, this.auth);
  }

  structuresMoveNodeJson(workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, parentNodeId?: string | null, label?: string | null, expectedRoot?: string | null): Promise<string> {
    return structuresMoveNodeJson(this.loomPath, workspace, pageWorkspaceId, structureId, nodeId, parentNodeId, label, expectedRoot, this.key, this.auth);
  }

  structuresLinkNodeJson(workspace: string, pageWorkspaceId: string, structureId: string, edgeId: string, srcNodeId: string, dstNodeId: string, label: string, targetRef?: string | null, expectedRoot?: string | null): Promise<string> {
    return structuresLinkNodeJson(this.loomPath, workspace, pageWorkspaceId, structureId, edgeId, srcNodeId, dstNodeId, label, targetRef, expectedRoot, this.key, this.auth);
  }

  structuresDecomposeToTicketsJson(workspace: string, pageWorkspaceId: string, structureId: string, itemsJson: string): Promise<string> {
    return structuresDecomposeToTicketsJson(this.loomPath, workspace, pageWorkspaceId, structureId, itemsJson, this.key, this.auth);
  }

  structuresGetJson(workspace: string, pageWorkspaceId: string, structureId: string): Promise<string> {
    return structuresGetJson(this.loomPath, workspace, pageWorkspaceId, structureId, this.key, this.auth);
  }

  structuresListJson(workspace: string, pageWorkspaceId: string): Promise<string> {
    return structuresListJson(this.loomPath, workspace, pageWorkspaceId, this.key, this.auth);
  }

  lanesCreate(workspace: string, lane: Uint8Array | number[]): Promise<Uint8Array> {
    return lanesCreate(this.loomPath, workspace, lane, this.key, this.auth);
  }

  lanesGet(workspace: string, laneId: string): Promise<Uint8Array | null> {
    return lanesGet(this.loomPath, workspace, laneId, this.key, this.auth);
  }

  lanesList(workspace: string): Promise<Uint8Array> {
    return lanesList(this.loomPath, workspace, this.key, this.auth);
  }

  lanesUpdate(workspace: string, laneId: string, fields: { title?: string | null; description?: string | null; laneStatus?: string | null; statusReport?: string | null; reviewerFeedback?: string | null }, updatedBy: string): Promise<Uint8Array> {
    return lanesUpdate(this.loomPath, workspace, laneId, fields, updatedBy, this.key, this.auth);
  }

  lanesTicketAdd(workspace: string, laneId: string, ticketId: string, updatedBy: string, placement: string = 'append', anchor?: string | null): Promise<Uint8Array> {
    return lanesTicketAdd(this.loomPath, workspace, laneId, ticketId, updatedBy, placement, anchor, this.key, this.auth);
  }

  lanesTicketRemove(workspace: string, laneId: string, ticketId: string, updatedBy: string): Promise<Uint8Array> {
    return lanesTicketRemove(this.loomPath, workspace, laneId, ticketId, updatedBy, this.key, this.auth);
  }

  chatCreateChannelJson(workspace: string, chatWorkspaceId: string, channelId: string, channelHandle: string, name: string): Promise<string> {
    return chatCreateChannelJson(this.loomPath, workspace, chatWorkspaceId, channelId, channelHandle, name, this.key, this.auth);
  }

  chatRenameChannelJson(workspace: string, chatWorkspaceId: string, selector: string, channelHandle: string): Promise<string> {
    return chatRenameChannelJson(this.loomPath, workspace, chatWorkspaceId, selector, channelHandle, this.key, this.auth);
  }

  chatListChannelsJson(workspace: string, chatWorkspaceId: string): Promise<string> {
    return chatListChannelsJson(this.loomPath, workspace, chatWorkspaceId, this.key, this.auth);
  }

  chatPostMessageJson(workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, threadId: string | null | undefined, bodyText: string): Promise<string> {
    return chatPostMessageJson(this.loomPath, workspace, chatWorkspaceId, channelId, messageId, threadId, bodyText, this.key, this.auth);
  }

  chatEditMessageJson(workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, bodyText: string): Promise<string> {
    return chatEditMessageJson(this.loomPath, workspace, chatWorkspaceId, channelId, messageId, bodyText, this.key, this.auth);
  }

  chatRedactMessageJson(workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, reason?: string | null): Promise<string> {
    return chatRedactMessageJson(this.loomPath, workspace, chatWorkspaceId, channelId, messageId, reason, this.key, this.auth);
  }

  chatCreateThreadJson(workspace: string, chatWorkspaceId: string, channelId: string, threadId: string, parentMessageId: string): Promise<string> {
    return chatCreateThreadJson(this.loomPath, workspace, chatWorkspaceId, channelId, threadId, parentMessageId, this.key, this.auth);
  }

  chatCreateTaskJson(workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, messageId: string, title: string): Promise<string> {
    return chatCreateTaskJson(this.loomPath, workspace, chatWorkspaceId, channelId, taskId, messageId, title, this.key, this.auth);
  }

  chatClaimTaskJson(workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, claimId: string, leaseToken?: string | null): Promise<string> {
    return chatClaimTaskJson(this.loomPath, workspace, chatWorkspaceId, channelId, taskId, claimId, leaseToken, this.key, this.auth);
  }

  chatCompleteTaskJson(workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, claimId: string, resultMessageId?: string | null): Promise<string> {
    return chatCompleteTaskJson(this.loomPath, workspace, chatWorkspaceId, channelId, taskId, claimId, resultMessageId, this.key, this.auth);
  }

  chatInvokeAgentJson(workspace: string, chatWorkspaceId: string, channelId: string, invocationId: string, agentPrincipal: string, sourceMessageIdsJson: string, promptText: string): Promise<string> {
    return chatInvokeAgentJson(this.loomPath, workspace, chatWorkspaceId, channelId, invocationId, agentPrincipal, sourceMessageIdsJson, promptText, this.key, this.auth);
  }

  chatAgentReplyJson(workspace: string, chatWorkspaceId: string, channelId: string, invocationId: string, messageId: string): Promise<string> {
    return chatAgentReplyJson(this.loomPath, workspace, chatWorkspaceId, channelId, invocationId, messageId, this.key, this.auth);
  }

  chatRequestHandoffJson(workspace: string, chatWorkspaceId: string, channelId: string, handoffId: string, fromAgentPrincipal: string, toPrincipal?: string | null, reason?: string | null): Promise<string> {
    return chatRequestHandoffJson(this.loomPath, workspace, chatWorkspaceId, channelId, handoffId, fromAgentPrincipal, toPrincipal, reason, this.key, this.auth);
  }

  chatAddReactionJson(workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, kind: string): Promise<string> {
    return chatAddReactionJson(this.loomPath, workspace, chatWorkspaceId, channelId, messageId, kind, this.key, this.auth);
  }

  chatRemoveReactionJson(workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, kind: string): Promise<string> {
    return chatRemoveReactionJson(this.loomPath, workspace, chatWorkspaceId, channelId, messageId, kind, this.key, this.auth);
  }

  chatEmojiListJson(workspace: string, chatWorkspaceId: string): Promise<string> {
    return chatEmojiListJson(this.loomPath, workspace, chatWorkspaceId, this.key, this.auth);
  }

  chatEmojiRegisterJson(workspace: string, chatWorkspaceId: string, kind: string): Promise<string> {
    return chatEmojiRegisterJson(this.loomPath, workspace, chatWorkspaceId, kind, this.key, this.auth);
  }

  chatEmojiUnregisterJson(workspace: string, chatWorkspaceId: string, kind: string): Promise<string> {
    return chatEmojiUnregisterJson(this.loomPath, workspace, chatWorkspaceId, kind, this.key, this.auth);
  }

  chatMessagesJson(workspace: string, chatWorkspaceId: string, channelId: string): Promise<string> {
    return chatMessagesJson(this.loomPath, workspace, chatWorkspaceId, channelId, this.key, this.auth);
  }

  chatCursorJson(workspace: string, chatWorkspaceId: string, channelId: string): Promise<string> {
    return chatCursorJson(this.loomPath, workspace, chatWorkspaceId, channelId, this.key, this.auth);
  }

  chatUpdateCursorJson(workspace: string, chatWorkspaceId: string, channelId: string, nextSequence: string): Promise<string> {
    return chatUpdateCursorJson(this.loomPath, workspace, chatWorkspaceId, channelId, nextSequence, this.key, this.auth);
  }

  chatFetchEventsJson(workspace: string, chatWorkspaceId: string, channelId: string, fromSequence: string, max: string): Promise<string> {
    return chatFetchEventsJson(this.loomPath, workspace, chatWorkspaceId, channelId, fromSequence, max, this.key, this.auth);
  }

  fsImport(workspace: string, srcPath: string, commit = false, dryRun = false): Promise<Uint8Array> {
    return fsImport(this.loomPath, workspace, srcPath, commit, dryRun, this.key, this.auth);
  }

  fsExport(
    workspace: string,
    dstPath: string,
    revision?: string | null,
    dryRun = false
  ): Promise<Uint8Array> {
    return fsExport(this.loomPath, workspace, dstPath, revision, dryRun, this.key, this.auth);
  }

  archiveImport(workspace: string, srcPath: string, kind: string, dryRun = false): Promise<Uint8Array> {
    return archiveImport(this.loomPath, workspace, srcPath, kind, dryRun, this.key, this.auth);
  }

  archiveExport(
    workspace: string,
    dstPath: string,
    kind: string,
    revision?: string | null,
    dryRun = false
  ): Promise<Uint8Array> {
    return archiveExport(this.loomPath, workspace, dstPath, kind, revision, dryRun, this.key, this.auth);
  }

  carImport(srcPath: string, dryRun = false): Promise<Uint8Array> {
    return carImport(this.loomPath, srcPath, dryRun, this.key, this.auth);
  }

  carExport(workspace: string, dstPath: string, dryRun = false): Promise<Uint8Array> {
    return carExport(this.loomPath, workspace, dstPath, dryRun, this.key, this.auth);
  }

  queueAppend(workspace: string, stream: string, entry: Uint8Array | number[]): Promise<QueueSeq> {
    return queueAppend(this.loomPath, workspace, stream, entry, this.key, this.auth);
  }

  queueGet(workspace: string, stream: string, seq: QueueSeq): Promise<Uint8Array | null> {
    return queueGet(this.loomPath, workspace, stream, seq, this.key, this.auth);
  }

  queueRangeCbor(workspace: string, stream: string, lo: QueueSeq, hi: QueueSeq): Promise<Uint8Array> {
    return queueRangeCbor(this.loomPath, workspace, stream, lo, hi, this.key, this.auth);
  }

  queueLen(workspace: string, stream: string): Promise<QueueSeq> {
    return queueLen(this.loomPath, workspace, stream, this.key, this.auth);
  }

  queueConsumerPosition(workspace: string, stream: string, consumerId: string): Promise<QueueSeq> {
    return queueConsumerPosition(this.loomPath, workspace, stream, consumerId, this.key, this.auth);
  }

  queueConsumerReadCbor(workspace: string, stream: string, consumerId: string, max: number): Promise<Uint8Array> {
    return queueConsumerReadCbor(this.loomPath, workspace, stream, consumerId, max, this.key, this.auth);
  }

  queueConsumerAdvance(workspace: string, stream: string, consumerId: string, nextSeq: QueueSeq): Promise<void> {
    return queueConsumerAdvance(this.loomPath, workspace, stream, consumerId, nextSeq, this.key, this.auth);
  }

  queueConsumerReset(workspace: string, stream: string, consumerId: string, nextSeq: QueueSeq): Promise<void> {
    return queueConsumerReset(this.loomPath, workspace, stream, consumerId, nextSeq, this.key, this.auth);
  }

  kvPut(workspace: string, collection: string, key: Uint8Array | number[], value: Uint8Array | number[]): Promise<void> {
    return kvPut(this.loomPath, workspace, collection, key, value, this.key, this.auth);
  }

  kvGet(workspace: string, collection: string, key: Uint8Array | number[]): Promise<Uint8Array | null> {
    return kvGet(this.loomPath, workspace, collection, key, this.key, this.auth);
  }

  kvDelete(workspace: string, collection: string, key: Uint8Array | number[]): Promise<boolean> {
    return kvDelete(this.loomPath, workspace, collection, key, this.key, this.auth);
  }

  kvListCbor(workspace: string, collection: string): Promise<Uint8Array> {
    return kvListCbor(this.loomPath, workspace, collection, this.key, this.auth);
  }

  kvRangeCbor(
    workspace: string,
    collection: string,
    lo: Uint8Array | number[],
    hi: Uint8Array | number[]
  ): Promise<Uint8Array> {
    return kvRangeCbor(this.loomPath, workspace, collection, lo, hi, this.key, this.auth);
  }

  docPutText(
    workspace: string,
    collection: string,
    id: string,
    text: string,
    expectedEntityTag?: string | null
  ): Promise<DocumentPutResult> {
    return docPutText(this.loomPath, workspace, collection, id, text, expectedEntityTag, this.key, this.auth);
  }

  docGetText(workspace: string, collection: string, id: string): Promise<DocumentText | null> {
    return docGetText(this.loomPath, workspace, collection, id, this.key, this.auth);
  }

  docPutBinary(
    workspace: string,
    collection: string,
    id: string,
    bytes: Uint8Array | number[],
    expectedEntityTag?: string | null
  ): Promise<DocumentPutResult> {
    return docPutBinary(this.loomPath, workspace, collection, id, bytes, expectedEntityTag, this.key, this.auth);
  }

  docGetBinary(workspace: string, collection: string, id: string): Promise<DocumentBinary | null> {
    return docGetBinary(this.loomPath, workspace, collection, id, this.key, this.auth);
  }

  docDelete(workspace: string, collection: string, id: string): Promise<boolean> {
    return docDelete(this.loomPath, workspace, collection, id, this.key, this.auth);
  }

  docListBinary(workspace: string, collection: string): Promise<Uint8Array> {
    return docListBinary(this.loomPath, workspace, collection, this.key, this.auth);
  }

  docIndexCreate(
    workspace: string,
    collection: string,
    name: string,
    fieldPath: string,
    unique = false
  ): Promise<void> {
    return docIndexCreate(this.loomPath, workspace, collection, name, fieldPath, unique, this.key, this.auth);
  }

  docIndexCreateJson(workspace: string, collection: string, declarationJson: Uint8Array): Promise<void> {
    return docIndexCreateJson(this.loomPath, workspace, collection, declarationJson, this.key, this.auth);
  }

  docIndexDrop(workspace: string, collection: string, name: string): Promise<boolean> {
    return docIndexDrop(this.loomPath, workspace, collection, name, this.key, this.auth);
  }

  docIndexRebuild(workspace: string, collection: string, name: string): Promise<void> {
    return docIndexRebuild(this.loomPath, workspace, collection, name, this.key, this.auth);
  }

  docIndexListJson(workspace: string, collection: string): Promise<string> {
    return docIndexListJson(this.loomPath, workspace, collection, this.key, this.auth);
  }

  docIndexStatusJson(workspace: string, collection: string): Promise<string> {
    return docIndexStatusJson(this.loomPath, workspace, collection, this.key, this.auth);
  }

  docFindJson(workspace: string, collection: string, index: string, valueJson: string): Promise<string> {
    return docFindJson(this.loomPath, workspace, collection, index, valueJson, this.key, this.auth);
  }

  docQueryJson(workspace: string, collection: string, queryJson: string): Promise<string> {
    return docQueryJson(this.loomPath, workspace, collection, queryJson, this.key, this.auth);
  }

  tsPut(workspace: string, collection: string, ts: number | bigint | string, value: Uint8Array | number[]): Promise<void> {
    return tsPut(this.loomPath, workspace, collection, ts, value, this.key, this.auth);
  }

  tsGet(workspace: string, collection: string, ts: number | bigint | string): Promise<Uint8Array | null> {
    return tsGet(this.loomPath, workspace, collection, ts, this.key, this.auth);
  }

  tsRangeCbor(
    workspace: string,
    collection: string,
    from: number | bigint | string,
    to: number | bigint | string
  ): Promise<Uint8Array> {
    return tsRangeCbor(this.loomPath, workspace, collection, from, to, this.key, this.auth);
  }

  tsLatest(workspace: string, collection: string): Promise<{ ts: string; value: Uint8Array } | null> {
    return tsLatest(this.loomPath, workspace, collection, this.key, this.auth);
  }

  metricsPutDescriptor(workspace: string, descriptor: Uint8Array | number[]): Promise<void> {
    return metricsPutDescriptor(this.loomPath, workspace, descriptor, this.key, this.auth);
  }

  metricsGetDescriptor(workspace: string, name: string): Promise<Uint8Array | null> {
    return metricsGetDescriptor(this.loomPath, workspace, name, this.key, this.auth);
  }

  metricsPutObservation(
    workspace: string,
    descriptorName: string,
    observation: Uint8Array | number[]
  ): Promise<void> {
    return metricsPutObservation(this.loomPath, workspace, descriptorName, observation, this.key, this.auth);
  }

  metricsQuery(
    workspace: string,
    descriptorName: string,
    fromTimestampMs: number | bigint | string,
    toTimestampMs: number | bigint | string,
    maxSeries: number,
    maxGroups: number,
    maxSamples: number,
    maxOutputBytes: number | bigint | string,
    nowTimestampMs: number | bigint | string
  ): Promise<Uint8Array> {
    return metricsQuery(
      this.loomPath,
      workspace,
      descriptorName,
      fromTimestampMs,
      toTimestampMs,
      maxSeries,
      maxGroups,
      maxSamples,
      maxOutputBytes,
      nowTimestampMs,
      this.key,
      this.auth
    );
  }

  logsPutRecord(workspace: string, record: Uint8Array | number[]): Promise<string> {
    return logsPutRecord(this.loomPath, workspace, record, this.key, this.auth);
  }

  logsGetRecord(workspace: string, recordId: string): Promise<Uint8Array | null> {
    return logsGetRecord(this.loomPath, workspace, recordId, this.key, this.auth);
  }

  logsQuery(
    workspace: string,
    fromTimeUnixNano: number | bigint | string,
    toTimeUnixNano: number | bigint | string,
    maxRecords: number,
    maxOutputBytes: number | bigint | string
  ): Promise<Uint8Array> {
    return logsQuery(
      this.loomPath,
      workspace,
      fromTimeUnixNano,
      toTimeUnixNano,
      maxRecords,
      maxOutputBytes,
      this.key,
      this.auth
    );
  }

  tracesPutSpan(workspace: string, span: Uint8Array | number[]): Promise<void> {
    return tracesPutSpan(this.loomPath, workspace, span, this.key, this.auth);
  }

  tracesGetSpan(workspace: string, traceId: string, spanId: string): Promise<Uint8Array | null> {
    return tracesGetSpan(this.loomPath, workspace, traceId, spanId, this.key, this.auth);
  }

  tracesTraceSpans(
    workspace: string,
    traceId: string,
    maxSpans: number,
    maxOutputBytes: number | bigint | string
  ): Promise<Uint8Array> {
    return tracesTraceSpans(this.loomPath, workspace, traceId, maxSpans, maxOutputBytes, this.key, this.auth);
  }

  tracesQuery(
    workspace: string,
    fromStartTimeNs: number | bigint | string,
    toStartTimeNs: number | bigint | string,
    maxSpans: number,
    maxOutputBytes: number | bigint | string
  ): Promise<Uint8Array> {
    return tracesQuery(
      this.loomPath,
      workspace,
      fromStartTimeNs,
      toStartTimeNs,
      maxSpans,
      maxOutputBytes,
      this.key,
      this.auth
    );
  }

  ledgerAppend(workspace: string, collection: string, payload: Uint8Array | number[]): Promise<string> {
    return ledgerAppend(this.loomPath, workspace, collection, payload, this.key, this.auth);
  }

  ledgerGet(workspace: string, collection: string, seq: number | bigint | string): Promise<Uint8Array | null> {
    return ledgerGet(this.loomPath, workspace, collection, seq, this.key, this.auth);
  }

  ledgerHead(workspace: string, collection: string): Promise<string | null> {
    return ledgerHead(this.loomPath, workspace, collection, this.key, this.auth);
  }

  ledgerLen(workspace: string, collection: string): Promise<string> {
    return ledgerLen(this.loomPath, workspace, collection, this.key, this.auth);
  }

  ledgerVerify(workspace: string, collection: string): Promise<void> {
    return ledgerVerify(this.loomPath, workspace, collection, this.key, this.auth);
  }

  execCbor(request: Uint8Array | number[]): Promise<Uint8Array> {
    return execCbor(this.loomPath, request, this.key, this.auth);
  }

  vectorCreate(workspace: string, name: string, dim: number, metric: number): Promise<void> {
    return vectorCreate(this.loomPath, workspace, name, dim, metric, this.key, this.auth);
  }

  vectorUpsert(
    workspace: string,
    name: string,
    id: string,
    vector: Uint8Array | number[],
    metadata: Uint8Array | number[]
  ): Promise<void> {
    return vectorUpsert(this.loomPath, workspace, name, id, vector, metadata, this.key, this.auth);
  }

  vectorUpsertSource(
    workspace: string,
    name: string,
    id: string,
    vector: Uint8Array | number[],
    metadata: Uint8Array | number[],
    sourceText: Uint8Array | number[],
    modelId?: string | null,
    weightsDigest?: string | null
  ): Promise<void> {
    return vectorUpsertSource(
      this.loomPath,
      workspace,
      name,
      id,
      vector,
      metadata,
      sourceText,
      modelId,
      weightsDigest,
      this.key,
      this.auth
    );
  }

  vectorGet(workspace: string, name: string, id: string): Promise<Uint8Array | null> {
    return vectorGet(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  vectorSourceText(workspace: string, name: string, id: string): Promise<Uint8Array | null> {
    return vectorSourceText(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  vectorEmbeddingModel(workspace: string, name: string): Promise<Uint8Array | null> {
    return vectorEmbeddingModel(this.loomPath, workspace, name, this.key, this.auth);
  }

  vectorIds(workspace: string, name: string, prefix?: string | null): Promise<Uint8Array> {
    return vectorIds(this.loomPath, workspace, name, prefix, this.key, this.auth);
  }

  vectorMetadataIndexKeys(workspace: string, name: string): Promise<Uint8Array> {
    return vectorMetadataIndexKeys(this.loomPath, workspace, name, this.key, this.auth);
  }

  vectorCreateMetadataIndex(workspace: string, name: string, key: string): Promise<boolean> {
    return vectorCreateMetadataIndex(this.loomPath, workspace, name, key, this.key, this.auth);
  }

  vectorDropMetadataIndex(workspace: string, name: string, key: string): Promise<boolean> {
    return vectorDropMetadataIndex(this.loomPath, workspace, name, key, this.key, this.auth);
  }

  vectorDelete(workspace: string, name: string, id: string): Promise<boolean> {
    return vectorDelete(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  vectorSearchCbor(
    workspace: string,
    name: string,
    query: Uint8Array | number[],
    k: number,
    filter: Uint8Array | number[]
  ): Promise<Uint8Array> {
    return vectorSearchCbor(this.loomPath, workspace, name, query, k, filter, this.key, this.auth);
  }

  vectorSearchPolicyCbor(
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
    pqIters: number
  ): Promise<Uint8Array> {
    return vectorSearchPolicyCbor(
      this.loomPath,
      workspace,
      name,
      query,
      k,
      filter,
      policy,
      threshold,
      ef,
      pqM,
      pqK,
      pqIters,
      this.key,
      this.auth
    );
  }

  graphUpsertNode(workspace: string, name: string, id: string, props: Uint8Array | number[]): Promise<void> {
    return graphUpsertNode(this.loomPath, workspace, name, id, props, this.key, this.auth);
  }

  graphGetNode(workspace: string, name: string, id: string): Promise<Uint8Array | null> {
    return graphGetNode(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  graphRemoveNode(workspace: string, name: string, id: string, cascade: boolean): Promise<void> {
    return graphRemoveNode(this.loomPath, workspace, name, id, cascade, this.key, this.auth);
  }

  graphUpsertEdge(
    workspace: string,
    name: string,
    id: string,
    src: string,
    dst: string,
    label: string,
    props: Uint8Array | number[]
  ): Promise<void> {
    return graphUpsertEdge(this.loomPath, workspace, name, id, src, dst, label, props, this.key, this.auth);
  }

  graphGetEdge(workspace: string, name: string, id: string): Promise<Uint8Array | null> {
    return graphGetEdge(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  graphRemoveEdge(workspace: string, name: string, id: string): Promise<boolean> {
    return graphRemoveEdge(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  graphNeighborsCbor(workspace: string, name: string, id: string): Promise<Uint8Array> {
    return graphNeighborsCbor(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  graphOutEdgesCbor(workspace: string, name: string, id: string): Promise<Uint8Array> {
    return graphOutEdgesCbor(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  graphInEdgesCbor(workspace: string, name: string, id: string): Promise<Uint8Array> {
    return graphInEdgesCbor(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  graphReachableCbor(
    workspace: string,
    name: string,
    start: string,
    maxDepth: number,
    viaLabel?: string | null
  ): Promise<Uint8Array> {
    return graphReachableCbor(this.loomPath, workspace, name, start, maxDepth, viaLabel, this.key, this.auth);
  }

  graphShortestPathCbor(
    workspace: string,
    name: string,
    from: string,
    to: string,
    viaLabel?: string | null
  ): Promise<Uint8Array | null> {
    return graphShortestPathCbor(this.loomPath, workspace, name, from, to, viaLabel, this.key, this.auth);
  }

  columnarCreate(
    workspace: string,
    name: string,
    columns: Uint8Array | number[],
    targetSegmentRows: number
  ): Promise<void> {
    return columnarCreate(this.loomPath, workspace, name, columns, targetSegmentRows, this.key, this.auth);
  }

  columnarAppend(workspace: string, name: string, row: Uint8Array | number[]): Promise<void> {
    return columnarAppend(this.loomPath, workspace, name, row, this.key, this.auth);
  }

  columnarScanCbor(workspace: string, name: string): Promise<Uint8Array> {
    return columnarScanCbor(this.loomPath, workspace, name, this.key, this.auth);
  }

  columnarColumnsCbor(workspace: string, name: string): Promise<Uint8Array> {
    return columnarColumnsCbor(this.loomPath, workspace, name, this.key, this.auth);
  }

  columnarRows(workspace: string, name: string): Promise<number> {
    return columnarRows(this.loomPath, workspace, name, this.key, this.auth);
  }

  columnarCompact(workspace: string, name: string): Promise<void> {
    return columnarCompact(this.loomPath, workspace, name, this.key, this.auth);
  }

  columnarInspectCbor(workspace: string, name: string): Promise<Uint8Array> {
    return columnarInspectCbor(this.loomPath, workspace, name, this.key, this.auth);
  }

  columnarSourceDigestCbor(workspace: string, name: string): Promise<Uint8Array> {
    return columnarSourceDigestCbor(this.loomPath, workspace, name, this.key, this.auth);
  }

  columnarSelectCbor(
    workspace: string,
    name: string,
    columns: Uint8Array | number[],
    filter: Uint8Array | number[]
  ): Promise<Uint8Array> {
    return columnarSelectCbor(this.loomPath, workspace, name, columns, filter, this.key, this.auth);
  }

  columnarAggregateCbor(
    workspace: string,
    name: string,
    aggregates: Uint8Array | number[],
    filter: Uint8Array | number[]
  ): Promise<Uint8Array> {
    return columnarAggregateCbor(this.loomPath, workspace, name, aggregates, filter, this.key, this.auth);
  }

  dataframeCreate(workspace: string, name: string, plan: Uint8Array | number[]): Promise<void> {
    return dataframeCreate(this.loomPath, workspace, name, plan, this.key, this.auth);
  }

  dataframeCollectCbor(workspace: string, name: string): Promise<Uint8Array> {
    return dataframeCollectCbor(this.loomPath, workspace, name, this.key, this.auth);
  }

  dataframePreviewCbor(workspace: string, name: string, rows: number): Promise<Uint8Array> {
    return dataframePreviewCbor(this.loomPath, workspace, name, rows, this.key, this.auth);
  }

  dataframeMaterialize(workspace: string, name: string): Promise<string | null> {
    return dataframeMaterialize(this.loomPath, workspace, name, this.key, this.auth);
  }

  dataframePlanDigest(workspace: string, name: string): Promise<string> {
    return dataframePlanDigest(this.loomPath, workspace, name, this.key, this.auth);
  }

  dataframeSourceDigestsCbor(workspace: string, name: string): Promise<Uint8Array> {
    return dataframeSourceDigestsCbor(this.loomPath, workspace, name, this.key, this.auth);
  }

  searchCreate(workspace: string, name: string, mapping: Uint8Array | number[]): Promise<void> {
    return searchCreate(this.loomPath, workspace, name, mapping, this.key, this.auth);
  }

  searchIndex(
    workspace: string,
    name: string,
    id: Uint8Array | number[],
    doc: Uint8Array | number[]
  ): Promise<void> {
    return searchIndex(this.loomPath, workspace, name, id, doc, this.key, this.auth);
  }

  searchGet(workspace: string, name: string, id: Uint8Array | number[]): Promise<Uint8Array | null> {
    return searchGet(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  searchDelete(workspace: string, name: string, id: Uint8Array | number[]): Promise<boolean> {
    return searchDelete(this.loomPath, workspace, name, id, this.key, this.auth);
  }

  searchIdsCbor(
    workspace: string,
    name: string,
    prefix?: Uint8Array | number[] | null
  ): Promise<Uint8Array> {
    return searchIdsCbor(this.loomPath, workspace, name, prefix, this.key, this.auth);
  }

  searchRemap(workspace: string, name: string, mapping: Uint8Array | number[]): Promise<void> {
    return searchRemap(this.loomPath, workspace, name, mapping, this.key, this.auth);
  }

  searchQueryCbor(workspace: string, name: string, request: Uint8Array | number[]): Promise<Uint8Array> {
    return searchQueryCbor(this.loomPath, workspace, name, request, this.key, this.auth);
  }

  vcsBlameCbor(workspace: string, branch: string): Promise<Uint8Array> {
    return vcsBlameCbor(this.loomPath, workspace, branch, this.key, this.auth);
  }

  vcsDiffCbor(workspace: string, fromCommit: string, toCommit: string): Promise<Uint8Array> {
    return vcsDiffCbor(this.loomPath, workspace, fromCommit, toCommit, this.key, this.auth);
  }

  watchSubscribe(
    workspace: string,
    branch: string,
    facet?: string | null,
    pathPrefix?: string | null,
    changeKinds: string[] = [],
    fromCommit?: string | null
  ): Promise<string> {
    return watchSubscribe(
      this.loomPath,
      workspace,
      branch,
      facet,
      pathPrefix,
      changeKinds,
      fromCommit,
      this.key,
      this.auth
    );
  }

  watchPollCbor(cursor: string, max: number): Promise<Uint8Array> {
    return watchPollCbor(this.loomPath, cursor, max, this.key, this.auth);
  }

  sqlExec(workspace: string, db: string, sql: string): Promise<LoomStatement[]> {
    return sqlExec(this.loomPath, workspace, db, sql, this.key, this.auth);
  }

  sqlBatch(workspace: string, db: string, statements: string[]): Promise<LoomStatement[]> {
    return sqlBatch(this.loomPath, workspace, db, statements, this.key, this.auth);
  }

  sqlExecJson(workspace: string, db: string, sql: string): Promise<string> {
    return sqlExecJson(this.loomPath, workspace, db, sql, this.key, this.auth);
  }

  sqlExecBytes(workspace: string, db: string, sql: string): Promise<Uint8Array> {
    return sqlExecBytes(this.loomPath, workspace, db, sql, this.key, this.auth);
  }

  sqlQueryBytes(workspace: string, db: string, sql: string): Promise<Uint8Array[]> {
    return sqlQueryBytes(this.loomPath, workspace, db, sql, this.key, this.auth);
  }

  sqlCommit(workspace: string, db: string, message: string, author: string): Promise<string> {
    return sqlCommit(this.loomPath, workspace, db, message, author, this.key, this.auth);
  }

  sqlReadTableCbor(workspace: string, table: string): Promise<Uint8Array> {
    return sqlReadTableCbor(this.loomPath, workspace, table, this.key, this.auth);
  }

  sqlReadTableAtCbor(workspace: string, table: string, commit: string): Promise<Uint8Array> {
    return sqlReadTableAtCbor(this.loomPath, workspace, table, commit, this.key, this.auth);
  }

  sqlIndexScanCbor(
    workspace: string,
    table: string,
    index: string,
    prefix: Uint8Array | number[]
  ): Promise<Uint8Array> {
    return sqlIndexScanCbor(this.loomPath, workspace, table, index, prefix, this.key, this.auth);
  }

  sqlIndexScanAtCbor(
    workspace: string,
    table: string,
    index: string,
    prefix: Uint8Array | number[],
    commit: string
  ): Promise<Uint8Array> {
    return sqlIndexScanAtCbor(this.loomPath, workspace, table, index, prefix, commit, this.key, this.auth);
  }

  sqlBlameCbor(workspace: string, branch: string, table: string): Promise<Uint8Array> {
    return sqlBlameCbor(this.loomPath, workspace, branch, table, this.key, this.auth);
  }

  sqlDiffCbor(workspace: string, table: string, fromCommit: string, toCommit: string): Promise<Uint8Array> {
    return sqlDiffCbor(this.loomPath, workspace, table, fromCommit, toCommit, this.key, this.auth);
  }

  sqlTableDiffCbor(workspace: string, table: string, fromCommit: string, toCommit: string): Promise<Uint8Array> {
    return sqlTableDiffCbor(this.loomPath, workspace, table, fromCommit, toCommit, this.key, this.auth);
  }

  calCreateCollection(
    workspace: string,
    principal: string,
    collection: string,
    displayName: string,
    components: string
  ): Promise<void> {
    return calCreateCollection(
      this.loomPath,
      workspace,
      principal,
      collection,
      displayName,
      components,
      this.key,
      this.auth
    );
  }

  calDeleteCollection(workspace: string, principal: string, collection: string): Promise<boolean> {
    return calDeleteCollection(this.loomPath, workspace, principal, collection, this.key, this.auth);
  }

  calListCollectionsCbor(workspace: string, principal: string): Promise<Uint8Array> {
    return calListCollectionsCbor(this.loomPath, workspace, principal, this.key, this.auth);
  }

  calPutEntry(workspace: string, principal: string, collection: string, entry: Uint8Array | number[]): Promise<void> {
    return calPutEntry(this.loomPath, workspace, principal, collection, entry, this.key, this.auth);
  }

  calGetEntry(workspace: string, principal: string, collection: string, uid: string): Promise<Uint8Array | null> {
    return calGetEntry(this.loomPath, workspace, principal, collection, uid, this.key, this.auth);
  }

  calDeleteEntry(workspace: string, principal: string, collection: string, uid: string): Promise<boolean> {
    return calDeleteEntry(this.loomPath, workspace, principal, collection, uid, this.key, this.auth);
  }

  calListEntriesCbor(workspace: string, principal: string, collection: string): Promise<Uint8Array> {
    return calListEntriesCbor(this.loomPath, workspace, principal, collection, this.key, this.auth);
  }

  calRangeCbor(
    workspace: string,
    principal: string,
    collection: string,
    from: string,
    to: string
  ): Promise<Uint8Array> {
    return calRangeCbor(this.loomPath, workspace, principal, collection, from, to, this.key, this.auth);
  }

  calSearchCbor(
    workspace: string,
    principal: string,
    collection: string,
    component: string,
    text: string
  ): Promise<Uint8Array> {
    return calSearchCbor(this.loomPath, workspace, principal, collection, component, text, this.key, this.auth);
  }

  calEntryIcs(workspace: string, principal: string, collection: string, uid: string): Promise<string | null> {
    return calEntryIcs(this.loomPath, workspace, principal, collection, uid, this.key, this.auth);
  }

  calPutIcs(workspace: string, principal: string, collection: string, ics: string): Promise<string> {
    return calPutIcs(this.loomPath, workspace, principal, collection, ics, this.key, this.auth);
  }

  cardCreateBook(workspace: string, principal: string, book: string, displayName: string): Promise<void> {
    return cardCreateBook(this.loomPath, workspace, principal, book, displayName, this.key, this.auth);
  }

  cardDeleteBook(workspace: string, principal: string, book: string): Promise<boolean> {
    return cardDeleteBook(this.loomPath, workspace, principal, book, this.key, this.auth);
  }

  cardListBooksCbor(workspace: string, principal: string): Promise<Uint8Array> {
    return cardListBooksCbor(this.loomPath, workspace, principal, this.key, this.auth);
  }

  cardPutEntry(workspace: string, principal: string, book: string, entry: Uint8Array | number[]): Promise<void> {
    return cardPutEntry(this.loomPath, workspace, principal, book, entry, this.key, this.auth);
  }

  cardGetEntry(workspace: string, principal: string, book: string, uid: string): Promise<Uint8Array | null> {
    return cardGetEntry(this.loomPath, workspace, principal, book, uid, this.key, this.auth);
  }

  cardDeleteEntry(workspace: string, principal: string, book: string, uid: string): Promise<boolean> {
    return cardDeleteEntry(this.loomPath, workspace, principal, book, uid, this.key, this.auth);
  }

  cardListEntriesCbor(workspace: string, principal: string, book: string): Promise<Uint8Array> {
    return cardListEntriesCbor(this.loomPath, workspace, principal, book, this.key, this.auth);
  }

  cardSearchCbor(workspace: string, principal: string, book: string, text: string): Promise<Uint8Array> {
    return cardSearchCbor(this.loomPath, workspace, principal, book, text, this.key, this.auth);
  }

  cardEntryVcard(workspace: string, principal: string, book: string, uid: string): Promise<string | null> {
    return cardEntryVcard(this.loomPath, workspace, principal, book, uid, this.key, this.auth);
  }

  cardPutVcard(workspace: string, principal: string, book: string, vcf: string): Promise<string> {
    return cardPutVcard(this.loomPath, workspace, principal, book, vcf, this.key, this.auth);
  }

  mailCreateMailbox(workspace: string, principal: string, mailbox: string, displayName: string): Promise<void> {
    return mailCreateMailbox(this.loomPath, workspace, principal, mailbox, displayName, this.key, this.auth);
  }

  mailDeleteMailbox(workspace: string, principal: string, mailbox: string): Promise<boolean> {
    return mailDeleteMailbox(this.loomPath, workspace, principal, mailbox, this.key, this.auth);
  }

  mailListMailboxesCbor(workspace: string, principal: string): Promise<Uint8Array> {
    return mailListMailboxesCbor(this.loomPath, workspace, principal, this.key, this.auth);
  }

  mailIngestMessage(
    workspace: string,
    principal: string,
    mailbox: string,
    uid: string,
    raw: Uint8Array | number[]
  ): Promise<string> {
    return mailIngestMessage(this.loomPath, workspace, principal, mailbox, uid, raw, this.key, this.auth);
  }

  mailGetMessage(workspace: string, principal: string, mailbox: string, uid: string): Promise<Uint8Array | null> {
    return mailGetMessage(this.loomPath, workspace, principal, mailbox, uid, this.key, this.auth);
  }

  mailToEml(workspace: string, principal: string, mailbox: string, uid: string): Promise<Uint8Array | null> {
    return mailToEml(this.loomPath, workspace, principal, mailbox, uid, this.key, this.auth);
  }

  mailDeleteMessage(workspace: string, principal: string, mailbox: string, uid: string): Promise<boolean> {
    return mailDeleteMessage(this.loomPath, workspace, principal, mailbox, uid, this.key, this.auth);
  }

  mailListMessagesCbor(workspace: string, principal: string, mailbox: string): Promise<Uint8Array> {
    return mailListMessagesCbor(this.loomPath, workspace, principal, mailbox, this.key, this.auth);
  }

  mailGetFlagsCbor(workspace: string, principal: string, mailbox: string, uid: string): Promise<Uint8Array> {
    return mailGetFlagsCbor(this.loomPath, workspace, principal, mailbox, uid, this.key, this.auth);
  }

  mailSetFlags(
    workspace: string,
    principal: string,
    mailbox: string,
    uid: string,
    flags: Uint8Array | number[]
  ): Promise<void> {
    return mailSetFlags(this.loomPath, workspace, principal, mailbox, uid, flags, this.key, this.auth);
  }

  mailSearchCbor(workspace: string, principal: string, mailbox: string, text: string): Promise<Uint8Array> {
    return mailSearchCbor(this.loomPath, workspace, principal, mailbox, text, this.key, this.auth);
  }
}
