package ai.uldren.loom

actual fun Loom.vcsBlameCbor(
        path: String,
        workspace: String,
        branch: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeVcsBlame(
        path, workspace, branch, passphrase?.encodeToByteArray(), kek,
        authPrincipal, authPassphrase?.encodeToByteArray(),
    )


actual fun Loom.vcsDiffCbor(
        path: String,
        workspace: String,
        fromCommit: String,
        toCommit: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeVcsDiff(
            path, workspace, fromCommit, toCommit, passphrase?.encodeToByteArray(), kek,
            authPrincipal, authPassphrase?.encodeToByteArray(),
        )

actual fun Loom.watchSubscribe(
        path: String,
        workspace: String,
        branch: String,
        facet: String?,
        pathPrefix: String?,
        changeKinds: List<String>,
        fromCommit: String?,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String =
        LoomNative.nativeWatchSubscribe(
            path, workspace, branch, facet, pathPrefix, changeKinds.joinToString(","),
            fromCommit, passphrase?.encodeToByteArray(), kek,
            authPrincipal, authPassphrase?.encodeToByteArray(),
        )

actual fun Loom.watchPollCbor(
        path: String,
        cursor: String,
        max: UInt,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeWatchPoll(
            path, cursor, max.toInt(), passphrase?.encodeToByteArray(), kek,
            authPrincipal, authPassphrase?.encodeToByteArray(),
        )
