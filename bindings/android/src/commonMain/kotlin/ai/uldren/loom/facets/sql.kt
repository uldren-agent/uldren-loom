package ai.uldren.loom

expect fun Loom.sqlReadTableCbor(
        path: String,
        workspace: String,
        table: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

expect fun Loom.sqlReadTableAtCbor(
        path: String,
        workspace: String,
        table: String,
        commit: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

expect fun Loom.sqlIndexScanCbor(
        path: String,
        workspace: String,
        table: String,
        index: String,
        prefix: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

expect fun Loom.sqlIndexScanAtCbor(
        path: String,
        workspace: String,
        table: String,
        index: String,
        prefix: ByteArray,
        commit: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

expect fun Loom.sqlBlameCbor(
        path: String,
        workspace: String,
        branch: String,
        table: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


expect fun Loom.sqlDiffCbor(
        path: String,
        workspace: String,
        table: String,
        fromCommit: String,
        toCommit: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

expect fun Loom.sqlTableDiffCbor(
        path: String,
        workspace: String,
        table: String,
        fromCommit: String,
        toCommit: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray
