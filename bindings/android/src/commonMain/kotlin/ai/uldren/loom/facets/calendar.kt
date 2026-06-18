package ai.uldren.loom

    /**
     * Create (or replace the metadata of) calendar collection [collection] under [principal] in
     * [workspace] (UUID or name, created with the `calendar` facet if absent). [components] is a
     * comma-separated component set ("event,todo"; "" is the empty set).
     */
expect fun Loom.calCreateCollection(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        displayName: String,
        components: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Delete calendar collection [collection] under [principal] and its entries; returns whether it existed. */
expect fun Loom.calDeleteCollection(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** List the calendar collection ids under [principal] as Loom Canonical CBOR (array of text strings). */
expect fun Loom.calListCollections(
        path: String,
        workspace: String,
        principal: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** Put the calendar [entry] (its `CalendarEntry` canonical CBOR) into [collection], keyed by its UID. */
expect fun Loom.calPutEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        entry: ByteArray,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    )


    /** Fetch the calendar entry at [uid] in [collection] as its `CalendarEntry` canonical CBOR, or null. */
expect fun Loom.calGetEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray?


    /** Remove the calendar entry at [uid] in [collection]; returns whether it was present. */
expect fun Loom.calDeleteEntry(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): Boolean


    /** List [collection] as Loom Canonical CBOR (array of per-entry `CalendarEntry` CBOR byte strings). */
expect fun Loom.calListEntries(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /**
     * Expand [collection] into occurrences within `[from, to)` (both `YYYYMMDDTHHMMSS`) as Loom Canonical
     * CBOR (an array of `[uid, "YYYYMMDDTHHMMSS"]` pairs).
     */
expect fun Loom.calRange(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        from: String,
        to: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /**
     * Search [collection] by [component] ("" any, "event", or "todo") and case-insensitive [text]; returns
     * Loom Canonical CBOR (array of per-entry `CalendarEntry` CBOR byte strings).
     */
expect fun Loom.calSearch(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        component: String,
        text: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): ByteArray


    /** The on-demand iCalendar (`.ics`) projection of the entry at [uid], or null if absent. */
expect fun Loom.calEntryIcs(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String?


    /** Parse iCalendar [ics] and store it as a record in [collection]; returns the new ETag ("algo:hex"). */
expect fun Loom.calPutIcs(
        path: String,
        workspace: String,
        principal: String,
        collection: String,
        ics: String,
        passphrase: String? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: String? = null,
    ): String
