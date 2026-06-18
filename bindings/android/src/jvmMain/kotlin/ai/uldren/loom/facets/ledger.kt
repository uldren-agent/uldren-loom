package ai.uldren.loom

actual fun Loom.ledgerAppend(
        path: String,
        workspace: String,
        collection: String,
        payload: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Long = LoomNative.nativeLedgerAppend(
        path, workspace, collection, payload, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.ledgerGet(
        path: String,
        workspace: String,
        collection: String,
        seq: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeLedgerGet(
        path, workspace, collection, seq, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.ledgerHead(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String? = LoomNative.nativeLedgerHead(
        path, workspace, collection, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.ledgerLen(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Long = LoomNative.nativeLedgerLen(
        path, workspace, collection, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.ledgerVerify(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeLedgerVerify(
        path, workspace, collection, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )
