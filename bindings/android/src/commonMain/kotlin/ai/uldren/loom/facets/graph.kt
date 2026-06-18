package ai.uldren.loom

    /** Insert or replace node [id] in graph [name] of [workspace]; [props] is a CBOR `text -> bytes` map (empty = none). */
expect fun Loom.graphUpsertNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        props: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Fetch node [id]'s props as CBOR from graph [name] of [workspace], or null if absent. */
expect fun Loom.graphGetNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Remove node [id] from graph [name]; [cascade] true also removes incident edges. */
expect fun Loom.graphRemoveNode(
        path: String,
        workspace: String,
        name: String,
        id: String,
        cascade: Boolean,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Insert or replace edge [id] from [src] to [dst] with [label] and CBOR [props] in graph [name]. */
expect fun Loom.graphUpsertEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Fetch edge [id] as the CBOR array `[src, dst, label, props]`, or null if absent. */
expect fun Loom.graphGetEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Remove edge [id] from graph [name]; returns whether it was present. */
expect fun Loom.graphRemoveEdge(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** Distinct adjacent node ids of [id] as a CBOR array of text. */
expect fun Loom.graphNeighborsCbor(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Out-edges of [id] as a CBOR array of `[edge_id, edge]`. */
expect fun Loom.graphOutEdgesCbor(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** In-edges of [id] as a CBOR array of `[edge_id, edge]`. */
expect fun Loom.graphInEdgesCbor(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Node ids reachable from [start] as a CBOR array of text; [maxDepth] < 0 = no limit, [viaLabel] null = any edge. */
expect fun Loom.graphReachableCbor(
        path: String,
        workspace: String,
        name: String,
        start: String,
        maxDepth: Long,
        viaLabel: String?,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** A shortest path from [from] to [to] as a CBOR array of node-id text, or null if none; [viaLabel] null = any edge. */
expect fun Loom.graphShortestPathCbor(
        path: String,
        workspace: String,
        name: String,
        from: String,
        to: String,
        viaLabel: String?,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?
