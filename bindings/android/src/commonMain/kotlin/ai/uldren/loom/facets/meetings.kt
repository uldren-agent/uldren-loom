package ai.uldren.loom

expect fun Loom.meetingsImportSnapshot(
    path: String,
    workspace: String,
    inputProfile: String,
    snapshot: ByteArray,
    dryRun: Boolean = false,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.meetingsSourceRead(
    path: String,
    workspace: String,
    sourceId: String,
    leaf: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray
