package ai.uldren.loom

actual fun Loom.dataframeCreate(
        path: String,
        workspace: String,
        name: String,
        plan: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeDataframeCreate(
        path, workspace, name, plan, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )

actual fun Loom.dataframeCollectCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeDataframeCollect(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )

actual fun Loom.dataframePreviewCbor(
        path: String,
        workspace: String,
        name: String,
        rows: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeDataframePreview(
        path, workspace, name, rows, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )

actual fun Loom.dataframeMaterialize(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String? = LoomNative.nativeDataframeMaterialize(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )

actual fun Loom.dataframePlanDigest(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String = LoomNative.nativeDataframePlanDigest(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )

actual fun Loom.dataframeSourceDigestsCbor(
        path: String,
        workspace: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeDataframeSourceDigests(
        path, workspace, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )
