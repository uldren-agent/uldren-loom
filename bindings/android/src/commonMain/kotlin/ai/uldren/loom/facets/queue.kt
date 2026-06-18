package ai.uldren.loom

    /**
     * Append [entry] to [stream] in [workspace] (UUID or name, created with the queue facet if absent);
     * returns the assigned zero-based sequence.
     */
expect fun Loom.queueAppend(
        path: String,
        workspace: String,
        stream: String,
        entry: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Long


    /** Fetch the entry at [seq] in [stream], or null if out of range. */
expect fun Loom.queueGet(
        path: String,
        workspace: String,
        stream: String,
        seq: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** The half-open range `[lo, hi)` of [stream] as raw Loom Canonical CBOR (an array of byte strings). */
expect fun Loom.queueRangeCbor(
        path: String,
        workspace: String,
        stream: String,
        lo: Long,
        hi: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** The number of entries in [stream]. */
expect fun Loom.queueLen(
        path: String,
        workspace: String,
        stream: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Long


    /** The named consumer's next sequence for [stream]; 0 when none is stored. */
expect fun Loom.queueConsumerPosition(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Long


    /**
     * Up to [max] entries from the consumer's stored next sequence as raw Loom Canonical CBOR; does not
     * advance the consumer.
     */
expect fun Loom.queueConsumerReadCbor(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        max: Int,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Advance the named consumer's next sequence for [stream] to [nextSeq] (monotonic). */
expect fun Loom.queueConsumerAdvance(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        nextSeq: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Set the named consumer's next sequence for [stream] to [nextSeq] (may move backward). */
expect fun Loom.queueConsumerReset(
        path: String,
        workspace: String,
        stream: String,
        consumerId: String,
        nextSeq: Long,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )
