package ai.uldren.loom

    /**
     * Create (or replace the metadata of) address book [book] under [principal] in [workspace] (UUID or
     * name, created with the `contacts` facet if absent).
     */
expect fun Loom.cardCreateBook(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        displayName: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Delete address book [book] under [principal] and its contacts; returns whether it existed. */
expect fun Loom.cardDeleteBook(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** List the address-book ids under [principal] as Loom Canonical CBOR (array of text strings). */
expect fun Loom.cardListBooks(
        path: String,
        workspace: String,
        principal: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Put the contact [entry] (its `ContactEntry` canonical CBOR) into [book], keyed by its UID. */
expect fun Loom.cardPutEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        entry: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Fetch the contact at [uid] in [book] as its `ContactEntry` canonical CBOR, or null if absent. */
expect fun Loom.cardGetEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Remove the contact at [uid] in [book]; returns whether it was present. */
expect fun Loom.cardDeleteEntry(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** List [book] as Loom Canonical CBOR (array of per-contact `ContactEntry` CBOR byte strings). */
expect fun Loom.cardListEntries(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /**
     * Search [book] by a case-insensitive substring [text] over name, organization, and email; returns
     * Loom Canonical CBOR (array of per-contact `ContactEntry` CBOR byte strings).
     */
expect fun Loom.cardSearch(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        text: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** The on-demand vCard (`.vcf`) projection of the contact at [uid], or null if absent. */
expect fun Loom.cardEntryVcard(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String?


    /** Parse vCard [vcf] and store it as a record in [book]; returns the new ETag ("algo:hex"). */
expect fun Loom.cardPutVcard(
        path: String,
        workspace: String,
        principal: String,
        book: String,
        vcf: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String
