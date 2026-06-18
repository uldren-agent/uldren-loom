package ai.uldren.loom

    /** Workspace/entry-level blame for [branch] (which commit last set each path) as raw Loom Canonical CBOR. */
expect fun Loom.vcsBlameCbor(
        path: String,
        workspace: String,
        branch: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

    /** Structural diff between two commit addresses as raw LMDIFF Loom Canonical CBOR. */
expect fun Loom.vcsDiffCbor(
        path: String,
        workspace: String,
        fromCommit: String,
        toCommit: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray

    /** Subscribe to workspace history changes and return an opaque watch cursor string. */
expect fun Loom.watchSubscribe(
        path: String,
        workspace: String,
        branch: String,
        facet: String? = null,
        pathPrefix: String? = null,
        changeKinds: List<String> = emptyList(),
        fromCommit: String? = null,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String

    /** Poll an opaque watch cursor and return a canonical-CBOR `loom.watch.batch.v1` batch. */
expect fun Loom.watchPollCbor(
        path: String,
        cursor: String,
        max: UInt,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray
