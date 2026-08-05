package ai.uldren.loom

private fun String?.keyBytes(): ByteArray? = this?.encodeToByteArray()

actual fun Loom.lifecycleDefineStandardJson(path: String, workspace: String, kind: String, version: String, completionPredicateDigest: String, passphrase: String?, kek: ByteArray?, authPrincipal: String?, authPassphrase: String?): String =
    LoomNative.nativeLifecycleDefineStandardJson(path, workspace, kind, version, completionPredicateDigest, passphrase.keyBytes(), kek, authPrincipal, authPassphrase.keyBytes())

actual fun Loom.lifecycleDefineJson(path: String, workspace: String, definition: ByteArray, passphrase: String?, kek: ByteArray?, authPrincipal: String?, authPassphrase: String?): String =
    LoomNative.nativeLifecycleDefineJson(path, workspace, definition, passphrase.keyBytes(), kek, authPrincipal, authPassphrase.keyBytes())

actual fun Loom.lifecycleInstantiateJson(path: String, workspace: String, instanceId: String, definitionId: String, subjectRefsJson: String, passphrase: String?, kek: ByteArray?, authPrincipal: String?, authPassphrase: String?): String =
    LoomNative.nativeLifecycleInstantiateJson(path, workspace, instanceId, definitionId, subjectRefsJson, passphrase.keyBytes(), kek, authPrincipal, authPassphrase.keyBytes())

actual fun Loom.lifecycleTransitionJson(path: String, workspace: String, instanceId: String, transitionId: String, toStageId: String, actorPrincipalId: String?, gateEvaluationsJson: String, snapshotDigest: String?, passphrase: String?, kek: ByteArray?, authPrincipal: String?, authPassphrase: String?): String =
    LoomNative.nativeLifecycleTransitionJson(path, workspace, instanceId, transitionId, toStageId, actorPrincipalId, gateEvaluationsJson, snapshotDigest, passphrase.keyBytes(), kek, authPrincipal, authPassphrase.keyBytes())

actual fun Loom.refsReconcileJson(path: String, workspace: String, max: Long, passphrase: String?, kek: ByteArray?, authPrincipal: String?, authPassphrase: String?): String =
    LoomNative.nativeRefsReconcileJson(path, workspace, max, passphrase.keyBytes(), kek, authPrincipal, authPassphrase.keyBytes())
