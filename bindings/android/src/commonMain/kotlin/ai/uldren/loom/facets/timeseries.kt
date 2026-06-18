package ai.uldren.loom

    /** Record [value] at timestamp [ts] in series [collection] of [workspace]. */
expect fun Loom.tsPut(
        path: String,
        workspace: String,
        collection: String,
        ts: Long,
        value: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Fetch the point at timestamp [ts] in series [collection], or null if absent. */
expect fun Loom.tsGet(
        path: String,
        workspace: String,
        collection: String,
        ts: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Points of series [collection] with `from <= ts < to` (half-open) as CBOR `[ts, value]` pairs. */
expect fun Loom.tsRange(
        path: String,
        workspace: String,
        collection: String,
        from: Long,
        to: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** The most recent point of series [collection] as a [TsPoint], or null if absent/empty. */
expect fun Loom.tsLatest(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): TsPoint?
