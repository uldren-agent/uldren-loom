package ai.uldren.loom.rn

/** Raw JNI surface for the React Native Android module. Internal FFI boundary;
 *  the public API is UldrenLoomModule's spec-override methods, which call through here. */
internal object UldrenLoomNative {
    init {
        System.loadLibrary("uldren_loom_rn")
    }

    external fun nativeVersion(): String

    external fun nativeBlobDigest(data: ByteArray): String

    external fun nativeQueueAppend(
        loomPath: String,
        workspace: String,
        stream: String,
        entry: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeQueueGet(
        loomPath: String,
        workspace: String,
        stream: String,
        seq: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeQueueRange(
        loomPath: String,
        workspace: String,
        stream: String,
        lo: String,
        hi: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeQueueLen(
        loomPath: String,
        workspace: String,
        stream: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeQueueConsumerPosition(
        loomPath: String,
        workspace: String,
        stream: String,
        consumerId: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeQueueConsumerRead(
        loomPath: String,
        workspace: String,
        stream: String,
        consumerId: String,
        max: Double,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeQueueConsumerAdvance(
        loomPath: String,
        workspace: String,
        stream: String,
        consumerId: String,
        nextSeq: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeQueueConsumerReset(
        loomPath: String,
        workspace: String,
        stream: String,
        consumerId: String,
        nextSeq: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeCreate(
        loomPath: String,
        profile: String,
        suite: String,
        passphrase: ByteArray,
    )

    external fun nativeCreateWithKek(
        loomPath: String,
        profile: String,
        suite: String,
        kek: ByteArray,
    )

    external fun nativeWorkspaceCreate(
        loomPath: String,
        name: String,
        facet: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeWorkspaceListJson(
        loomPath: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeWorkspaceRename(
        loomPath: String,
        workspace: String,
        newName: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeWorkspaceDelete(
        loomPath: String,
        workspace: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeAuthenticatePassphrase(
        loomPath: String,
        principal: String,
        principalPassphrase: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
    )

    external fun nativeIdentityListJson(
        loomPath: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeIdentityAddPrincipal(
        loomPath: String,
        principalHandle: String,
        name: String,
        kind: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeIdentityRenamePrincipalHandle(
        loomPath: String,
        principal: String,
        principalHandle: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeIdentitySetPassphrase(
        loomPath: String,
        principal: String,
        principalPassphrase: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeIdentityRemovePrincipal(
        loomPath: String,
        principal: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeIdentityAssignRole(
        loomPath: String,
        principal: String,
        role: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeIdentityRevokeRole(
        loomPath: String,
        principal: String,
        role: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeIdentityCreateExternalCredential(
        loomPath: String,
        principal: String,
        kind: String,
        label: String,
        issuer: String,
        subject: String,
        materialDigest: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeIdentityRevokeExternalCredential(
        loomPath: String,
        credential: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeIdentityAddPublicKey(
        loomPath: String,
        principal: String,
        label: String,
        algorithm: String,
        publicKeyHex: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeIdentityRevokePublicKey(
        loomPath: String,
        key: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeAclListJson(
        loomPath: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeAclGrant(
        loomPath: String,
        effect: Double,
        subject: String,
        workspace: String,
        facet: String,
        rightsMask: Double,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeAclGrantScoped(
        loomPath: String,
        effect: Double,
        subject: String,
        workspace: String,
        facet: String,
        rightsMask: Double,
        refGlob: String,
        scopeKinds: IntArray,
        scopePrefixes: Array<ByteArray>,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeAclGrantScopedPredicate(
        loomPath: String,
        effect: Double,
        subject: String,
        workspace: String,
        facet: String,
        rightsMask: Double,
        refGlob: String,
        scopeKinds: IntArray,
        scopePrefixes: Array<ByteArray>,
        predicateCel: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeAclRevoke(
        loomPath: String,
        effect: Double,
        subject: String,
        workspace: String,
        facet: String,
        rightsMask: Double,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeAclRevokeScoped(
        loomPath: String,
        effect: Double,
        subject: String,
        workspace: String,
        facet: String,
        rightsMask: Double,
        refGlob: String,
        scopeKinds: IntArray,
        scopePrefixes: Array<ByteArray>,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeAclRevokeScopedPredicate(
        loomPath: String,
        effect: Double,
        subject: String,
        workspace: String,
        facet: String,
        rightsMask: Double,
        refGlob: String,
        scopeKinds: IntArray,
        scopePrefixes: Array<ByteArray>,
        predicateCel: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeProtectedRefListJson(
        loomPath: String,
        workspace: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeProtectedRefGetJson(
        loomPath: String,
        workspace: String,
        refName: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeProtectedRefSet(
        loomPath: String,
        workspace: String,
        refName: String,
        fastForwardOnly: Boolean,
        signedCommitsRequired: Boolean,
        signedRefAdvanceRequired: Boolean,
        requiredReviewCount: Double,
        retentionLock: Boolean,
        governanceLock: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeProtectedRefRemove(
        loomPath: String,
        workspace: String,
        refName: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeSqlReadTable(
        loomPath: String,
        workspace: String,
        table: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSqlReadTableAt(
        loomPath: String,
        workspace: String,
        table: String,
        commit: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSqlIndexScan(
        loomPath: String,
        workspace: String,
        table: String,
        index: String,
        prefix: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSqlIndexScanAt(
        loomPath: String,
        workspace: String,
        table: String,
        index: String,
        prefix: ByteArray,
        commit: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSqlBlame(
        loomPath: String,
        workspace: String,
        branch: String,
        table: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSqlDiff(
        loomPath: String,
        workspace: String,
        table: String,
        fromCommit: String,
        toCommit: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSqlTableDiff(
        loomPath: String,
        workspace: String,
        table: String,
        fromCommit: String,
        toCommit: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeVcsBlame(
        loomPath: String,
        workspace: String,
        branch: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeVcsDiff(
        loomPath: String,
        workspace: String,
        fromCommit: String,
        toCommit: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeWatchSubscribe(
        loomPath: String,
        workspace: String,
        branch: String,
        facet: String,
        pathPrefix: String,
        changeKinds: String,
        fromCommit: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeWatchPoll(
        loomPath: String,
        cursor: String,
        max: Int,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSqlExec(
        loomPath: String,
        ns: String,
        db: String,
        sql: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeSqlExecTyped(
        loomPath: String,
        ns: String,
        db: String,
        sql: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeSqlBatch(
        loomPath: String,
        ns: String,
        db: String,
        statements: Array<String>,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeSqlExecBytes(
        loomPath: String,
        ns: String,
        db: String,
        sql: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSqlQueryBytes(
        loomPath: String,
        ns: String,
        db: String,
        sql: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Array<ByteArray>

    external fun nativeSqlCommit(
        loomPath: String,
        ns: String,
        db: String,
        message: String,
        author: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeCapabilities(): ByteArray

    external fun nativeRuntimeProfile(): ByteArray

    external fun nativeStudioSurfaceCatalogJson(workspace: String, set: String): String

    external fun nativeExecCbor(
        loomPath: String,
        request: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCasPut(
        loomPath: String,
        workspace: String,
        content: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeCasGet(
        loomPath: String,
        workspace: String,
        digest: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeCasHas(
        loomPath: String,
        workspace: String,
        digest: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeCasListJson(
        loomPath: String,
        workspace: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeCasDelete(
        loomPath: String,
        workspace: String,
        digest: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeMeetingsImportSnapshot(
        loomPath: String,
        workspace: String,
        inputProfile: String,
        snapshot: ByteArray,
        dryRun: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeMeetingsSourceRead(
        loomPath: String,
        workspace: String,
        sourceId: String,
        leaf: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeDriveListJson(loomPath: String, workspace: String, driveWorkspaceId: String, folderId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveStatJson(loomPath: String, workspace: String, driveWorkspaceId: String, folderId: String, name: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveReadFile(loomPath: String, workspace: String, driveWorkspaceId: String, fileId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): ByteArray
    external fun nativeDriveListVersionsJson(loomPath: String, workspace: String, driveWorkspaceId: String, fileId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveListConflictsJson(loomPath: String, workspace: String, driveWorkspaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveListSharesJson(loomPath: String, workspace: String, driveWorkspaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveListRetentionJson(loomPath: String, workspace: String, driveWorkspaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveCreateFolderJson(loomPath: String, workspace: String, driveWorkspaceId: String, parentFolderId: String, folderId: String, name: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveCreateUploadJson(loomPath: String, workspace: String, driveWorkspaceId: String, uploadId: String, parentFolderId: String, name: String, fileId: String, expectedRoot: String, createdAtMs: String, replaceFile: Boolean, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveUploadChunkJson(loomPath: String, workspace: String, driveWorkspaceId: String, uploadId: String, chunk: ByteArray, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveCommitUploadJson(loomPath: String, workspace: String, driveWorkspaceId: String, uploadId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveRenameJson(loomPath: String, workspace: String, driveWorkspaceId: String, folderId: String, nodeId: String, newName: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveMoveJson(loomPath: String, workspace: String, driveWorkspaceId: String, sourceFolderId: String, targetFolderId: String, nodeId: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveDeleteJson(loomPath: String, workspace: String, driveWorkspaceId: String, folderId: String, nodeId: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveResolveConflictJson(loomPath: String, workspace: String, driveWorkspaceId: String, conflictId: String, resolution: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveGrantShareJson(loomPath: String, workspace: String, driveWorkspaceId: String, grantId: String, targetKind: String, targetId: String, principal: String, role: String, grantedAtMs: String, expiresAtMs: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveRevokeShareJson(loomPath: String, workspace: String, driveWorkspaceId: String, grantId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveApplyShareExpiryJson(loomPath: String, workspace: String, driveWorkspaceId: String, nowMs: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDrivePinRetentionJson(loomPath: String, workspace: String, driveWorkspaceId: String, pinId: String, kind: String, root: String, targetEntityId: String, addedAtMs: String, expiresAtMs: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveUnpinRetentionJson(loomPath: String, workspace: String, driveWorkspaceId: String, pinId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeDriveApplyRetentionJson(loomPath: String, workspace: String, driveWorkspaceId: String, nowMs: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsProjectCreateJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projectId: String, keyPrefix: String, name: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsProjectRekeyJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projectId: String, keyPrefix: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsProjectSettingsGetJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projectId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsProjectSettingsSetJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projectId: String, defaultProjection: String, enableProjectionsJson: String, disableProjectionsJson: String, actorEnforcement: String, projectOwnerPrincipal: String, clearProjectOwnerPrincipal: Boolean, acceptanceAuthoritiesJson: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsFieldsJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projectId: String, projection: String, operation: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsFieldPutJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projectId: String, fieldId: String, fieldKey: String, name: String, description: String, fieldType: String, optionSet: String, maxLength: Double, hasMaxLength: Boolean, required: Boolean, searchable: Boolean, orderable: Boolean, cardinality: String, applicableTypeIdsJson: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsFieldRetireJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projectId: String, fieldId: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsCreateJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projectId: String, ticketType: String, externalSource: String, externalId: String, fieldsJson: String, policyLabelsJson: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsUpdateJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, setFieldsJson: String, deleteFieldsJson: String, action: String, targetStatus: String, observedSourceStatus: String, observedWorkflowVersion: String, assignee: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray, commentId: String, commentType: String, commentBody: String, commentsJson: String, relationSetsJson: String, relationRemovesJson: String): String
    external fun nativeTicketsDeleteJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsCommentsJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsCommentAddJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, commentId: String, commentType: String, body: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsCommentUpdateJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, commentId: String, commentType: String, body: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsCommentDeleteJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, commentId: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsRelationSetJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, relationId: String, kind: String, targetId: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsRelationRemoveJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, relationId: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsGetJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, projection: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsListJson(loomPath: String, workspace: String, ticketWorkspaceId: String, projection: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeTicketsHistoryJson(loomPath: String, workspace: String, ticketWorkspaceId: String, ticketId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeSpacesCreateJson(loomPath: String, workspace: String, pageWorkspaceId: String, spaceId: String, title: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeSpacesListJson(loomPath: String, workspace: String, pageWorkspaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeSpacesGetJson(loomPath: String, workspace: String, pageWorkspaceId: String, spaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativePagesCreateJson(loomPath: String, workspace: String, pageWorkspaceId: String, pageId: String, spaceId: String, parentPageId: String, title: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativePagesUpdateJson(loomPath: String, workspace: String, pageWorkspaceId: String, pageId: String, bodyText: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativePagesPublishJson(loomPath: String, workspace: String, pageWorkspaceId: String, pageId: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativePagesGetJson(loomPath: String, workspace: String, pageWorkspaceId: String, pageId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativePagesListJson(loomPath: String, workspace: String, pageWorkspaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativePagesHistoryJson(loomPath: String, workspace: String, pageWorkspaceId: String, pageId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresCreateJson(loomPath: String, workspace: String, pageWorkspaceId: String, structureId: String, spaceId: String, kind: String, title: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresAddNodeJson(loomPath: String, workspace: String, pageWorkspaceId: String, structureId: String, nodeId: String, kind: String, label: String, bodyDigest: String, entityRef: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresUpdateNodeJson(loomPath: String, workspace: String, pageWorkspaceId: String, structureId: String, nodeId: String, kind: String, label: String, bodyDigest: String, entityRef: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresBindJson(loomPath: String, workspace: String, pageWorkspaceId: String, structureId: String, nodeId: String, entityRef: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresMoveNodeJson(loomPath: String, workspace: String, pageWorkspaceId: String, structureId: String, nodeId: String, parentNodeId: String, label: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresLinkNodeJson(loomPath: String, workspace: String, pageWorkspaceId: String, structureId: String, edgeId: String, srcNodeId: String, dstNodeId: String, label: String, targetRef: String, expectedRoot: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresDecomposeToTicketsJson(loomPath: String, workspace: String, pageWorkspaceId: String, structureId: String, itemsJson: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresGetJson(loomPath: String, workspace: String, pageWorkspaceId: String, structureId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeStructuresListJson(loomPath: String, workspace: String, pageWorkspaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeLanesCreate(loomPath: String, workspace: String, lane: ByteArray, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): ByteArray
    external fun nativeLanesGet(loomPath: String, workspace: String, laneId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): ByteArray?
    external fun nativeLanesList(loomPath: String, workspace: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): ByteArray
    external fun nativeLanesUpdate(loomPath: String, workspace: String, laneId: String, title: String?, description: String?, laneStatus: String?, statusReport: String?, reviewerFeedback: String?, updatedBy: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): ByteArray
    external fun nativeLanesTicketAdd(loomPath: String, workspace: String, laneId: String, ticketId: String, updatedBy: String, placement: String, anchor: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): ByteArray
    external fun nativeLanesTicketRemove(loomPath: String, workspace: String, laneId: String, ticketId: String, updatedBy: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): ByteArray
    external fun nativeChatCreateChannelJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, channelHandle: String, name: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatRenameChannelJson(loomPath: String, workspace: String, chatWorkspaceId: String, selector: String, channelHandle: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatListChannelsJson(loomPath: String, workspace: String, chatWorkspaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatPostMessageJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, threadId: String, bodyText: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatEditMessageJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, bodyText: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatRedactMessageJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, reason: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatCreateThreadJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, threadId: String, parentMessageId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatCreateTaskJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, taskId: String, messageId: String, title: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatClaimTaskJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, taskId: String, claimId: String, leaseToken: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatCompleteTaskJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, taskId: String, claimId: String, resultMessageId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatInvokeAgentJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, invocationId: String, agentPrincipal: String, sourceMessageIdsJson: String, promptText: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatAgentReplyJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, invocationId: String, messageId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatRequestHandoffJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, handoffId: String, fromAgentPrincipal: String, toPrincipal: String, reason: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatAddReactionJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, kind: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatRemoveReactionJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, kind: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatEmojiListJson(loomPath: String, workspace: String, chatWorkspaceId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatEmojiRegisterJson(loomPath: String, workspace: String, chatWorkspaceId: String, kind: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatEmojiUnregisterJson(loomPath: String, workspace: String, chatWorkspaceId: String, kind: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatMessagesJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatCursorJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatUpdateCursorJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, nextSequence: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String
    external fun nativeChatFetchEventsJson(loomPath: String, workspace: String, chatWorkspaceId: String, channelId: String, fromSequence: String, max: String, passphrase: ByteArray, kek: ByteArray, authPrincipal: String, authPassphrase: ByteArray): String

    external fun nativeFsImport(
        loomPath: String,
        workspace: String,
        srcPath: String,
        commit: Boolean,
        dryRun: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeFsExport(
        loomPath: String,
        workspace: String,
        dstPath: String,
        revision: String,
        dryRun: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeArchiveImport(
        loomPath: String,
        workspace: String,
        srcPath: String,
        kind: String,
        dryRun: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeArchiveExport(
        loomPath: String,
        workspace: String,
        dstPath: String,
        kind: String,
        revision: String,
        dryRun: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCarImport(
        loomPath: String,
        srcPath: String,
        dryRun: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCarExport(
        loomPath: String,
        workspace: String,
        dstPath: String,
        dryRun: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeKvPut(
        loomPath: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        value: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeKvGet(
        loomPath: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeKvDelete(
        loomPath: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeKvList(
        loomPath: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeKvRange(
        loomPath: String,
        workspace: String,
        collection: String,
        lo: ByteArray,
        hi: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeDocPutText(
        loomPath: String,
        workspace: String,
        collection: String,
        id: String,
        text: String,
        expectedEntityTag: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Array<Any>

    external fun nativeDocGetText(
        loomPath: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Array<Any>?

    external fun nativeDocPutBinary(
        loomPath: String,
        workspace: String,
        collection: String,
        id: String,
        bytes: ByteArray,
        expectedEntityTag: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Array<Any>

    external fun nativeDocGetBinary(
        loomPath: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Array<Any>?

    external fun nativeDocDelete(
        loomPath: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeDocListBinary(
        loomPath: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeDocIndexCreate(
        loomPath: String,
        workspace: String,
        collection: String,
        name: String,
        fieldPath: String,
        unique: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeDocIndexCreateJson(
        loomPath: String,
        workspace: String,
        collection: String,
        declarationJson: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeDocIndexDrop(
        loomPath: String,
        workspace: String,
        collection: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeDocIndexRebuild(
        loomPath: String,
        workspace: String,
        collection: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeDocIndexListJson(
        loomPath: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeDocIndexStatusJson(
        loomPath: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeDocFindJson(
        loomPath: String,
        workspace: String,
        collection: String,
        index: String,
        valueJson: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeDocQueryJson(
        loomPath: String,
        workspace: String,
        collection: String,
        queryJson: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeTsPut(
        loomPath: String,
        workspace: String,
        collection: String,
        ts: Long,
        value: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeTsGet(
        loomPath: String,
        workspace: String,
        collection: String,
        ts: Long,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeTsRange(
        loomPath: String,
        workspace: String,
        collection: String,
        from: Long,
        to: Long,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeTsLatest(
        loomPath: String,
        workspace: String,
        collection: String,
        outTs: LongArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeMetricsPutDescriptor(
        loomPath: String,
        workspace: String,
        descriptor: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeMetricsGetDescriptor(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeMetricsPutObservation(
        loomPath: String,
        workspace: String,
        descriptorName: String,
        observation: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeMetricsQuery(
        loomPath: String,
        workspace: String,
        descriptorName: String,
        fromTimestampMs: String,
        toTimestampMs: String,
        maxSeries: Double,
        maxGroups: Double,
        maxSamples: Double,
        maxOutputBytes: String,
        nowTimestampMs: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeLogsPutRecord(
        loomPath: String,
        workspace: String,
        record: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeLogsGetRecord(
        loomPath: String,
        workspace: String,
        recordId: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeLogsQuery(
        loomPath: String,
        workspace: String,
        fromTimeUnixNano: String,
        toTimeUnixNano: String,
        maxRecords: Double,
        maxOutputBytes: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeTracesPutSpan(
        loomPath: String,
        workspace: String,
        span: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeTracesGetSpan(
        loomPath: String,
        workspace: String,
        traceId: String,
        spanId: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeTracesTraceSpans(
        loomPath: String,
        workspace: String,
        traceId: String,
        maxSpans: Double,
        maxOutputBytes: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeTracesQuery(
        loomPath: String,
        workspace: String,
        fromStartTimeNs: String,
        toStartTimeNs: String,
        maxSpans: Double,
        maxOutputBytes: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeLedgerAppend(
        loomPath: String,
        workspace: String,
        collection: String,
        payload: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeLedgerGet(
        loomPath: String,
        workspace: String,
        collection: String,
        seq: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeLedgerHead(
        loomPath: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String?

    external fun nativeLedgerLen(
        loomPath: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeLedgerVerify(
        loomPath: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeCalCreateCollection(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        displayName: String,
        components: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeCalDeleteCollection(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeCalListCollections(
        loomPath: String,
        workspace: String,
        principal: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCalPutEntry(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        entry: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeCalGetEntry(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeCalDeleteEntry(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeCalListEntries(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCalRange(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        from: String,
        to: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCalSearch(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        component: String,
        text: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCalEntryIcs(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String?

    external fun nativeCalPutIcs(
        loomPath: String,
        workspace: String,
        principal: String,
        collection: String,
        ics: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeCardCreateBook(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        displayName: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeCardDeleteBook(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeCardListBooks(
        loomPath: String,
        workspace: String,
        principal: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCardPutEntry(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        entry: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeCardGetEntry(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeCardDeleteEntry(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeCardListEntries(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCardSearch(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        text: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeCardEntryVcard(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String?

    external fun nativeCardPutVcard(
        loomPath: String,
        workspace: String,
        principal: String,
        book: String,
        vcf: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeMailCreateMailbox(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        displayName: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeMailDeleteMailbox(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeMailListMailboxes(
        loomPath: String,
        workspace: String,
        principal: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeMailIngestMessage(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        raw: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeMailGetMessage(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeMailToEml(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeMailDeleteMessage(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeMailListMessages(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeMailGetFlags(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeMailSetFlags(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        flags: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeMailSearch(
        loomPath: String,
        workspace: String,
        principal: String,
        mailbox: String,
        text: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeGraphUpsertNode(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        props: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeGraphGetNode(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeGraphRemoveNode(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        cascade: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeGraphUpsertEdge(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeGraphGetEdge(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeGraphRemoveEdge(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeGraphNeighbors(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeGraphOutEdges(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeGraphInEdges(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeGraphReachable(
        loomPath: String,
        workspace: String,
        name: String,
        start: String,
        maxDepth: Long,
        viaLabel: String?,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeGraphShortestPath(
        loomPath: String,
        workspace: String,
        name: String,
        from: String,
        to: String,
        viaLabel: String?,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeVectorCreate(
        loomPath: String,
        workspace: String,
        name: String,
        dim: Long,
        metric: Int,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeVectorUpsert(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeVectorUpsertSource(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        sourceText: ByteArray,
        modelId: String?,
        weightsDigest: String?,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeVectorGet(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeVectorSourceText(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeVectorEmbeddingModel(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeVectorIds(
        loomPath: String,
        workspace: String,
        name: String,
        prefix: String?,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeVectorMetadataIndexKeys(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeVectorCreateMetadataIndex(
        loomPath: String,
        workspace: String,
        name: String,
        key: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeVectorDropMetadataIndex(
        loomPath: String,
        workspace: String,
        name: String,
        key: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeVectorDelete(
        loomPath: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeVectorSearch(
        loomPath: String,
        workspace: String,
        name: String,
        query: ByteArray,
        k: Long,
        filter: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeVectorSearchPolicy(
        loomPath: String,
        workspace: String,
        name: String,
        query: ByteArray,
        k: Long,
        filter: ByteArray,
        policy: Int,
        threshold: Long,
        ef: Long,
        pqM: Long,
        pqK: Long,
        pqIters: Long,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeColumnarCreate(
        loomPath: String,
        workspace: String,
        name: String,
        columns: ByteArray,
        targetSegmentRows: Long,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeColumnarAppend(
        loomPath: String,
        workspace: String,
        name: String,
        row: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeColumnarScan(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeColumnarColumns(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeColumnarRows(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Double

    external fun nativeColumnarCompact(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeColumnarInspect(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeColumnarSourceDigest(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeColumnarSelect(
        loomPath: String,
        workspace: String,
        name: String,
        columns: ByteArray,
        filter: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeColumnarAggregate(
        loomPath: String,
        workspace: String,
        name: String,
        aggregates: ByteArray,
        filter: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeDataframeCreate(
        loomPath: String,
        workspace: String,
        name: String,
        plan: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeDataframeCollect(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeDataframePreview(
        loomPath: String,
        workspace: String,
        name: String,
        rows: Long,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeDataframeMaterialize(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String?

    external fun nativeDataframePlanDigest(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): String

    external fun nativeDataframeSourceDigests(
        loomPath: String,
        workspace: String,
        name: String,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSearchCreate(
        loomPath: String,
        workspace: String,
        name: String,
        mapping: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeSearchIndex(
        loomPath: String,
        workspace: String,
        name: String,
        id: ByteArray,
        doc: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeSearchGet(
        loomPath: String,
        workspace: String,
        name: String,
        id: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray?

    external fun nativeSearchDelete(
        loomPath: String,
        workspace: String,
        name: String,
        id: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): Boolean

    external fun nativeSearchIds(
        loomPath: String,
        workspace: String,
        name: String,
        prefix: ByteArray,
        hasPrefix: Boolean,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray

    external fun nativeSearchRemap(
        loomPath: String,
        workspace: String,
        name: String,
        mapping: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    )

    external fun nativeSearchQuery(
        loomPath: String,
        workspace: String,
        name: String,
        request: ByteArray,
        passphrase: ByteArray,
        kek: ByteArray,
        authPrincipal: String,
        authPassphrase: ByteArray,
    ): ByteArray
}
