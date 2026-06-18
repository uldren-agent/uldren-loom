package ai.uldren.loom

actual fun Loom.mailCreateMailbox(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        displayName: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeMailCreateMailbox(path, workspace, principal, mailbox, displayName, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailDeleteMailbox(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean = LoomNative.nativeMailDeleteMailbox(path, workspace, principal, mailbox, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailListMailboxes(
        path: String,
        workspace: String,
        principal: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeMailListMailboxes(path, workspace, principal, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailIngestMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        raw: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): String =
        LoomNative.nativeMailIngestMessage(path, workspace, principal, mailbox, uid, raw, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailGetMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? =
        LoomNative.nativeMailGetMessage(path, workspace, principal, mailbox, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailToEml(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray? = LoomNative.nativeMailToEml(path, workspace, principal, mailbox, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailDeleteMessage(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): Boolean =
        LoomNative.nativeMailDeleteMessage(path, workspace, principal, mailbox, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailListMessages(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeMailListMessages(path, workspace, principal, mailbox, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailGetFlags(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeMailGetFlags(path, workspace, principal, mailbox, uid, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailSetFlags(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        flags: ByteArray,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ) = LoomNative.nativeMailSetFlags(path, workspace, principal, mailbox, uid, flags, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())


actual fun Loom.mailSearch(
        path: String,
        workspace: String,
        principal: String,
        mailbox: String,
        text: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    ): ByteArray = LoomNative.nativeMailSearch(path, workspace, principal, mailbox, text, passphrase?.encodeToByteArray(), kek, authPrincipal, authPassphrase?.encodeToByteArray())
