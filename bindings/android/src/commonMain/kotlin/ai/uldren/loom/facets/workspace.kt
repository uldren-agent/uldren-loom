package ai.uldren.loom

    /** Create a workspace and return its UUID string. */
expect fun Loom.workspaceCreate(
        path: String,
        name: String? = null,
        facet: String? = null,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String


    /** List workspaces as JSON records with id, name, facets, and head. */
expect fun Loom.workspaceListJson(
        path: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String


    /** Rename a workspace selected by UUID or current name. */
expect fun Loom.workspaceRename(
        path: String,
        workspace: String,
        newName: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Delete a workspace selected by UUID or name. */
expect fun Loom.workspaceDelete(
        path: String,
        workspace: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )
