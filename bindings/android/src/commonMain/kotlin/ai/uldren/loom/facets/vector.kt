package ai.uldren.loom

    /** Create vector set [name] of width [dim] and [metric] (1 cosine, 2 L2, 3 dot) in [workspace]. */
expect fun Loom.vectorCreate(
        path: String,
        workspace: String,
        name: String,
        dim: Long,
        metric: Int,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Insert or replace [id] in vector set [name]: [vector] is little-endian f32 bytes, [metadata] a CBOR map (empty = none). */
expect fun Loom.vectorUpsert(
        path: String,
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Insert or replace [id] with UTF-8 [sourceText] and optional embedding model profile. */
expect fun Loom.vectorUpsertSource(
        path: String,
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        sourceText: ByteArray,
        modelId: String? = null,
        weightsDigest: String? = null,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Fetch the vector + metadata at [id] as the CBOR array `[vector_bytes, metadata]`, or null if absent. */
expect fun Loom.vectorGet(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Fetch UTF-8 source text for [id], or null if absent. */
expect fun Loom.vectorSourceText(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Fetch the embedding model profile as CBOR `[1, model_id, dimension, weights_digest]`, or null. */
expect fun Loom.vectorEmbeddingModelCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Vector ids in [name], sorted ascending, as a CBOR array of text. [prefix] restricts by string prefix. */
expect fun Loom.vectorIdsCbor(
        path: String,
        workspace: String,
        name: String,
        prefix: String? = null,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

    /** Declared metadata equality index keys in [name], sorted ascending, as a CBOR array of text. */
expect fun Loom.vectorMetadataIndexKeysCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Declare and build a metadata equality index for [key]; returns whether it was new. */
expect fun Loom.vectorCreateMetadataIndex(
        path: String,
        workspace: String,
        name: String,
        key: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** Drop the metadata equality index for [key]; returns whether an index was present. */
expect fun Loom.vectorDropMetadataIndex(
        path: String,
        workspace: String,
        name: String,
        key: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** Remove [id] from vector set [name]; returns whether it was present. */
expect fun Loom.vectorDelete(
        path: String,
        workspace: String,
        name: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** Top-[k] nearest neighbours of [query] (f32 bytes) passing CBOR [filter] (empty = all) as a CBOR array of `[id, score_cell]`. */
expect fun Loom.vectorSearchCbor(
        path: String,
        workspace: String,
        name: String,
        query: ByteArray,
        k: Long,
        filter: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Top-[k] nearest neighbours with explicit accelerator policy over built-in PQ. Policy 0 exact, 1 approximate-above-threshold. */
expect fun Loom.vectorSearchPolicyCbor(
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
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray
