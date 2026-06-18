package ai.uldren.loom

    /** Store [content] in [workspace]'s `cas` facet and return its digest. */
expect fun Loom.casPut(
        path: String,
        workspace: String,
        content: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String


    /** Fetch the blob addressed by [digest] from [workspace], or null if absent. */
expect fun Loom.casGet(
        path: String,
        workspace: String,
        digest: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Whether the blob addressed by [digest] is present in [workspace]. */
expect fun Loom.casHas(
        path: String,
        workspace: String,
        digest: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** Drop the blob addressed by [digest] from [workspace]; returns whether it was present. */
expect fun Loom.casDelete(
        path: String,
        workspace: String,
        digest: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** The digests reachable in [workspace]'s `cas` facet as a JSON array of strings. */
expect fun Loom.casListJson(
        path: String,
        workspace: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String
