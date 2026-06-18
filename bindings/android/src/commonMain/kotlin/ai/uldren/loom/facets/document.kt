package ai.uldren.loom

expect fun Loom.docPutText(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        text: String,
        expectedEntityTag: String? = null,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): DocumentPutResult


expect fun Loom.docGetText(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): DocumentText?


expect fun Loom.docPutBinary(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        bytes: ByteArray,
        expectedEntityTag: String? = null,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): DocumentPutResult


expect fun Loom.docGetBinary(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): DocumentBinary?


    /** Remove [id] from collection [collection]; returns whether it was present. */
expect fun Loom.docDelete(
        path: String,
        workspace: String,
        collection: String,
        id: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


expect fun Loom.docListBinary(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


expect fun Loom.docIndexCreate(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        fieldPath: String,
        unique: Boolean = false,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


expect fun Loom.docIndexCreateJson(
        path: String,
        workspace: String,
        collection: String,
        declarationJson: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


expect fun Loom.docIndexDrop(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


expect fun Loom.docIndexRebuild(
        path: String,
        workspace: String,
        collection: String,
        name: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


expect fun Loom.docIndexListJson(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String


expect fun Loom.docIndexStatusJson(
        path: String,
        workspace: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String


expect fun Loom.docFindJson(
        path: String,
        workspace: String,
        collection: String,
        index: String,
        valueJson: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String


expect fun Loom.docQueryJson(
        path: String,
        workspace: String,
        collection: String,
        queryJson: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String
