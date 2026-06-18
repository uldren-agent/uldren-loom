package ai.uldren.loom

actual fun Loom.meetingsImportSnapshot(
    path: String,
    workspace: String,
    inputProfile: String,
    snapshot: ByteArray,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): String = LoomNative.nativeMeetingsImportSnapshot(
    path, workspace, inputProfile, snapshot, dryRun, passphrase?.encodeToByteArray(), kek,
    authPrincipal, authPassphrase?.encodeToByteArray(),
)

actual fun Loom.meetingsSourceRead(
    path: String,
    workspace: String,
    sourceId: String,
    leaf: String,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeMeetingsSourceRead(
    path, workspace, sourceId, leaf, passphrase?.encodeToByteArray(), kek,
    authPrincipal, authPassphrase?.encodeToByteArray(),
)
