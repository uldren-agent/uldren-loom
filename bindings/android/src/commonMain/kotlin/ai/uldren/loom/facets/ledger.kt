package ai.uldren.loom

    /** Append [payload] to ledger [collection] of [workspace]; returns the new entry's zero-based sequence. */
expect fun Loom.ledgerAppend(
        path: String,
        workspace: String,
        collection: String,
        payload: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Long


    /** Fetch the payload at [seq] in ledger [collection], or null if absent. */
expect fun Loom.ledgerGet(
        path: String,
        workspace: String,
        collection: String,
        seq: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** The head chain hash of ledger [collection] as `"algo:hex"`, or null when absent or empty. */
expect fun Loom.ledgerHead(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String?


    /** The number of entries in ledger [collection] (0 when absent). */
expect fun Loom.ledgerLen(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Long


    /** Recompute ledger [collection]'s chain and confirm every stored hash matches; throws if broken. */
expect fun Loom.ledgerVerify(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )
