package ai.uldren.loom

// Desktop/server JVM (off Android). Ensure libuldren_loom_jni.{so,dylib,dll} (and the
// libuldren_loom it links) are on java.library.path, e.g. -Djava.library.path=../../target/release.
actual object Loom {
    init {
        System.loadLibrary("uldren_loom_jni")
    }

    actual external fun version(): String

    actual external fun blobDigest(data: ByteArray): String

    actual external fun capabilities(): ByteArray

    actual external fun runtimeProfile(): ByteArray

    actual external fun studioSurfaceCatalogJson(workspace: String, set: String): String

    actual fun execCbor(
        path: String,
        request: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray = LoomNative.nativeExecCbor(
        path,
        request,
        passphrase,
        kek,
        authPrincipal,
        authPassphrase,
    )

}

/** Raw JNI surface for the Loom facet operations. Internal FFI boundary;
 *  the public API is the `Loom.<op>(...)` extension functions in facets/. */
internal object LoomNative {
    init {
        System.loadLibrary("uldren_loom_jni")
    }


    external fun nativeQueueAppend(
        path: String,
        workspace: String,
        stream: String,
        entry: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Long


    external fun nativeQueueGet(
        path: String,
        workspace: String,
        stream: String,
        seq: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeQueueRange(
        path: String,
        workspace: String,
        stream: String,
        lo: Long,
        hi: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeQueueLen(
        path: String,
        workspace: String,
        stream: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Long


    external fun nativeQueueConsumerPosition(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Long


    external fun nativeQueueConsumerRead(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        max: Int,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeQueueConsumerAdvance(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        nextSeq: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeQueueConsumerReset(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        nextSeq: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeCreate(
        path: String,
        profile: String,
        suite: String?,
        passphrase: ByteArray?,
    )


    external fun nativeCreateWithKek(
        path: String,
        profile: String,
        suite: String?,
        kek: ByteArray?,
    )


    external fun nativeWorkspaceCreate(
        path: String,
        name: String?,
        facet: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String


    external fun nativeWorkspaceListJson(
        path: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String


    external fun nativeWorkspaceRename(
        path: String,
        workspace: String,
        newName: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeWorkspaceDelete(
        path: String,
        workspace: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeAuthenticatePassphrase(
        path: String,
        principal: String,
        principalPassphrase: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
    )

    external fun nativeExecCbor(
        path: String,
        request: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeIdentityListJson(
        path: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeIdentityAddPrincipal(
        path: String,
        principalHandle: String,
        name: String,
        kind: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeIdentityRenamePrincipalHandle(
        path: String,
        principal: String,
        principalHandle: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeIdentitySetPassphrase(
        path: String,
        principal: String,
        principalPassphrase: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeIdentityRemovePrincipal(
        path: String,
        principal: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeIdentityAssignRole(
        path: String,
        principal: String,
        role: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeIdentityRevokeRole(
        path: String,
        principal: String,
        role: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean

    external fun nativeIdentityCreateExternalCredential(
        path: String,
        principal: String,
        kind: String,
        label: String,
        issuer: String,
        subject: String,
        materialDigest: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeIdentityRevokeExternalCredential(
        path: String,
        credential: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeIdentityAddPublicKey(
        path: String,
        principal: String,
        label: String,
        algorithm: String,
        publicKeyHex: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeIdentityRevokePublicKey(
        path: String,
        key: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeAclListJson(
        path: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeAclGrant(
        path: String,
        effect: Int,
        subject: String,
        workspace: String?,
        domain: String?,
        rightsMask: Int,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeAclRevoke(
        path: String,
        effect: Int,
        subject: String,
        workspace: String?,
        domain: String?,
        rightsMask: Int,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean

    external fun nativeAclGrantScoped(
        path: String,
        effect: Int,
        subject: String,
        workspace: String?,
        domain: String?,
        rightsMask: Int,
        refGlob: String?,
        scopeKinds: IntArray,
        scopePrefixes: Array<ByteArray>,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeAclGrantScopedPredicate(
        path: String,
        effect: Int,
        subject: String,
        workspace: String?,
        domain: String?,
        rightsMask: Int,
        refGlob: String?,
        scopeKinds: IntArray,
        scopePrefixes: Array<ByteArray>,
        predicateCel: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeAclRevokeScoped(
        path: String,
        effect: Int,
        subject: String,
        workspace: String?,
        domain: String?,
        rightsMask: Int,
        refGlob: String?,
        scopeKinds: IntArray,
        scopePrefixes: Array<ByteArray>,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean

    external fun nativeAclRevokeScopedPredicate(
        path: String,
        effect: Int,
        subject: String,
        workspace: String?,
        domain: String?,
        rightsMask: Int,
        refGlob: String?,
        scopeKinds: IntArray,
        scopePrefixes: Array<ByteArray>,
        predicateCel: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean

    external fun nativeProtectedRefListJson(
        path: String,
        workspace: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeProtectedRefGetJson(
        path: String,
        workspace: String,
        refName: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeProtectedRefSet(
        path: String,
        workspace: String,
        refName: String,
        fastForwardOnly: Boolean,
        signedCommitsRequired: Boolean,
        signedRefAdvanceRequired: Boolean,
        requiredReviewCount: Int,
        retentionLock: Boolean,
        governanceLock: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeProtectedRefRemove(
        path: String,
        workspace: String,
        refName: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean

    external fun nativeSqlReadTable(
        path: String,
        workspace: String,
        table: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeSqlReadTableAt(
        path: String,
        workspace: String,
        table: String,
        commit: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeSqlIndexScan(
        path: String,
        workspace: String,
        table: String,
        index: String,
        prefix: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeSqlIndexScanAt(
        path: String,
        workspace: String,
        table: String,
        index: String,
        prefix: ByteArray,
        commit: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeSqlBlame(
        path: String,
        workspace: String,
        branch: String,
        table: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeSqlDiff(
        path: String,
        workspace: String,
        table: String,
        fromCommit: String,
        toCommit: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeSqlTableDiff(
        path: String,
        workspace: String,
        table: String,
        fromCommit: String,
        toCommit: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeVcsBlame(
        path: String,
        workspace: String,
        branch: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeVcsDiff(
        path: String,
        workspace: String,
        fromCommit: String,
        toCommit: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeWatchSubscribe(
        path: String,
        workspace: String,
        branch: String,
        facet: String?,
        pathPrefix: String?,
        changeKinds: String?,
        fromCommit: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeWatchPoll(
        path: String,
        cursor: String,
        max: Int,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeDocPutText(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        text: String,
        expectedEntityTag: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Array<Any>


    external fun nativeDocGetText(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Array<Any>?


    external fun nativeDocPutBinary(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        bytes: ByteArray,
        expectedEntityTag: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Array<Any>


    external fun nativeDocGetBinary(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Array<Any>?


    external fun nativeDocDelete(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeDocListBinary(
        path: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeMetricsPutDescriptor(
        path: String,
        workspace: String,
        descriptor: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeMetricsGetDescriptor(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?

    external fun nativeMetricsPutObservation(
        path: String,
        workspace: String,
        descriptorName: String,
        observation: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeMetricsQuery(
        path: String,
        workspace: String,
        descriptorName: String,
        fromTimestampMs: Long,
        toTimestampMs: Long,
        maxSeries: Int,
        maxGroups: Int,
        maxSamples: Int,
        maxOutputBytes: Long,
        nowTimestampMs: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeLogsPutRecord(
        path: String,
        workspace: String,
        record: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeLogsGetRecord(
        path: String,
        workspace: String,
        recordId: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?

    external fun nativeLogsQuery(
        path: String,
        workspace: String,
        fromTimeUnixNano: Long,
        toTimeUnixNano: Long,
        maxRecords: Int,
        maxOutputBytes: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeTracesPutSpan(
        path: String,
        workspace: String,
        span: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeTracesGetSpan(
        path: String,
        workspace: String,
        traceId: String,
        spanId: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?

    external fun nativeTracesTraceSpans(
        path: String,
        workspace: String,
        traceId: String,
        maxSpans: Int,
        maxOutputBytes: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeTracesQuery(
        path: String,
        workspace: String,
        fromStartTimeNs: Long,
        toStartTimeNs: Long,
        maxSpans: Int,
        maxOutputBytes: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeDocIndexCreate(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        fieldPath: String,
        unique: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeDocIndexCreateJson(
        path: String,
        workspace: String,
        collection: String,
        declarationJson: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeDocIndexDrop(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean

    external fun nativeDocIndexRebuild(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeDocIndexListJson(
        path: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeDocIndexStatusJson(
        path: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeDocFindJson(
        path: String,
        workspace: String,
        collection: String,
        index: String,
        valueJson: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeDocQueryJson(
        path: String,
        workspace: String,
        collection: String,
        queryJson: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String


    external fun nativeTsPut(
        path: String,
        workspace: String,
        collection: String,
        ts: Long,
        value: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeTsGet(
        path: String,
        workspace: String,
        collection: String,
        ts: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeTsRange(
        path: String,
        workspace: String,
        collection: String,
        from: Long,
        to: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeTsLatest(
        path: String,
        workspace: String,
        collection: String,
        outTs: LongArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeLedgerAppend(
        path: String,
        workspace: String,
        collection: String,
        payload: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Long


    external fun nativeLedgerGet(
        path: String,
        workspace: String,
        collection: String,
        seq: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeLedgerHead(
        path: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String?


    external fun nativeLedgerLen(
        path: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Long


    external fun nativeLedgerVerify(
        path: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeKvPut(
        path: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        value: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeKvGet(
        path: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeKvDelete(
        path: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeKvList(
        path: String,
        workspace: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeKvRange(
        path: String,
        workspace: String,
        collection: String,
        lo: ByteArray,
        hi: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCasPut(
        path: String,
        workspace: String,
        content: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String


    external fun nativeCasGet(
        path: String,
        workspace: String,
        digest: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeCasHas(
        path: String,
        workspace: String,
        digest: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeCasListJson(
        path: String,
        workspace: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String


    external fun nativeCasDelete(
        path: String,
        workspace: String,
        digest: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean

    external fun nativeMeetingsImportSnapshot(
        path: String,
        workspace: String,
        inputProfile: String,
        snapshot: ByteArray,
        dryRun: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeMeetingsSourceRead(
        path: String,
        workspace: String,
        sourceId: String,
        leaf: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeDriveListJson(path: String, workspace: String, driveWorkspaceId: String, folderId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveStatJson(path: String, workspace: String, driveWorkspaceId: String, folderId: String, name: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveReadFile(path: String, workspace: String, driveWorkspaceId: String, fileId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): ByteArray
    external fun nativeDriveListVersionsJson(path: String, workspace: String, driveWorkspaceId: String, fileId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveListConflictsJson(path: String, workspace: String, driveWorkspaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveListSharesJson(path: String, workspace: String, driveWorkspaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveListRetentionJson(path: String, workspace: String, driveWorkspaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveCreateFolderJson(path: String, workspace: String, driveWorkspaceId: String, parentFolderId: String, folderId: String, name: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveCreateUploadJson(path: String, workspace: String, driveWorkspaceId: String, uploadId: String, parentFolderId: String, name: String, fileId: String, expectedRoot: String, createdAtMs: Long, replaceFile: Boolean, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveUploadChunkJson(path: String, workspace: String, driveWorkspaceId: String, uploadId: String, chunk: ByteArray, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveCommitUploadJson(path: String, workspace: String, driveWorkspaceId: String, uploadId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveRenameJson(path: String, workspace: String, driveWorkspaceId: String, folderId: String, nodeId: String, newName: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveMoveJson(path: String, workspace: String, driveWorkspaceId: String, sourceFolderId: String, targetFolderId: String, nodeId: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveDeleteJson(path: String, workspace: String, driveWorkspaceId: String, folderId: String, nodeId: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveResolveConflictJson(path: String, workspace: String, driveWorkspaceId: String, conflictId: String, resolution: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveGrantShareJson(path: String, workspace: String, driveWorkspaceId: String, grantId: String, targetKind: String, targetId: String, principal: String, role: String, grantedAtMs: Long, expiresAtMs: Long, hasExpiresAtMs: Boolean, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveRevokeShareJson(path: String, workspace: String, driveWorkspaceId: String, grantId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveApplyShareExpiryJson(path: String, workspace: String, driveWorkspaceId: String, nowMs: Long, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDrivePinRetentionJson(path: String, workspace: String, driveWorkspaceId: String, pinId: String, kind: String, root: String, targetEntityId: String?, addedAtMs: Long, expiresAtMs: Long, hasExpiresAtMs: Boolean, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveUnpinRetentionJson(path: String, workspace: String, driveWorkspaceId: String, pinId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeDriveApplyRetentionJson(path: String, workspace: String, driveWorkspaceId: String, nowMs: Long, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsProjectCreateJson(path: String, workspace: String, ticketWorkspaceId: String, projectId: String, keyPrefix: String, name: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsProjectRekeyJson(path: String, workspace: String, ticketWorkspaceId: String, projectId: String, keyPrefix: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsProjectSettingsGetJson(path: String, workspace: String, ticketWorkspaceId: String, projectId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsProjectSettingsSetJson(path: String, workspace: String, ticketWorkspaceId: String, projectId: String, defaultProjection: String?, enableProjectionsJson: String, disableProjectionsJson: String, actorEnforcement: String?, projectOwnerPrincipal: String?, clearProjectOwnerPrincipal: Boolean, acceptanceAuthoritiesJson: String?, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsFieldsJson(path: String, workspace: String, ticketWorkspaceId: String, projectId: String, projection: String, operation: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsFieldPutJson(path: String, workspace: String, ticketWorkspaceId: String, projectId: String, fieldId: String, key: String, name: String, description: String?, fieldType: String, optionSet: String?, maxLength: Int, hasMaxLength: Boolean, required: Boolean, searchable: Boolean, orderable: Boolean, cardinality: String, applicableTypeIdsJson: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsFieldRetireJson(path: String, workspace: String, ticketWorkspaceId: String, projectId: String, fieldId: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsCreateJson(path: String, workspace: String, ticketWorkspaceId: String, projectId: String, ticketType: String, externalSource: String?, externalId: String?, fieldsJson: String, policyLabelsJson: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsUpdateJson(path: String, workspace: String, ticketWorkspaceId: String, ticketId: String, setFieldsJson: String, deleteFieldsJson: String, action: String?, targetStatus: String?, observedSourceStatus: String?, observedWorkflowVersion: String?, assignee: String?, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?, commentId: String?, commentType: String?, commentBody: String?, commentsJson: String?, relationSetsJson: String?, relationRemovesJson: String?): String
    external fun nativeTicketsDeleteJson(path: String, workspace: String, ticketWorkspaceId: String, ticketId: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsRelationSetJson(path: String, workspace: String, ticketWorkspaceId: String, ticketId: String, relationId: String, kind: String, targetId: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsRelationRemoveJson(path: String, workspace: String, ticketWorkspaceId: String, ticketId: String, relationId: String, expectedRoot: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsGetJson(path: String, workspace: String, ticketWorkspaceId: String, ticketId: String, projection: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsListJson(path: String, workspace: String, ticketWorkspaceId: String, projection: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeTicketsHistoryJson(path: String, workspace: String, ticketWorkspaceId: String, ticketId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeSpacesCreateJson(path: String, workspace: String, pageWorkspaceId: String, spaceId: String, title: String, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeSpacesListJson(path: String, workspace: String, pageWorkspaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeSpacesGetJson(path: String, workspace: String, pageWorkspaceId: String, spaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativePagesCreateJson(path: String, workspace: String, pageWorkspaceId: String, pageId: String, spaceId: String, parentPageId: String?, title: String, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativePagesUpdateJson(path: String, workspace: String, pageWorkspaceId: String, pageId: String, bodyText: String, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativePagesPublishJson(path: String, workspace: String, pageWorkspaceId: String, pageId: String, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativePagesGetJson(path: String, workspace: String, pageWorkspaceId: String, pageId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativePagesListJson(path: String, workspace: String, pageWorkspaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativePagesHistoryJson(path: String, workspace: String, pageWorkspaceId: String, pageId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresCreateJson(path: String, workspace: String, pageWorkspaceId: String, structureId: String, spaceId: String, kind: String, title: String, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresAddNodeJson(path: String, workspace: String, pageWorkspaceId: String, structureId: String, nodeId: String, kind: String, label: String, bodyDigest: String?, entityRef: String?, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresUpdateNodeJson(path: String, workspace: String, pageWorkspaceId: String, structureId: String, nodeId: String, kind: String, label: String, bodyDigest: String?, entityRef: String?, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresBindJson(path: String, workspace: String, pageWorkspaceId: String, structureId: String, nodeId: String, entityRef: String?, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresMoveNodeJson(path: String, workspace: String, pageWorkspaceId: String, structureId: String, nodeId: String, parentNodeId: String?, label: String?, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresLinkNodeJson(path: String, workspace: String, pageWorkspaceId: String, structureId: String, edgeId: String, srcNodeId: String, dstNodeId: String, label: String, targetRef: String?, expectedRoot: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresDecomposeToTicketsJson(path: String, workspace: String, pageWorkspaceId: String, structureId: String, itemsJson: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresGetJson(path: String, workspace: String, pageWorkspaceId: String, structureId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeStructuresListJson(path: String, workspace: String, pageWorkspaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeLanesCreate(path: String, workspace: String, lane: ByteArray, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): ByteArray
    external fun nativeLanesGet(path: String, workspace: String, laneId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): ByteArray?
    external fun nativeLanesList(path: String, workspace: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): ByteArray
    external fun nativeLanesUpdate(path: String, workspace: String, laneId: String, title: String?, description: String?, laneStatus: String?, statusReport: String?, reviewerFeedback: String?, updatedBy: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): ByteArray
    external fun nativeLanesTicketAdd(path: String, workspace: String, laneId: String, ticketId: String, updatedBy: String, placement: String, anchor: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): ByteArray
    external fun nativeLanesTicketRemove(path: String, workspace: String, laneId: String, ticketId: String, updatedBy: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): ByteArray
    external fun nativeChatCreateChannelJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, channelHandle: String, name: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatRenameChannelJson(path: String, workspace: String, chatWorkspaceId: String, selector: String, channelHandle: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatListChannelsJson(path: String, workspace: String, chatWorkspaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatPostMessageJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, threadId: String?, bodyText: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatEditMessageJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, bodyText: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatRedactMessageJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, reason: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatCreateThreadJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, threadId: String, parentMessageId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatCreateTaskJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, taskId: String, messageId: String, title: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatClaimTaskJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, taskId: String, claimId: String, leaseToken: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatCompleteTaskJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, taskId: String, claimId: String, resultMessageId: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatInvokeAgentJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, invocationId: String, agentPrincipal: String, sourceMessageIdsJson: String, promptText: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatAgentReplyJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, invocationId: String, messageId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatRequestHandoffJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, handoffId: String, fromAgentPrincipal: String, toPrincipal: String?, reason: String?, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatAddReactionJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, kind: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatRemoveReactionJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, messageId: String, kind: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatEmojiListJson(path: String, workspace: String, chatWorkspaceId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatEmojiRegisterJson(path: String, workspace: String, chatWorkspaceId: String, kind: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatEmojiUnregisterJson(path: String, workspace: String, chatWorkspaceId: String, kind: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatMessagesJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatCursorJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatUpdateCursorJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, nextSequence: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String
    external fun nativeChatFetchEventsJson(path: String, workspace: String, chatWorkspaceId: String, channelId: String, fromSequence: String, max: String, passphrase: ByteArray?, kek: ByteArray?, authPrincipal: String?, authPassphrase: ByteArray?): String

    external fun nativeFsImport(
        path: String,
        workspace: String,
        srcPath: String,
        commit: Boolean,
        dryRun: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeFsExport(
        path: String,
        workspace: String,
        dstPath: String,
        revision: String?,
        dryRun: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeArchiveImport(
        path: String,
        workspace: String,
        srcPath: String,
        kind: String,
        dryRun: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeArchiveExport(
        path: String,
        workspace: String,
        dstPath: String,
        kind: String,
        revision: String?,
        dryRun: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeCarImport(
        path: String,
        srcPath: String,
        dryRun: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeCarExport(
        path: String,
        workspace: String,
        dstPath: String,
        dryRun: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCalCreateCollection(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        displayName: String,
        components: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeCalDeleteCollection(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeCalListCollections(
        path: String,
        workspace: String,
        principal: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCalPutEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        entry: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeCalGetEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeCalDeleteEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeCalListEntries(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCalRange(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        from: String,
        to: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCalSearch(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        component: String,
        text: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCalEntryIcs(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String?


    external fun nativeCalPutIcs(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        ics: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String


    external fun nativeCardCreateBook(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        displayName: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeCardDeleteBook(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeCardListBooks(
        path: String,
        workspace: String,
        principal: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCardPutEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        entry: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeCardGetEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeCardDeleteEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeCardListEntries(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCardSearch(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        text: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeCardEntryVcard(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String?


    external fun nativeCardPutVcard(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        vcf: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String


    external fun nativeMailCreateMailbox(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        displayName: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeMailDeleteMailbox(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeMailListMailboxes(
        path: String,
        workspace: String,
        principal: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeMailIngestMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        raw: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String


    external fun nativeMailGetMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeMailToEml(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeMailDeleteMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeMailListMessages(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeMailGetFlags(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeMailSetFlags(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        flags: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeMailSearch(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        text: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeGraphUpsertNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        props: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeGraphGetNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeGraphRemoveNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        cascade: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeGraphUpsertEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeGraphGetEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeGraphRemoveEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeGraphNeighbors(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeGraphOutEdges(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeGraphInEdges(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeGraphReachable(
        path: String,
        workspace: String,
        name: String,
        start: String,
        maxDepth: Long,
        viaLabel: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeGraphShortestPath(
        path: String,
        workspace: String,
        name: String,
        from: String,
        to: String,
        viaLabel: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeVectorCreate(
        path: String,
        workspace: String,
        name: String,
        dim: Long,
        metric: Int,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeVectorUpsert(
        path: String,
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeVectorUpsertSource(
        path: String,
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        sourceText: ByteArray,
        modelId: String?,
        weightsDigest: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeVectorGet(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeVectorSourceText(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeVectorEmbeddingModel(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeVectorIds(
        path: String,
        workspace: String,
        name: String,
        prefix: String?,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeVectorMetadataIndexKeys(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeVectorCreateMetadataIndex(
        path: String,
        workspace: String,
        name: String,
        key: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeVectorDropMetadataIndex(
        path: String,
        workspace: String,
        name: String,
        key: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeVectorDelete(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeVectorSearch(
        path: String,
        workspace: String,
        name: String,
        query: ByteArray,
        k: Long,
        filter: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeVectorSearchPolicy(
        path: String,
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
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeColumnarCreate(
        path: String,
        workspace: String,
        name: String,
        columns: ByteArray,
        targetSegmentRows: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeColumnarAppend(
        path: String,
        workspace: String,
        name: String,
        row: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeColumnarScan(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeColumnarColumns(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeColumnarRows(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Long

    external fun nativeColumnarCompact(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeColumnarInspect(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeColumnarSourceDigest(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeColumnarSelect(
        path: String,
        workspace: String,
        name: String,
        columns: ByteArray,
        filter: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeColumnarAggregate(
        path: String,
        workspace: String,
        name: String,
        aggregates: ByteArray,
        filter: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeDataframeCreate(
        path: String,
        workspace: String,
        name: String,
        plan: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )

    external fun nativeDataframeCollect(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeDataframePreview(
        path: String,
        workspace: String,
        name: String,
        rows: Long,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray

    external fun nativeDataframeMaterialize(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String?

    external fun nativeDataframePlanDigest(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): String

    external fun nativeDataframeSourceDigests(
        path: String,
        workspace: String,
        name: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeSearchCreate(
        path: String,
        workspace: String,
        name: String,
        mapping: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeSearchIndex(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        doc: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeSearchGet(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray?


    external fun nativeSearchDelete(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Boolean


    external fun nativeSearchIds(
        path: String,
        workspace: String,
        name: String,
        prefix: ByteArray,
        hasPrefix: Boolean,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray


    external fun nativeSearchRemap(
        path: String,
        workspace: String,
        name: String,
        mapping: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    )


    external fun nativeSearchQuery(
        path: String,
        workspace: String,
        name: String,
        request: ByteArray,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): ByteArray
}

actual class LoomSql {
    private var handle: Long

    actual constructor(path: String, workspace: String, db: String) {
        handle = nativeOpen(path, workspace, db)
    }

    actual constructor(path: String, workspace: String, db: String, passphrase: String) {
        handle = nativeOpenKeyed(path, workspace, db, passphrase.encodeToByteArray())
    }

    actual constructor(path: String, workspace: String, db: String, kek: ByteArray) {
        handle = nativeOpenWithKek(path, workspace, db, kek)
    }

    actual constructor(
        path: String,
        workspace: String,
        db: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) {
        handle = nativeOpenAuthenticated(
            path,
            workspace,
            db,
            passphrase?.encodeToByteArray(),
            kek,
            authPrincipal,
            authPassphrase?.encodeToByteArray(),
        )
    }

    actual fun exec(sql: String): LoomResult = LoomResult(nativeResultOpen(handle, sql))

    actual fun execJson(sql: String): String = nativeExec(handle, sql)

    actual fun execBytes(sql: String): ByteArray = nativeExecBytes(handle, sql)

    actual fun query(sql: String): LoomRowStream = LoomRowStream(nativeQueryOpen(handle, sql))

    actual fun commit(message: String, author: String): String =
        nativeCommit(handle, message, author)

    actual fun close() {
        if (handle != 0L) {
            nativeClose(handle)
            handle = 0L
        }
    }

    private external fun nativeOpen(path: String, workspace: String, db: String): Long
    private external fun nativeOpenKeyed(
        path: String,
        workspace: String,
        db: String,
        passphrase: ByteArray,
    ): Long

    private external fun nativeOpenWithKek(
        path: String,
        workspace: String,
        db: String,
        kek: ByteArray,
    ): Long

    private external fun nativeOpenAuthenticated(
        path: String,
        workspace: String,
        db: String,
        passphrase: ByteArray?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: ByteArray?,
    ): Long

    private external fun nativeExec(handle: Long, sql: String): String
    private external fun nativeResultOpen(handle: Long, sql: String): Long
    private external fun nativeExecBytes(handle: Long, sql: String): ByteArray
    private external fun nativeQueryOpen(handle: Long, sql: String): Long
    private external fun nativeCommit(handle: Long, message: String, author: String): String
    private external fun nativeClose(handle: Long)

    private companion object {
        init {
            System.loadLibrary("uldren_loom_jni")
        }
    }
}

actual class LoomSqlBatch {
    private var handle: Long

    actual constructor(path: String, workspace: String, db: String) {
        handle = nativeBegin(path, workspace, db)
    }

    actual constructor(path: String, workspace: String, db: String, passphrase: String) {
        handle = nativeBeginKeyed(path, workspace, db, passphrase.encodeToByteArray())
    }

    actual constructor(path: String, workspace: String, db: String, kek: ByteArray) {
        handle = nativeBeginWithKek(path, workspace, db, kek)
    }

    actual fun exec(sql: String): LoomResult = LoomResult(nativeResultOpen(handle, sql))

    actual fun execBytes(sql: String): ByteArray = nativeExecBytes(handle, sql)

    actual fun commit() = nativeCommit(handle)

    actual fun commitVcs(message: String, author: String): String =
        nativeCommitVcs(handle, message, author)

    actual fun abort() = nativeAbort(handle)

    actual fun close() {
        if (handle != 0L) {
            nativeClose(handle)
            handle = 0L
        }
    }

    private external fun nativeBegin(path: String, workspace: String, db: String): Long
    private external fun nativeBeginKeyed(
        path: String,
        workspace: String,
        db: String,
        passphrase: ByteArray,
    ): Long

    private external fun nativeBeginWithKek(
        path: String,
        workspace: String,
        db: String,
        kek: ByteArray,
    ): Long

    private external fun nativeResultOpen(handle: Long, sql: String): Long
    private external fun nativeExecBytes(handle: Long, sql: String): ByteArray
    private external fun nativeCommit(handle: Long)
    private external fun nativeCommitVcs(handle: Long, message: String, author: String): String
    private external fun nativeAbort(handle: Long)
    private external fun nativeClose(handle: Long)

    private companion object {
        init {
            System.loadLibrary("uldren_loom_jni")
        }
    }
}

actual class LoomRowStream actual constructor(handle: Long) {
    private var iter: Long = handle

    actual fun next(): LoomResult? {
        val view = nativeIterNextRow(iter)
        return if (view == 0L) null else LoomResult(view)
    }

    actual fun close() {
        if (iter != 0L) {
            nativeIterFree(iter)
            iter = 0L
        }
    }

    private external fun nativeIterNextRow(iter: Long): Long
    private external fun nativeIterFree(iter: Long)

    private companion object {
        init {
            System.loadLibrary("uldren_loom_jni")
        }
    }
}

actual class LoomResult actual constructor(handle: Long) {
    private var view: Long = handle

    actual fun len(): Long = nativeResultLen(view)
    actual fun itemKind(item: Long): Int = nativeResultItemKind(view, item)
    actual fun columnCount(item: Long): Long = nativeResultColumnCount(view, item)
    actual fun columnName(item: Long, col: Long): String =
        nativeResultColumnName(view, item, col).decodeToString()

    actual fun columnType(item: Long, col: Long): String =
        nativeResultColumnType(view, item, col).decodeToString()

    actual fun rowCount(item: Long): Long = nativeResultRowCount(view, item)
    actual fun rowLen(item: Long, row: Long): Long = nativeResultRowLen(view, item, row)
    actual fun cell(item: Long, row: Long, col: Long): LoomCell =
        nativeResultCell(view, item, row, col)

    actual fun rowsAffected(item: Long): Long = nativeResultCount(view, item)
    actual fun stringCount(item: Long): Long = nativeResultStringCount(view, item)
    actual fun string(item: Long, index: Long): String =
        nativeResultString(view, item, index).decodeToString()

    actual fun variableKind(item: Long): Int = nativeResultVariableKind(view, item)
    actual fun mapLen(item: Long, row: Long): Long = nativeResultMapLen(view, item, row)
    actual fun mapKey(item: Long, row: Long, idx: Long): String =
        nativeResultMapKey(view, item, row, idx).decodeToString()

    actual fun mapValue(item: Long, row: Long, idx: Long): LoomCell =
        nativeResultMapValue(view, item, row, idx)

    actual fun close() {
        if (view != 0L) {
            nativeResultClose(view)
            view = 0L
        }
    }

    private external fun nativeResultClose(view: Long)
    private external fun nativeResultLen(view: Long): Long
    private external fun nativeResultItemKind(view: Long, item: Long): Int
    private external fun nativeResultColumnCount(view: Long, item: Long): Long
    private external fun nativeResultColumnName(view: Long, item: Long, col: Long): ByteArray
    private external fun nativeResultColumnType(view: Long, item: Long, col: Long): ByteArray
    private external fun nativeResultRowCount(view: Long, item: Long): Long
    private external fun nativeResultRowLen(view: Long, item: Long, row: Long): Long
    private external fun nativeResultCell(view: Long, item: Long, row: Long, col: Long): LoomCell
    private external fun nativeResultCount(view: Long, item: Long): Long
    private external fun nativeResultStringCount(view: Long, item: Long): Long
    private external fun nativeResultString(view: Long, item: Long, index: Long): ByteArray
    private external fun nativeResultVariableKind(view: Long, item: Long): Int
    private external fun nativeResultMapLen(view: Long, item: Long, row: Long): Long
    private external fun nativeResultMapKey(view: Long, item: Long, row: Long, idx: Long): ByteArray
    private external fun nativeResultMapValue(view: Long, item: Long, row: Long, idx: Long): LoomCell

    private companion object {
        init {
            System.loadLibrary("uldren_loom_jni")
        }
    }
}
