package ai.uldren.loom

actual fun Loom.fsImport(
    path: String,
    workspace: String,
    srcPath: String,
    commit: Boolean,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeFsImport(
    path, workspace, srcPath, commit, dryRun, passphrase?.encodeToByteArray(), kek,
    authPrincipal, authPassphrase?.encodeToByteArray(),
)

actual fun Loom.fsExport(
    path: String,
    workspace: String,
    dstPath: String,
    revision: String?,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeFsExport(
    path, workspace, dstPath, revision, dryRun, passphrase?.encodeToByteArray(), kek,
    authPrincipal, authPassphrase?.encodeToByteArray(),
)

actual fun Loom.archiveImport(
    path: String,
    workspace: String,
    srcPath: String,
    kind: String,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeArchiveImport(
    path, workspace, srcPath, kind, dryRun, passphrase?.encodeToByteArray(), kek,
    authPrincipal, authPassphrase?.encodeToByteArray(),
)

actual fun Loom.archiveExport(
    path: String,
    workspace: String,
    dstPath: String,
    kind: String,
    revision: String?,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeArchiveExport(
    path, workspace, dstPath, kind, revision, dryRun, passphrase?.encodeToByteArray(), kek,
    authPrincipal, authPassphrase?.encodeToByteArray(),
)

actual fun Loom.carImport(
    path: String,
    srcPath: String,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeCarImport(
    path, srcPath, dryRun, passphrase?.encodeToByteArray(), kek,
    authPrincipal, authPassphrase?.encodeToByteArray(),
)

actual fun Loom.carExport(
    path: String,
    workspace: String,
    dstPath: String,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeCarExport(
    path, workspace, dstPath, dryRun, passphrase?.encodeToByteArray(), kek,
    authPrincipal, authPassphrase?.encodeToByteArray(),
)
