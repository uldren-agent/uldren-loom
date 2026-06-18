package ai.uldren.loom

data class AclScope(val kind: Int, val prefix: ByteArray) {
    companion object {
        fun ref(prefix: String) = AclScope(0, prefix.encodeToByteArray())
        fun collection(prefix: String) = AclScope(1, prefix.encodeToByteArray())
        fun path(prefix: String) = AclScope(2, prefix.encodeToByteArray())
        fun key(prefix: ByteArray) = AclScope(3, prefix)
        fun table(prefix: String) = AclScope(4, prefix.encodeToByteArray())
        fun exec(prefix: String) = AclScope(5, prefix.encodeToByteArray())
    }
}

expect fun Loom.authenticatePassphrase(
    path: String,
    principal: String,
    principalPassphrase: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
)

expect fun Loom.identityListJson(
    path: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.identityAddPrincipal(
    path: String,
    principalHandle: String,
    name: String,
    kind: String = "user",
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.identityRenamePrincipalHandle(
    path: String,
    principal: String,
    principalHandle: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.identitySetPassphrase(
    path: String,
    principal: String,
    principalPassphrase: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.identityRemovePrincipal(
    path: String,
    principal: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.identityAssignRole(
    path: String,
    principal: String,
    role: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.identityRevokeRole(
    path: String,
    principal: String,
    role: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): Boolean

expect fun Loom.identityCreateExternalCredential(
    path: String,
    principal: String,
    kind: String,
    label: String,
    issuer: String,
    subject: String,
    materialDigest: String? = null,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.identityRevokeExternalCredential(
    path: String,
    credential: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.identityAddPublicKey(
    path: String,
    principal: String,
    label: String,
    algorithm: String,
    publicKeyHex: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.identityRevokePublicKey(
    path: String,
    key: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.aclListJson(
    path: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.aclGrant(
    path: String,
    effect: Int,
    subject: String,
    workspace: String? = null,
    domain: String? = null,
    rightsMask: Int,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.aclRevoke(
    path: String,
    effect: Int,
    subject: String,
    workspace: String? = null,
    domain: String? = null,
    rightsMask: Int,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): Boolean

expect fun Loom.aclGrantScoped(
    path: String,
    effect: Int,
    subject: String,
    workspace: String? = null,
    domain: String? = null,
    rightsMask: Int,
    refGlob: String? = null,
    scopes: List<AclScope> = emptyList(),
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.aclGrantScopedPredicate(
    path: String,
    effect: Int,
    subject: String,
    workspace: String? = null,
    domain: String? = null,
    rightsMask: Int,
    refGlob: String? = null,
    scopes: List<AclScope> = emptyList(),
    predicateCel: String? = null,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.aclRevokeScoped(
    path: String,
    effect: Int,
    subject: String,
    workspace: String? = null,
    domain: String? = null,
    rightsMask: Int,
    refGlob: String? = null,
    scopes: List<AclScope> = emptyList(),
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): Boolean

expect fun Loom.aclRevokeScopedPredicate(
    path: String,
    effect: Int,
    subject: String,
    workspace: String? = null,
    domain: String? = null,
    rightsMask: Int,
    refGlob: String? = null,
    scopes: List<AclScope> = emptyList(),
    predicateCel: String? = null,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): Boolean

expect fun Loom.protectedRefListJson(
    path: String,
    workspace: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.protectedRefGetJson(
    path: String,
    workspace: String,
    refName: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.protectedRefSet(
    path: String,
    workspace: String,
    refName: String,
    fastForwardOnly: Boolean,
    signedCommitsRequired: Boolean,
    signedRefAdvanceRequired: Boolean,
    requiredReviewCount: Int,
    retentionLock: Boolean,
    governanceLock: Boolean,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.protectedRefRemove(
    path: String,
    workspace: String,
    refName: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): Boolean
