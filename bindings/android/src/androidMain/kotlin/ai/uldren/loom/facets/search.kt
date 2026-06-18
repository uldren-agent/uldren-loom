package ai.uldren.loom

actual fun Loom.searchCreate(
        path: String,
        workspace: String,
        name: String,
        mapping: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeSearchCreate(
        path, workspace, name, mapping, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.searchIndex(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        doc: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeSearchIndex(
        path, workspace, name, id, doc, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.searchGet(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeSearchGet(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.searchDelete(
        path: String,
        workspace: String,
        name: String,
        id: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeSearchDelete(
        path, workspace, name, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.searchIdsCbor(
        path: String,
        workspace: String,
        name: String,
        prefix: ByteArray?,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeSearchIds(
        path, workspace, name, prefix ?: ByteArray(0), prefix != null, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.searchRemap(
        path: String,
        workspace: String,
        name: String,
        mapping: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeSearchRemap(
        path, workspace, name, mapping, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.searchQueryCbor(
        path: String,
        workspace: String,
        name: String,
        request: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeSearchQuery(
        path, workspace, name, request, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )
