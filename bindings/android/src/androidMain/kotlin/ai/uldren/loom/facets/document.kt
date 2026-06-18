package ai.uldren.loom

actual fun Loom.docPutText(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        text: String,
        expectedEntityTag: String?,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): DocumentPutResult = LoomNative.nativeDocPutText(
        path, workspace, collection, id, text, expectedEntityTag, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    ).let { DocumentPutResult(it[0] as String, it[1] as String) }


actual fun Loom.docGetText(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): DocumentText? = LoomNative.nativeDocGetText(
        path, workspace, collection, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )?.let { DocumentText(it[0] as String, it[1] as String, it[2] as String) }


actual fun Loom.docPutBinary(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        bytes: ByteArray,
        expectedEntityTag: String?,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): DocumentPutResult = LoomNative.nativeDocPutBinary(
        path, workspace, collection, id, bytes, expectedEntityTag, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    ).let { DocumentPutResult(it[0] as String, it[1] as String) }


actual fun Loom.docGetBinary(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): DocumentBinary? = LoomNative.nativeDocGetBinary(
        path, workspace, collection, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )?.let { DocumentBinary(it[0] as ByteArray, it[1] as String, it[2] as String) }


actual fun Loom.docDelete(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeDocDelete(
        path, workspace, collection, id, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docListBinary(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeDocListBinary(
        path, workspace, collection, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docIndexCreate(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        fieldPath: String,
        unique: Boolean,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeDocIndexCreate(
        path, workspace, collection, name, fieldPath, unique, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docIndexCreateJson(
        path: String,
        workspace: String,
        collection: String,
        declarationJson: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeDocIndexCreateJson(
        path, workspace, collection, declarationJson, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docIndexDrop(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeDocIndexDrop(
        path, workspace, collection, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docIndexRebuild(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeDocIndexRebuild(
        path, workspace, collection, name, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docIndexListJson(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String = LoomNative.nativeDocIndexListJson(
        path, workspace, collection, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docIndexStatusJson(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String = LoomNative.nativeDocIndexStatusJson(
        path, workspace, collection, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docFindJson(
        path: String,
        workspace: String,
        collection: String,
        index: String,
        valueJson: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String = LoomNative.nativeDocFindJson(
        path, workspace, collection, index, valueJson, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.docQueryJson(
        path: String,
        workspace: String,
        collection: String,
        queryJson: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String = LoomNative.nativeDocQueryJson(
        path, workspace, collection, queryJson, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )
