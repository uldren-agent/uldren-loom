package ai.uldren.loom

    /**
     * Create (or replace the metadata of) mailbox [mailbox] under [principal] in [workspace] (UUID or name,
     * created with the `mail` facet if absent).
     */
expect fun Loom.mailCreateMailbox(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        displayName: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Delete mailbox [mailbox] under [principal] and its message indexes/flags; returns whether it existed. */
expect fun Loom.mailDeleteMailbox(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** List the mailbox ids under [principal] as Loom Canonical CBOR (array of text strings). */
expect fun Loom.mailListMailboxes(
        path: String,
        workspace: String,
        principal: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /**
     * Ingest the raw RFC 5322 message [raw] into [mailbox] under [uid] (CAS the body, index the headers);
     * returns the body's content address ("algo:hex").
     */
expect fun Loom.mailIngestMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        raw: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String


    /** Fetch the structured index of the message at [uid] as its `MailMessage` canonical CBOR, or null. */
expect fun Loom.mailGetMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Fetch the raw RFC 5322 body (`.eml` bytes) of the message at [uid], digest-verified, or null. */
expect fun Loom.mailToEml(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Remove the message index and its flags at [uid] (body stays in the CAS); returns whether it existed. */
expect fun Loom.mailDeleteMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** List [mailbox] as Loom Canonical CBOR (array of per-message `MailMessage` CBOR byte strings). */
expect fun Loom.mailListMessages(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** The flags/labels on the message at [uid] as Loom Canonical CBOR (sorted, deduplicated text strings). */
expect fun Loom.mailGetFlags(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Replace the flags/labels on the message at [uid] with [flags] (a Loom Canonical CBOR `Array(Text)`). */
expect fun Loom.mailSetFlags(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        flags: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /**
     * Search [mailbox] by a case-insensitive substring [text] over subject and from; returns Loom Canonical
     * CBOR (array of per-message `MailMessage` CBOR byte strings).
     */
expect fun Loom.mailSearch(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        text: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray
