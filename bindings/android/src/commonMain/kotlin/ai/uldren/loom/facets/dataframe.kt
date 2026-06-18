package ai.uldren.loom

/** Create dataframe frame [name] from canonical DataframePlan CBOR [plan]. */
expect fun Loom.dataframeCreate(
        path: String,
        workspace: String,
        name: String,
        plan: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )

/** Execute dataframe frame [name] and return canonical CBOR `[columns, rows]`. */
expect fun Loom.dataframeCollectCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

/** Execute dataframe frame [name] and return at most [rows] rows as canonical CBOR `[columns, rows]`. */
expect fun Loom.dataframePreviewCbor(
        path: String,
        workspace: String,
        name: String,
        rows: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

/** Materialize dataframe frame [name]; returns a CAS digest when the materialization target emits one. */
expect fun Loom.dataframeMaterialize(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String?

/** Canonical dataframe plan digest as `algo:hex`. */
expect fun Loom.dataframePlanDigest(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String

/** Source digests pinned in the dataframe plan as canonical CBOR array of `algo:hex` strings. */
expect fun Loom.dataframeSourceDigestsCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray
