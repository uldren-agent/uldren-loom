package ai.uldren.loom

expect fun Loom.lifecycleDefineStandardJson(
    path: String,
    workspace: String,
    kind: String,
    version: String,
    completionPredicateDigest: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.lifecycleDefineJson(
    path: String,
    workspace: String,
    definition: ByteArray,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.lifecycleInstantiateJson(
    path: String,
    workspace: String,
    instanceId: String,
    definitionId: String,
    subjectRefsJson: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.lifecycleTransitionJson(
    path: String,
    workspace: String,
    instanceId: String,
    transitionId: String,
    toStageId: String,
    actorPrincipalId: String? = null,
    gateEvaluationsJson: String,
    snapshotDigest: String? = null,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.refsReconcileJson(
    path: String,
    workspace: String,
    max: Long,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String
