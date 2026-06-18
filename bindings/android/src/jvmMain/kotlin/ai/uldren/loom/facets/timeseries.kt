package ai.uldren.loom

actual fun Loom.tsPut(
        path: String,
        workspace: String,
        collection: String,
        ts: Long,
        value: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeTsPut(
        path, workspace, collection, ts, value, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.tsGet(
        path: String,
        workspace: String,
        collection: String,
        ts: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeTsGet(
        path, workspace, collection, ts, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.tsRange(
        path: String,
        workspace: String,
        collection: String,
        from: Long,
        to: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeTsRange(
        path, workspace, collection, from, to, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.tsLatest(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): TsPoint? {
        val outTs = LongArray(1)
        val value = LoomNative.nativeTsLatest(
            path, workspace, collection, outTs, passphrase?.encodeToByteArray(), kek,
            authPrincipal, authPassphrase?.encodeToByteArray(),
        )
            ?: return null
        return TsPoint(outTs[0], value)
    }
