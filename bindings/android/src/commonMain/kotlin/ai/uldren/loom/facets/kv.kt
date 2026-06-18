package ai.uldren.loom

    /** Put [value] at the typed [key] (Loom Canonical CBOR cell) in map [name] of [workspace]. */
expect fun Loom.kvPut(
        path: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        value: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Fetch the value at typed [key] in map [collection] of [workspace], or null if absent. */
expect fun Loom.kvGet(
        path: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Remove the typed [key] from map [collection] of [workspace]; returns whether it was present. */
expect fun Loom.kvDelete(
        path: String,
        workspace: String,
        collection: String,
        key: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** List map [collection] of [workspace] as Loom Canonical CBOR `[key, value]` pairs in key order. */
expect fun Loom.kvList(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Entries of map [collection] with `lo <= key < hi` (half-open, key order) as CBOR `[key, value]` pairs. */
expect fun Loom.kvRange(
        path: String,
        workspace: String,
        collection: String,
        lo: ByteArray,
        hi: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray
