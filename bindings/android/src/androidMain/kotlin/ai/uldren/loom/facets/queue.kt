package ai.uldren.loom

actual fun Loom.queueAppend(
        path: String,
        workspace: String,
        stream: String,
        entry: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Long = LoomNative.nativeQueueAppend(
        path, workspace, stream, entry, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.queueGet(
        path: String,
        workspace: String,
        stream: String,
        seq: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeQueueGet(
        path, workspace, stream, seq, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.queueRangeCbor(
        path: String,
        workspace: String,
        stream: String,
        lo: Long,
        hi: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeQueueRange(
        path, workspace, stream, lo, hi, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.queueLen(
        path: String,
        workspace: String,
        stream: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Long = LoomNative.nativeQueueLen(
        path, workspace, stream, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.queueConsumerPosition(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Long =
        LoomNative.nativeQueueConsumerPosition(
            path, workspace, stream, consumerId, passphrase?.encodeToByteArray(), kek,
            authPrincipal, authPassphrase?.encodeToByteArray(),
        )


actual fun Loom.queueConsumerReadCbor(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        max: Int,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeQueueConsumerRead(
            path, workspace, stream, consumerId, max, passphrase?.encodeToByteArray(), kek,
            authPrincipal, authPassphrase?.encodeToByteArray(),
        )


actual fun Loom.queueConsumerAdvance(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        nextSeq: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) =
        LoomNative.nativeQueueConsumerAdvance(
            path, workspace, stream, consumerId, nextSeq, passphrase?.encodeToByteArray(), kek,
            authPrincipal, authPassphrase?.encodeToByteArray(),
        )


actual fun Loom.queueConsumerReset(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        nextSeq: Long,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) =
        LoomNative.nativeQueueConsumerReset(
            path, workspace, stream, consumerId, nextSeq, passphrase?.encodeToByteArray(), kek,
            authPrincipal, authPassphrase?.encodeToByteArray(),
        )
