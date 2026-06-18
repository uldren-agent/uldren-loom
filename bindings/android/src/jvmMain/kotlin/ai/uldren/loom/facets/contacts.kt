package ai.uldren.loom

actual fun Loom.cardCreateBook(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        displayName: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeCardCreateBook(path, workspace, principal, book, displayName, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardDeleteBook(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeCardDeleteBook(path, workspace, principal, book, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardListBooks(
        path: String,
        workspace: String,
        principal: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeCardListBooks(path, workspace, principal, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardPutEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        entry: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeCardPutEntry(path, workspace, principal, book, entry, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardGetEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeCardGetEntry(path, workspace, principal, book, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardDeleteEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeCardDeleteEntry(path, workspace, principal, book, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardListEntries(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeCardListEntries(path, workspace, principal, book, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardSearch(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        text: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeCardSearch(path, workspace, principal, book, text, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardEntryVcard(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String? = LoomNative.nativeCardEntryVcard(path, workspace, principal, book, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.cardPutVcard(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        vcf: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String = LoomNative.nativeCardPutVcard(path, workspace, principal, book, vcf, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())
