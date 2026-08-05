package ai.uldren.loom

actual fun Loom.fsImport(
    path: String,
    workspace: String,
    srcPath: String,
    author: String?,
    message: String?,
    commit: Boolean,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeFsImport(
    path, workspace, srcPath, author, message, commit, dryRun, passphrase?.encodeToByteArray(), kek,
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
    gzipOutputPath: String?,
    commit: Boolean,
    author: String?,
    message: String?,
    dryRun: Boolean,
    passphrase: String?,
    kek: ByteArray?,
    authPrincipal: String?,
    authPassphrase: String?,
): ByteArray = LoomNative.nativeArchiveImport(
    path, workspace, srcPath, kind, gzipOutputPath, commit, author, message, dryRun,
    passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray(),
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
