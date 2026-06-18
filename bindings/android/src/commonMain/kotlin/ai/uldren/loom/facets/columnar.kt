package ai.uldren.loom

    /** Create columnar dataset [name] with [columns] (CBOR array of `[name, type_tag]`) and [targetSegmentRows] (0 = default). */
expect fun Loom.columnarCreate(
        path: String,
        workspace: String,
        name: String,
        columns: ByteArray,
        targetSegmentRows: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Append [row] (a CBOR cell array) to columnar dataset [name] of [workspace]. */
expect fun Loom.columnarAppend(
        path: String,
        workspace: String,
        name: String,
        row: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** All rows of columnar dataset [name] in append order as a CBOR array of cell arrays. */
expect fun Loom.columnarScanCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** The columns of dataset [name] as a CBOR array of `[name, type_tag]`. */
expect fun Loom.columnarColumnsCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** The total row count of columnar dataset [name] of [workspace]. */
expect fun Loom.columnarRows(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Long

    /** Compact columnar dataset [name] at its target segment size. */
expect fun Loom.columnarCompact(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Inspect columnar dataset [name] as CBOR `[columns, rows, segment_count, target_segment_rows, source_digest]`. */
expect fun Loom.columnarInspectCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Source digest for derived columnar projections as CBOR text. */
expect fun Loom.columnarSourceDigestCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Project [columns] (CBOR array of text) from rows matching CBOR [filter] (empty = all) as a CBOR array of cell arrays. */
expect fun Loom.columnarSelectCbor(
        path: String,
        workspace: String,
        name: String,
        columns: ByteArray,
        filter: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Evaluate aggregate expressions from CBOR `[[op, column?] ...]`, with optional select [filter]. */
expect fun Loom.columnarAggregateCbor(
        path: String,
        workspace: String,
        name: String,
        aggregates: ByteArray,
        filter: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray
