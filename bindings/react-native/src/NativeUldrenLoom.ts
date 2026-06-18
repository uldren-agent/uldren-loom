import type { TurboModule } from 'react-native';
import { TurboModuleRegistry } from 'react-native';

export type DocumentText = {
  text: string;
  digest: string;
  entity_tag: string;
};

export type DocumentBinary = {
  bytes: number[];
  digest: string;
  entity_tag: string;
};

export type DocumentPutResult = {
  digest: string;
  entity_tag: string;
};

// Codegen spec (new architecture). `bytes` is an array of 0-255 byte values; the result is the
// content address "algo:hex".
export interface Spec extends TurboModule {
  version(): string;
  blobDigest(bytes: number[]): string;
  // Create a fresh `.loom` under an identity `profile` ("default"/"blake3" or "fips"/"sha256"),
  // optionally encrypted. An empty `suite` picks the profile default; a non-empty
  // `passphrase` encrypts the store by wrapping the DEK under it, an empty one leaves it unencrypted.
  // `createWithKek` wraps the DEK under a host-supplied 256-bit KEK; `kek` is 32 bytes as a
  // 0-255 number array). Both resolve a Promise off the JS thread; reject carries the engine error.
  create(
    loomPath: string,
    profile: string,
    suite: string,
    passphrase: string
  ): Promise<void>;
  createWithKek(
    loomPath: string,
    profile: string,
    suite: string,
    kek: number[]
  ): Promise<void>;
  workspaceCreate(
    loomPath: string,
    name: string,
    facet: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  workspaceListJson(
    loomPath: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  workspaceRename(
    loomPath: string,
    workspace: string,
    newName: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  workspaceDelete(
    loomPath: string,
    workspace: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  authenticatePassphrase(
    loomPath: string,
    principal: string,
    principalPassphrase: string,
    passphrase: string,
    kek: number[]
  ): Promise<void>;
  identityListJson(
    loomPath: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  identityAddPrincipal(
    loomPath: string,
    principalHandle: string,
    name: string,
    kind: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  identityRenamePrincipalHandle(
    loomPath: string,
    principal: string,
    principalHandle: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  identitySetPassphrase(
    loomPath: string,
    principal: string,
    principalPassphrase: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  identityRemovePrincipal(
    loomPath: string,
    principal: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  identityAssignRole(
    loomPath: string,
    principal: string,
    role: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  identityRevokeRole(
    loomPath: string,
    principal: string,
    role: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  identityCreateExternalCredential(
    loomPath: string,
    principal: string,
    kind: string,
    label: string,
    issuer: string,
    subject: string,
    materialDigest: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  identityRevokeExternalCredential(
    loomPath: string,
    credential: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  identityAddPublicKey(
    loomPath: string,
    principal: string,
    label: string,
    algorithm: string,
    publicKeyHex: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  identityRevokePublicKey(
    loomPath: string,
    key: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  aclListJson(
    loomPath: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  aclGrant(
    loomPath: string,
    effect: number,
    subject: string,
    workspace: string,
    domain: string,
    rightsMask: number,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  aclGrantScoped(
    loomPath: string,
    effect: number,
    subject: string,
    workspace: string,
    domain: string,
    rightsMask: number,
    refGlob: string,
    scopes: string[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  aclGrantScopedPredicate(
    loomPath: string,
    effect: number,
    subject: string,
    workspace: string,
    domain: string,
    rightsMask: number,
    refGlob: string,
    scopes: string[],
    predicateCel: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  aclRevoke(
    loomPath: string,
    effect: number,
    subject: string,
    workspace: string,
    domain: string,
    rightsMask: number,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  aclRevokeScoped(
    loomPath: string,
    effect: number,
    subject: string,
    workspace: string,
    domain: string,
    rightsMask: number,
    refGlob: string,
    scopes: string[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  aclRevokeScopedPredicate(
    loomPath: string,
    effect: number,
    subject: string,
    workspace: string,
    domain: string,
    rightsMask: number,
    refGlob: string,
    scopes: string[],
    predicateCel: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  protectedRefListJson(
    loomPath: string,
    workspace: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  protectedRefGetJson(
    loomPath: string,
    workspace: string,
    refName: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  protectedRefSet(
    loomPath: string,
    workspace: string,
    refName: string,
    fastForwardOnly: boolean,
    signedCommitsRequired: boolean,
    signedRefAdvanceRequired: boolean,
    requiredReviewCount: number,
    retentionLock: boolean,
    governanceLock: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  protectedRefRemove(
    loomPath: string,
    workspace: string,
    refName: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  sqlReadTable(
    loomPath: string,
    workspace: string,
    table: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  sqlReadTableAt(
    loomPath: string,
    workspace: string,
    table: string,
    commit: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  sqlIndexScan(
    loomPath: string,
    workspace: string,
    table: string,
    index: string,
    prefix: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  sqlIndexScanAt(
    loomPath: string,
    workspace: string,
    table: string,
    index: string,
    prefix: number[],
    commit: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  sqlBlame(
    loomPath: string,
    workspace: string,
    branch: string,
    table: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  sqlDiff(
    loomPath: string,
    workspace: string,
    table: string,
    fromCommit: string,
    toCommit: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  sqlTableDiff(
    loomPath: string,
    workspace: string,
    table: string,
    fromCommit: string,
    toCommit: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // Workspace/entry-level history. These methods return raw Loom Canonical CBOR (a 0-255 number
  // array). Each call opens the loom, runs, and closes.
  vcsBlame(
    loomPath: string,
    workspace: string,
    branch: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  vcsDiff(
    loomPath: string,
    workspace: string,
    fromCommit: string,
    toCommit: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  watchSubscribe(
    loomPath: string,
    workspace: string,
    branch: string,
    facet: string,
    pathPrefix: string,
    changeKinds: string,
    fromCommit: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  watchPoll(
    loomPath: string,
    cursor: string,
    max: number,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // Append-log queue facade over a workspace stream. Like the workspace methods, each call opens the
  // loom, runs, and closes; a write by name ensures a queue-facet workspace. `entry` is a 0-255 byte
  // array; `queueRange` and `queueConsumerRead` resolve raw Loom Canonical CBOR (an array of byte
  // strings) as a 0-255 number array. `queueGet` resolves the entry bytes, or null when out of range.
  // Sequence and length values cross as unsigned 64-bit decimal strings. Consumer offsets are
  // operational metadata: reads do not advance, `queueConsumerAdvance` is monotonic, and
  // `queueConsumerReset` may move backward.
  queueAppend(
    loomPath: string,
    workspace: string,
    stream: string,
    entry: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  queueGet(
    loomPath: string,
    workspace: string,
    stream: string,
    seq: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  queueRange(
    loomPath: string,
    workspace: string,
    stream: string,
    lo: string,
    hi: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  queueLen(
    loomPath: string,
    workspace: string,
    stream: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  queueConsumerPosition(
    loomPath: string,
    workspace: string,
    stream: string,
    consumerId: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  queueConsumerRead(
    loomPath: string,
    workspace: string,
    stream: string,
    consumerId: string,
    max: number,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  queueConsumerAdvance(
    loomPath: string,
    workspace: string,
    stream: string,
    consumerId: string,
    nextSeq: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  queueConsumerReset(
    loomPath: string,
    workspace: string,
    stream: string,
    consumerId: string,
    nextSeq: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  // The capability registry as Loom Canonical CBOR (a 0-255 number array). Handle-free: it reports the
  // bindings layer's static catalog and does not open a loom.
  capabilities(): Promise<number[]>;
  // The runtime provider/profile report as Loom Canonical CBOR.
  runtimeProfile(): Promise<number[]>;
  studioSurfaceCatalogJson(workspace: string, set: string): Promise<string>;
  execCbor(
    loomPath: string,
    request: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // Content-addressed store facade over a workspace `cas` facet in a `.loom`. Stateless one-shot calls
  // (each opens the loom, runs, and closes). `content` is a 0-255 byte array; `casPut` resolves the
  // content address "algo:hex"; `casGet` resolves the blob bytes as a 0-255 number array, or null when
  // the digest is absent; `casHas` resolves presence; `casListJson` resolves a JSON string array of
  // reachable digests, sorted. Encrypted stores carry a `passphrase` and a `kek` (32-byte 0-255 number
  // array) so the per-op reopen can unlock; `kek` wins if both are given.
  casPut(
    loomPath: string,
    workspace: string,
    content: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  casGet(
    loomPath: string,
    workspace: string,
    digest: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  casHas(
    loomPath: string,
    workspace: string,
    digest: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  casListJson(
    loomPath: string,
    workspace: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  // Drop the blob addressed by `digest` from the workspace's working tree (unreachable going forward);
  // resolves whether it was present. CAS stays immutable: an earlier commit that held it still restores
  // it, and bytes are GC-reclaimed once unreferenced.
  casDelete(
    loomPath: string,
    workspace: string,
    digest: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  meetingsImportSnapshot(
    loomPath: string,
    workspace: string,
    inputProfile: string,
    snapshot: number[],
    dryRun: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  meetingsSourceRead(
    loomPath: string,
    workspace: string,
    sourceId: string,
    leaf: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  driveListJson(
    loomPath: string,
    workspace: string,
    driveWorkspaceId: string,
    folderId: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  driveStatJson(
    loomPath: string,
    workspace: string,
    driveWorkspaceId: string,
    folderId: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  driveReadFile(
    loomPath: string,
    workspace: string,
    driveWorkspaceId: string,
    fileId: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  driveListVersionsJson(
    loomPath: string,
    workspace: string,
    driveWorkspaceId: string,
    fileId: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  driveListConflictsJson(loomPath: string, workspace: string, driveWorkspaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveListSharesJson(loomPath: string, workspace: string, driveWorkspaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveListRetentionJson(loomPath: string, workspace: string, driveWorkspaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveCreateFolderJson(loomPath: string, workspace: string, driveWorkspaceId: string, parentFolderId: string, folderId: string, name: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveCreateUploadJson(loomPath: string, workspace: string, driveWorkspaceId: string, uploadId: string, parentFolderId: string, name: string, fileId: string, expectedRoot: string, createdAtMs: string, replaceFile: boolean, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveUploadChunkJson(loomPath: string, workspace: string, driveWorkspaceId: string, uploadId: string, chunk: number[], passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveCommitUploadJson(loomPath: string, workspace: string, driveWorkspaceId: string, uploadId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveRenameJson(loomPath: string, workspace: string, driveWorkspaceId: string, folderId: string, nodeId: string, newName: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveMoveJson(loomPath: string, workspace: string, driveWorkspaceId: string, sourceFolderId: string, targetFolderId: string, nodeId: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveDeleteJson(loomPath: string, workspace: string, driveWorkspaceId: string, folderId: string, nodeId: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveResolveConflictJson(loomPath: string, workspace: string, driveWorkspaceId: string, conflictId: string, resolution: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveGrantShareJson(loomPath: string, workspace: string, driveWorkspaceId: string, grantId: string, targetKind: string, targetId: string, principal: string, role: string, grantedAtMs: string, expiresAtMs: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveRevokeShareJson(loomPath: string, workspace: string, driveWorkspaceId: string, grantId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveApplyShareExpiryJson(loomPath: string, workspace: string, driveWorkspaceId: string, nowMs: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  drivePinRetentionJson(loomPath: string, workspace: string, driveWorkspaceId: string, pinId: string, kind: string, root: string, targetEntityId: string, addedAtMs: string, expiresAtMs: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveUnpinRetentionJson(loomPath: string, workspace: string, driveWorkspaceId: string, pinId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  driveApplyRetentionJson(loomPath: string, workspace: string, driveWorkspaceId: string, nowMs: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsProjectCreateJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, keyPrefix: string, name: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsProjectRekeyJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, keyPrefix: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsProjectSettingsGetJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsProjectSettingsSetJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, defaultProjection: string, enableProjectionsJson: string, disableProjectionsJson: string, actorEnforcement: string, projectOwnerPrincipal: string, clearProjectOwnerPrincipal: boolean, acceptanceAuthoritiesJson: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsFieldsJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, projection: string, operation: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsFieldPutJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, fieldId: string, fieldKey: string, name: string, description: string, fieldType: string, optionSet: string, maxLength: number, hasMaxLength: boolean, required: boolean, searchable: boolean, orderable: boolean, cardinality: string, applicableTypeIdsJson: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsFieldRetireJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, fieldId: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsCreateJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projectId: string, ticketType: string, externalSource: string, externalId: string, fieldsJson: string, policyLabelsJson: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsUpdateJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, setFieldsJson: string, deleteFieldsJson: string, action: string, targetStatus: string, observedSourceStatus: string, observedWorkflowVersion: string, assignee: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string, commentId: string, commentType: string, commentBody: string, commentsJson: string, relationSetsJson: string, relationRemovesJson: string): Promise<string>;
  ticketsDeleteJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsCommentsJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsCommentAddJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string, commentType: string, body: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsCommentUpdateJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string, commentType: string, body: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsCommentDeleteJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, commentId: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsRelationSetJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, relationId: string, kind: string, targetId: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsRelationRemoveJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, relationId: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsGetJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, projection: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsListJson(loomPath: string, workspace: string, ticketWorkspaceId: string, projection: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  ticketsHistoryJson(loomPath: string, workspace: string, ticketWorkspaceId: string, ticketId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  spacesCreateJson(loomPath: string, workspace: string, pageWorkspaceId: string, spaceId: string, title: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  spacesListJson(loomPath: string, workspace: string, pageWorkspaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  spacesGetJson(loomPath: string, workspace: string, pageWorkspaceId: string, spaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  pagesCreateJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, spaceId: string, parentPageId: string, title: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  pagesUpdateJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, bodyText: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  pagesPublishJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  pagesGetJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  pagesListJson(loomPath: string, workspace: string, pageWorkspaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  pagesHistoryJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresCreateJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, spaceId: string, kind: string, title: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresAddNodeJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, kind: string, label: string, bodyDigest: string, entityRef: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresUpdateNodeJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, kind: string, label: string, bodyDigest: string, entityRef: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresBindJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, entityRef: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresMoveNodeJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, parentNodeId: string, label: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresLinkNodeJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, edgeId: string, srcNodeId: string, dstNodeId: string, label: string, targetRef: string, expectedRoot: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresDecomposeToTicketsJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, itemsJson: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresGetJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  structuresListJson(loomPath: string, workspace: string, pageWorkspaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  lanesCreate(loomPath: string, workspace: string, lane: number[], passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<number[]>;
  lanesGet(loomPath: string, workspace: string, laneId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<number[] | null>;
  lanesList(loomPath: string, workspace: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<number[]>;
  lanesUpdate(loomPath: string, workspace: string, laneId: string, title: string | null, description: string | null, laneStatus: string | null, statusReport: string | null, reviewerFeedback: string | null, updatedBy: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<number[]>;
  lanesTicketAdd(loomPath: string, workspace: string, laneId: string, ticketId: string, updatedBy: string, placement: string, anchor: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<number[]>;
  lanesTicketRemove(loomPath: string, workspace: string, laneId: string, ticketId: string, updatedBy: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<number[]>;
  chatCreateChannelJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, channelHandle: string, name: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatRenameChannelJson(loomPath: string, workspace: string, chatWorkspaceId: string, selector: string, channelHandle: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatListChannelsJson(loomPath: string, workspace: string, chatWorkspaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatPostMessageJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, threadId: string, bodyText: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatEditMessageJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, bodyText: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatRedactMessageJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, reason: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatCreateThreadJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, threadId: string, parentMessageId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatCreateTaskJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, messageId: string, title: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatClaimTaskJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, claimId: string, leaseToken: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatCompleteTaskJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, claimId: string, resultMessageId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatInvokeAgentJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, invocationId: string, agentPrincipal: string, sourceMessageIdsJson: string, promptText: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatAgentReplyJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, invocationId: string, messageId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatRequestHandoffJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, handoffId: string, fromAgentPrincipal: string, toPrincipal: string, reason: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatAddReactionJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, kind: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatRemoveReactionJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, kind: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatEmojiListJson(loomPath: string, workspace: string, chatWorkspaceId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatEmojiRegisterJson(loomPath: string, workspace: string, chatWorkspaceId: string, kind: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatEmojiUnregisterJson(loomPath: string, workspace: string, chatWorkspaceId: string, kind: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatMessagesJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatCursorJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatUpdateCursorJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, nextSequence: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  chatFetchEventsJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, fromSequence: string, max: string, passphrase: string, kek: number[], authPrincipal: string, authPassphrase: string): Promise<string>;
  fsImport(
    loomPath: string,
    workspace: string,
    srcPath: string,
    commit: boolean,
    dryRun: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  fsExport(
    loomPath: string,
    workspace: string,
    dstPath: string,
    revision: string,
    dryRun: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  archiveImport(
    loomPath: string,
    workspace: string,
    srcPath: string,
    kind: string,
    dryRun: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  archiveExport(
    loomPath: string,
    workspace: string,
    dstPath: string,
    kind: string,
    revision: string,
    dryRun: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  carImport(
    loomPath: string,
    srcPath: string,
    dryRun: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  carExport(
    loomPath: string,
    workspace: string,
    dstPath: string,
    dryRun: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // Key-value facade over a workspace `kv` facet: a versioned map keyed by a typed, order-preserving key
  // (one Loom Canonical CBOR cell as a 0-255 byte array), valued by opaque bytes. `kvPut` ensures a
  // kv-facet workspace; `kvGet` resolves the value bytes or null; `kvDelete` resolves whether the key was
  // present; `kvList`/`kvRange` resolve the canonical CBOR array of `[key, value]` pairs (a 0-255 byte
  // array), `kvRange` half-open [lo, hi).
  kvPut(
    loomPath: string,
    workspace: string,
    collection: string,
    key: number[],
    value: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  kvGet(
    loomPath: string,
    workspace: string,
    collection: string,
    key: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  kvDelete(
    loomPath: string,
    workspace: string,
    collection: string,
    key: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  kvList(
    loomPath: string,
    workspace: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  kvRange(
    loomPath: string,
    workspace: string,
    collection: string,
    lo: number[],
    hi: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // Document facade over a workspace `document` facet: text APIs require valid UTF-8 strings and binary
  // APIs use explicit byte arrays. Time-series facade over a `time-series` facet,
  // keyed by an i64 timestamp (passed as a decimal string for 64-bit safety): `tsPut`/`tsGet` a single
  // point; `tsRange` resolves the half-open [from, to) CBOR `[ts, value]` pairs; `tsLatest` resolves the
  // most recent point as `{ ts, value }` or null. Ledger facade over a `ledger` facet, an append-only
  // hash chain: `ledgerAppend` resolves the new entry's u64 sequence (decimal string); `ledgerGet`
  // resolves the payload or null; `ledgerHead` resolves the head hash "algo:hex" or null; `ledgerLen`
  // resolves the entry count (decimal string); `ledgerVerify` rejects if the chain is broken.
  docPutText(
    loomPath: string,
    workspace: string,
    collection: string,
    id: string,
    text: string,
    expectedEntityTag: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<DocumentPutResult>;
  docGetText(
    loomPath: string,
    workspace: string,
    collection: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<DocumentText | null>;
  docPutBinary(
    loomPath: string,
    workspace: string,
    collection: string,
    id: string,
    bytes: number[],
    expectedEntityTag: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<DocumentPutResult>;
  docGetBinary(
    loomPath: string,
    workspace: string,
    collection: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<DocumentBinary | null>;
  docDelete(
    loomPath: string,
    workspace: string,
    collection: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  docListBinary(
    loomPath: string,
    workspace: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  docIndexCreate(
    loomPath: string,
    workspace: string,
    collection: string,
    name: string,
    fieldPath: string,
    unique: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  docIndexCreateJson(
    loomPath: string,
    workspace: string,
    collection: string,
    declarationJson: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  docIndexDrop(
    loomPath: string,
    workspace: string,
    collection: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  docIndexRebuild(
    loomPath: string,
    workspace: string,
    collection: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  docIndexListJson(
    loomPath: string,
    workspace: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  docIndexStatusJson(
    loomPath: string,
    workspace: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  docFindJson(
    loomPath: string,
    workspace: string,
    collection: string,
    index: string,
    valueJson: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  docQueryJson(
    loomPath: string,
    workspace: string,
    collection: string,
    queryJson: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  tsPut(
    loomPath: string,
    workspace: string,
    collection: string,
    ts: string,
    value: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  tsGet(
    loomPath: string,
    workspace: string,
    collection: string,
    ts: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  tsRange(
    loomPath: string,
    workspace: string,
    collection: string,
    from: string,
    to: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  tsLatest(
    loomPath: string,
    workspace: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<{ ts: string; value: number[] } | null>;
  metricsPutDescriptor(
    loomPath: string,
    workspace: string,
    descriptor: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  metricsGetDescriptor(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  metricsPutObservation(
    loomPath: string,
    workspace: string,
    descriptorName: string,
    observation: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  metricsQuery(
    loomPath: string,
    workspace: string,
    descriptorName: string,
    fromTimestampMs: string,
    toTimestampMs: string,
    maxSeries: number,
    maxGroups: number,
    maxSamples: number,
    maxOutputBytes: string,
    nowTimestampMs: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  logsPutRecord(
    loomPath: string,
    workspace: string,
    record: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  logsGetRecord(
    loomPath: string,
    workspace: string,
    recordId: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  logsQuery(
    loomPath: string,
    workspace: string,
    fromTimeUnixNano: string,
    toTimeUnixNano: string,
    maxRecords: number,
    maxOutputBytes: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  tracesPutSpan(
    loomPath: string,
    workspace: string,
    span: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  tracesGetSpan(
    loomPath: string,
    workspace: string,
    traceId: string,
    spanId: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  tracesTraceSpans(
    loomPath: string,
    workspace: string,
    traceId: string,
    maxSpans: number,
    maxOutputBytes: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  tracesQuery(
    loomPath: string,
    workspace: string,
    fromStartTimeNs: string,
    toStartTimeNs: string,
    maxSpans: number,
    maxOutputBytes: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  ledgerAppend(
    loomPath: string,
    workspace: string,
    collection: string,
    payload: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  ledgerGet(
    loomPath: string,
    workspace: string,
    collection: string,
    seq: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  ledgerHead(
    loomPath: string,
    workspace: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string | null>;
  ledgerLen(
    loomPath: string,
    workspace: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  ledgerVerify(
    loomPath: string,
    workspace: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  // Calendar facade over a workspace `calendar` facet (CalDAV collections + entries). Stateless one-shot
  // calls (each opens the loom, runs, and closes); a write by name ensures a calendar-facet workspace.
  // `calPutEntry` takes the `CalendarEntry` canonical CBOR as a 0-255 byte array. `calListCollections`,
  // `calListEntries`, `calRange`, and `calSearch` resolve raw Loom Canonical CBOR (a 0-255 number array).
  // `calGetEntry` resolves the entry CBOR, or null when absent. `calEntryIcs` resolves the `.ics` text, or
  // null when absent; `calPutIcs` resolves the new ETag "algo:hex". `passphrase`/`kek` unlock an encrypted
  // store for the per-op reopen (`kek` wins if both given).
  calCreateCollection(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    displayName: string,
    components: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  calDeleteCollection(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  calListCollections(
    loomPath: string,
    workspace: string,
    principal: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  calPutEntry(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    entry: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  calGetEntry(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  calDeleteEntry(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  calListEntries(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  calRange(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    from: string,
    to: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  calSearch(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    component: string,
    text: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  calEntryIcs(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string | null>;
  calPutIcs(
    loomPath: string,
    workspace: string,
    principal: string,
    collection: string,
    ics: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  // Contacts facade over a workspace `contacts` facet (CardDAV address books + contacts). Stateless
  // one-shot calls; a write by name ensures a contacts-facet workspace. `cardPutEntry` takes the
  // `ContactEntry` canonical CBOR as a 0-255 byte array. `cardListBooks`, `cardListEntries`, and
  // `cardSearch` resolve raw Loom Canonical CBOR. `cardGetEntry` resolves the entry CBOR, or null.
  // `cardEntryVcard` resolves the `.vcf` text, or null; `cardPutVcard` resolves the new ETag "algo:hex".
  cardCreateBook(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    displayName: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  cardDeleteBook(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  cardListBooks(
    loomPath: string,
    workspace: string,
    principal: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  cardPutEntry(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    entry: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  cardGetEntry(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  cardDeleteEntry(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  cardListEntries(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  cardSearch(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    text: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  cardEntryVcard(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string | null>;
  cardPutVcard(
    loomPath: string,
    workspace: string,
    principal: string,
    book: string,
    vcf: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  // Mail facade over a workspace `mail` facet (mailboxes + messages). Stateless one-shot calls; a write by
  // name ensures a mail-facet workspace. `mailIngestMessage` takes the raw RFC 5322 message as a 0-255 byte
  // array and resolves the body's content address "algo:hex". `mailListMailboxes`, `mailListMessages`,
  // `mailGetFlags`, and `mailSearch` resolve raw Loom Canonical CBOR. `mailGetMessage` resolves the message
  // index CBOR or null; `mailToEml` resolves the raw `.eml` bytes or null. `mailSetFlags` takes a Loom
  // Canonical CBOR `Array(Text)` buffer (0-255 byte array).
  mailCreateMailbox(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    displayName: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  mailDeleteMailbox(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  mailListMailboxes(
    loomPath: string,
    workspace: string,
    principal: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  mailIngestMessage(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    uid: string,
    raw: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  mailGetMessage(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  mailToEml(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  mailDeleteMessage(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  mailListMessages(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  mailGetFlags(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    uid: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  mailSetFlags(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    uid: string,
    flags: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  mailSearch(
    loomPath: string,
    workspace: string,
    principal: string,
    mailbox: string,
    text: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // SQL over a workspace SQL facet in a `.loom`. Stateless one-shot calls (each opens the loom, runs, and
  // closes), matching the engine's per-op model - no native handle is held across the JS bridge.
  //
  // These resolve a Promise and run off the JS thread: the engine has no worker pool of its own, so
  // the native module dispatches each call to a background queue. `sqlExecTyped`
  // resolves **lossless bridge JSON** (the type-faithful RN form: typed/tagged cells the JS bridge can
  // carry without BigInt/Uint8Array - the TS `sqlExec` JSON.parses it); `sqlExecJson` resolves the JSON
  // debug form; `sqlExecBytes` resolves the canonical-CBOR bytes as a 0-255 number
  // array; `sqlQueryBytes` resolves one canonical-CBOR row byte array per row and is read-only;
  // `sqlCommit` resolves the new commit's content address.
  //
  // Encrypted stores: each op carries a `passphrase` and a `kek`
  // (a 32-byte 0-255 number array) so the per-op reopen can unlock. The native layer picks the
  // opener: a 32-byte `kek` -> KEK unlock; else a non-empty `passphrase` -> passphrase unlock; else the
  // plain open. Both empty = an unencrypted store. `kek` wins if both are given.
  sqlExecTyped(
    loomPath: string,
    ns: string,
    db: string,
    sql: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  // Atomic transaction/batch in one native round-trip. Opens a held-open batch, runs the
  // statements in order (including BEGIN/COMMIT/ROLLBACK), and on success commits with a single atomic
  // save; any error aborts and discards. The writer lock is held entirely inside native code (off the
  // JS thread), never across the bridge. Resolves the **lossless bridge JSON** of the final statement's
  // result (TS JSON.parses it).
  sqlBatch(
    loomPath: string,
    ns: string,
    db: string,
    statements: string[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  sqlExecJson(
    loomPath: string,
    ns: string,
    db: string,
    sql: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  sqlExecBytes(
    loomPath: string,
    ns: string,
    db: string,
    sql: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  sqlQueryBytes(
    loomPath: string,
    ns: string,
    db: string,
    sql: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<Array<Array<number>>>;
  sqlCommit(
    loomPath: string,
    ns: string,
    db: string,
    message: string,
    author: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  // Property graph facade over a workspace `graph` facet. A write by name ensures a graph-facet
  // workspace. Node/edge props cross as raw Loom Canonical CBOR (`text -> bytes`, empty = none, a 0-255
  // byte array). `graphGetNode` resolves the props CBOR or null; `graphGetEdge` resolves the
  // `[src, dst, label, props]` CBOR or null; `graphRemoveEdge` resolves whether it was present.
  // `graphNeighbors`/`graphOutEdges`/`graphInEdges`/`graphReachable` resolve raw Loom Canonical CBOR.
  // `graphReachable` takes `maxDepth` (< 0 = no limit) and a nullable `viaLabel` (null = any edge).
  // `graphShortestPath` resolves the path CBOR or null when none exists.
  graphUpsertNode(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    props: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  graphGetNode(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  graphRemoveNode(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    cascade: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  graphUpsertEdge(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    src: string,
    dst: string,
    label: string,
    props: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  graphGetEdge(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  graphRemoveEdge(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  graphNeighbors(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  graphOutEdges(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  graphInEdges(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  graphReachable(
    loomPath: string,
    workspace: string,
    name: string,
    start: string,
    maxDepth: number,
    viaLabel: string | null,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  graphShortestPath(
    loomPath: string,
    workspace: string,
    name: string,
    from: string,
    to: string,
    viaLabel: string | null,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  // Vector set facade over a workspace `vector` facet. A write by name ensures a vector-facet
  // workspace. An embedding crosses as little-endian f32 bytes (a 0-255 byte array); metadata and
  // filters cross as raw Loom Canonical CBOR (a 0-255 byte array, empty = none/all). `vectorCreate`
  // takes `dim` and `metric` (1 cosine, 2 L2, 3 dot). `vectorGet` resolves `[vector_bytes, metadata]`
  // CBOR or null; `vectorIds` and `vectorMetadataIndexKeys` resolve CBOR text arrays; metadata-index
  // create/drop resolve whether the declaration changed; `vectorDelete` resolves whether it was
  // present; `vectorSearch` takes `k` and resolves the `[id, score_cell]` CBOR array.
  vectorCreate(
    loomPath: string,
    workspace: string,
    name: string,
    dim: number,
    metric: number,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  vectorUpsert(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    vector: number[],
    metadata: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  vectorUpsertSource(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    vector: number[],
    metadata: number[],
    sourceText: number[],
    modelId: string | null,
    weightsDigest: string | null,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  vectorGet(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  vectorSourceText(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  vectorEmbeddingModel(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  vectorIds(
    loomPath: string,
    workspace: string,
    name: string,
    prefix: string | null,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  vectorMetadataIndexKeys(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  vectorCreateMetadataIndex(
    loomPath: string,
    workspace: string,
    name: string,
    key: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  vectorDropMetadataIndex(
    loomPath: string,
    workspace: string,
    name: string,
    key: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  vectorDelete(
    loomPath: string,
    workspace: string,
    name: string,
    id: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  vectorSearch(
    loomPath: string,
    workspace: string,
    name: string,
    query: number[],
    k: number,
    filter: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  vectorSearchPolicy(
    loomPath: string,
    workspace: string,
    name: string,
    query: number[],
    k: number,
    filter: number[],
    policy: number,
    threshold: number,
    ef: number,
    pqM: number,
    pqK: number,
    pqIters: number,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // Columnar dataset facade over a workspace `columnar` facet. A write by name ensures a
  // columnar-facet workspace. Columns, rows, and filters cross as raw Loom Canonical CBOR (a 0-255 byte
  // array). `columnarCreate` takes the columns CBOR (an array of `[name, type_tag]`) and
  // `targetSegmentRows` (0 = default). `columnarScan`/`columnarColumns`/`columnarSelect` resolve raw
  // Loom Canonical CBOR; `columnarSelect` takes a columns CBOR array of text and a filter CBOR
  // (`[col, op, value_cell]`; empty = all). `columnarRows` resolves the total row count.
  // `columnarAggregate` takes CBOR `[[op, column?] ...]` plus the same filter shape.
  columnarCreate(
    loomPath: string,
    workspace: string,
    name: string,
    columns: number[],
    targetSegmentRows: number,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  columnarAppend(
    loomPath: string,
    workspace: string,
    name: string,
    row: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  columnarScan(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  columnarColumns(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  columnarRows(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number>;
  columnarCompact(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  columnarInspect(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  columnarSourceDigest(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  columnarSelect(
    loomPath: string,
    workspace: string,
    name: string,
    columns: number[],
    filter: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  columnarAggregate(
    loomPath: string,
    workspace: string,
    name: string,
    aggregates: number[],
    filter: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // Dataframe frame facade over a workspace `dataframe` facet. Plans cross as canonical
  // DataframePlan CBOR. Collect and preview return canonical CBOR `[columns, rows]`; materialize
  // resolves a CAS digest when the materialization target emits one.
  dataframeCreate(
    loomPath: string,
    workspace: string,
    name: string,
    plan: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  dataframeCollect(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  dataframePreview(
    loomPath: string,
    workspace: string,
    name: string,
    rows: number,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  dataframeMaterialize(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string | null>;
  dataframePlanDigest(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<string>;
  dataframeSourceDigests(
    loomPath: string,
    workspace: string,
    name: string,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  // Search index facade over a workspace `search` facet. A write by name ensures a search-facet
  // workspace. The mapping, id, doc, prefix, request, and CBOR results all cross as raw Loom Canonical
  // CBOR / opaque bytes (a 0-255 byte array). `searchCreate` takes the mapping CBOR (a map
  // `field -> [type_tag, stored, faceted]`); `searchIndex` takes the opaque id bytes and the doc CBOR
  // (a map `field -> value`). `searchGet` resolves the doc CBOR or null; `searchDelete` resolves whether
  // it was present. `searchIds` resolves the CBOR array of byte strings; `hasPrefix` restricts to ids
  // under `prefix`. `searchRemap` replaces the mapping; `searchQuery` takes the request CBOR
  // (`[query, limit, offset]`) and resolves the response CBOR.
  searchCreate(
    loomPath: string,
    workspace: string,
    name: string,
    mapping: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  searchIndex(
    loomPath: string,
    workspace: string,
    name: string,
    id: number[],
    doc: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  searchGet(
    loomPath: string,
    workspace: string,
    name: string,
    id: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[] | null>;
  searchDelete(
    loomPath: string,
    workspace: string,
    name: string,
    id: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<boolean>;
  searchIds(
    loomPath: string,
    workspace: string,
    name: string,
    prefix: number[],
    hasPrefix: boolean,
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
  searchRemap(
    loomPath: string,
    workspace: string,
    name: string,
    mapping: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<void>;
  searchQuery(
    loomPath: string,
    workspace: string,
    name: string,
    request: number[],
    passphrase: string,
    kek: number[],
    authPrincipal: string,
    authPassphrase: string
  ): Promise<number[]>;
}

export default TurboModuleRegistry.getEnforcing<Spec>('UldrenLoom');
