package ai.uldren.loom

actual fun Loom.sqlReadTableCbor(
        path: String,
        workspace: String,
        table: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeSqlReadTable(path, workspace, table, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())

actual fun Loom.sqlReadTableAtCbor(
        path: String,
        workspace: String,
        table: String,
        commit: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeSqlReadTableAt(path, workspace, table, commit, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.sqlIndexScanCbor(
        path: String,
        workspace: String,
        table: String,
        index: String,
        prefix: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeSqlIndexScan(path, workspace, table, index, prefix, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())

actual fun Loom.sqlIndexScanAtCbor(
        path: String,
        workspace: String,
        table: String,
        index: String,
        prefix: ByteArray,
        commit: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeSqlIndexScanAt(path, workspace, table, index, prefix, commit, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.sqlBlameCbor(
        path: String,
        workspace: String,
        branch: String,
        table: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeSqlBlame(path, workspace, branch, table, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.sqlDiffCbor(
        path: String,
        workspace: String,
        table: String,
        fromCommit: String,
        toCommit: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeSqlDiff(path, workspace, table, fromCommit, toCommit, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())

actual fun Loom.sqlTableDiffCbor(
        path: String,
        workspace: String,
        table: String,
        fromCommit: String,
        toCommit: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray =
        LoomNative.nativeSqlTableDiff(path, workspace, table, fromCommit, toCommit, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())
