package ai.uldren.loom

actual fun Loom.columnarCreate(
        path: String,
        workspace: String,
        name: String,
        columns: ByteArray,
        targetSegmentRows: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeColumnarCreate(
        path, workspace, name, columns, targetSegmentRows, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.columnarAppend(
        path: String,
        workspace: String,
        name: String,
        row: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeColumnarAppend(
        path, workspace, name, row, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.columnarScanCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeColumnarScan(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.columnarColumnsCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeColumnarColumns(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.columnarRows(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Long = LoomNative.nativeColumnarRows(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )

actual fun Loom.columnarCompact(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeColumnarCompact(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.columnarInspectCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeColumnarInspect(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.columnarSourceDigestCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeColumnarSourceDigest(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.columnarSelectCbor(
        path: String,
        workspace: String,
        name: String,
        columns: ByteArray,
        filter: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeColumnarSelect(
        path, workspace, name, columns, filter, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.columnarAggregateCbor(
        path: String,
        workspace: String,
        name: String,
        aggregates: ByteArray,
        filter: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeColumnarAggregate(
        path, workspace, name, aggregates, filter, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )
