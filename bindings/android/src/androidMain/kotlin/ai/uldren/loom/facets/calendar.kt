package ai.uldren.loom

actual fun Loom.calCreateCollection(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        displayName: String,
        components: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeCalCreateCollection(
        path, workspace, principal, collection, displayName, components,
        passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.calDeleteCollection(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean =
        LoomNative.nativeCalDeleteCollection(path, workspace, principal, collection, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calListCollections(
        path: String,
        workspace: String,
        principal: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeCalListCollections(path, workspace, principal, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calPutEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        entry: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeCalPutEntry(path, workspace, principal, collection, entry, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calGetEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? =
        LoomNative.nativeCalGetEntry(path, workspace, principal, collection, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calDeleteEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean =
        LoomNative.nativeCalDeleteEntry(path, workspace, principal, collection, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calListEntries(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeCalListEntries(path, workspace, principal, collection, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calRange(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        from: String,
        to: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeCalRange(path, workspace, principal, collection, from, to, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calSearch(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        component: String,
        text: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeCalSearch(path, workspace, principal, collection, component, text, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calEntryIcs(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String? =
        LoomNative.nativeCalEntryIcs(path, workspace, principal, collection, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.calPutIcs(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        ics: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String =
        LoomNative.nativeCalPutIcs(path, workspace, principal, collection, ics, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())
