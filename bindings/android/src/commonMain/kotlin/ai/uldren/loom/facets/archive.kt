package ai.uldren.loom

expect fun Loom.fsImport(
    path: String,
    workspace: String,
    srcPath: String,
    commit: Boolean = false,
    dryRun: Boolean = false,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

expect fun Loom.fsExport(
    path: String,
    workspace: String,
    dstPath: String,
    revision: String? = null,
    dryRun: Boolean = false,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

expect fun Loom.archiveImport(
    path: String,
    workspace: String,
    srcPath: String,
    kind: String,
    dryRun: Boolean = false,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

expect fun Loom.archiveExport(
    path: String,
    workspace: String,
    dstPath: String,
    kind: String,
    revision: String? = null,
    dryRun: Boolean = false,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

expect fun Loom.carImport(
    path: String,
    srcPath: String,
    dryRun: Boolean = false,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

expect fun Loom.carExport(
    path: String,
    workspace: String,
    dstPath: String,
    dryRun: Boolean = false,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray
