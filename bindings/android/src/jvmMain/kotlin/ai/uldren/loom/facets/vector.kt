package ai.uldren.loom

actual fun Loom.vectorCreate(
        path: String,
        workspace: String,
        name: String,
        dim: Long,
        metric: Int,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeVectorCreate(
        path, workspace, name, dim, metric, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorUpsert(
        path: String,
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeVectorUpsert(
        path, workspace, name, id, vector, metadata, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorUpsertSource(
        path: String,
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        sourceText: ByteArray,
        modelId: String?,
        weightsDigest: String?,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeVectorUpsertSource(
        path, workspace, name, id, vector, metadata, sourceText, modelId, weightsDigest,
        passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorGet(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeVectorGet(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorSourceText(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeVectorSourceText(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorEmbeddingModelCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeVectorEmbeddingModel(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorIdsCbor(
        path: String,
        workspace: String,
        name: String,
        prefix: String?,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeVectorIds(
        path, workspace, name, prefix, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorMetadataIndexKeysCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeVectorMetadataIndexKeys(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorCreateMetadataIndex(
        path: String,
        workspace: String,
        name: String,
        key: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeVectorCreateMetadataIndex(
        path, workspace, name, key, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorDropMetadataIndex(
        path: String,
        workspace: String,
        name: String,
        key: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeVectorDropMetadataIndex(
        path, workspace, name, key, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorDelete(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeVectorDelete(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorSearchCbor(
        path: String,
        workspace: String,
        name: String,
        query: ByteArray,
        k: Long,
        filter: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeVectorSearch(
        path, workspace, name, query, k, filter, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vectorSearchPolicyCbor(
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
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeVectorSearchPolicy(
        path, workspace, name, query, k, filter, policy, threshold, ef, pqM, pqK, pqIters,
        passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray(),
    )
