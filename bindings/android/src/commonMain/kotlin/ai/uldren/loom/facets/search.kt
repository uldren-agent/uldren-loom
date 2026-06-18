package ai.uldren.loom

    /** Create search collection [name] in [workspace] with [mapping] (CBOR map field -> `[type_tag, stored, faceted]`). */
expect fun Loom.searchCreate(
        path: String,
        workspace: String,
        name: String,
        mapping: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Insert or replace the document at [id] (opaque bytes); [doc] is a CBOR `field -> value` map. */
expect fun Loom.searchIndex(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        doc: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Fetch the document at [id] as a CBOR `field -> value` map, or null if absent. */
expect fun Loom.searchGet(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Remove [id] from search collection [name]; returns whether it was present. */
expect fun Loom.searchDelete(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** Document ids as a CBOR array of byte strings; [prefix] non-null restricts to ids under that prefix. */
expect fun Loom.searchIdsCbor(
        path: String,
        workspace: String,
        name: String,
        prefix: ByteArray?,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Replace the field [mapping] (CBOR) of search collection [name] of [workspace]. */
expect fun Loom.searchRemap(
        path: String,
        workspace: String,
        name: String,
        mapping: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Run the linear-scan query [request] (CBOR `[query, limit, offset]`) against [name] -> response CBOR. */
expect fun Loom.searchQueryCbor(
        path: String,
        workspace: String,
        name: String,
        request: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray
