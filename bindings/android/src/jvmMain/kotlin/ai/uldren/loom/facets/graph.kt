package ai.uldren.loom

actual fun Loom.graphUpsertNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        props: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeGraphUpsertNode(
        path, workspace, name, id, props, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphGetNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeGraphGetNode(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphRemoveNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        cascade: Boolean,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeGraphRemoveNode(
        path, workspace, name, id, cascade, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphUpsertEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeGraphUpsertEdge(
        path, workspace, name, id, src, dst, label, props, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphGetEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeGraphGetEdge(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphRemoveEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeGraphRemoveEdge(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphNeighborsCbor(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeGraphNeighbors(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphOutEdgesCbor(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeGraphOutEdges(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphInEdgesCbor(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeGraphInEdges(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphReachableCbor(
        path: String,
        workspace: String,
        name: String,
        start: String,
        maxDepth: Long,
        viaLabel: String?,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeGraphReachable(
        path, workspace, name, start, maxDepth, viaLabel, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.graphShortestPathCbor(
        path: String,
        workspace: String,
        name: String,
        from: String,
        to: String,
        viaLabel: String?,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeGraphShortestPath(
        path, workspace, name, from, to, viaLabel, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )
