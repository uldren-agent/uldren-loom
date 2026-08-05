package ai.uldren.loom

expect fun Loom.importTableCsv(path: String, workspace: String, sourceScope: String, csvPayload: ByteArray, database: String, table: String, schema: String, primaryKey: String, mode: String, commit: Boolean, author: String? = null, message: String? = null, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.importRedmine(path: String, workspace: String, profile: String, sourceScope: String, snapshotPayload: ByteArray, fieldPolicy: String, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.importAsana(path: String, workspace: String, profile: String, sourceScope: String, snapshotPayload: ByteArray, fieldPolicy: String, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.importJira(path: String, workspace: String, profile: String, sourceScope: String, snapshotPayload: ByteArray, fieldPolicy: String, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.importConfluence(path: String, workspace: String, profile: String, sourceScope: String, snapshotPayload: ByteArray, defaultSpace: String, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.importSlack(path: String, workspace: String, profile: String, sourceScope: String, snapshotPayload: ByteArray, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.importDrive(path: String, workspace: String, profile: String, sourceScope: String, archivePayload: ByteArray, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.importMarkdown(path: String, workspace: String, profile: String, sourceScope: String, archivePayload: ByteArray, space: String, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.importNotion(path: String, workspace: String, profile: String, sourceScope: String, snapshotPayload: ByteArray, defaultSpace: String, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.studioReindexJson(path: String, workspace: String, profile: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.studioRevisionsRebuildJson(path: String, workspace: String, profile: String, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.storeBundleImport(path: String, bundle: ByteArray, dryRun: Boolean = false, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.auditCompact(path: String, throughSeq: Long, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.storeMaintenanceStatus(path: String, request: ByteArray, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.storeMaintenancePolicySet(path: String, update: ByteArray, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.storeMaintenanceRun(path: String, request: ByteArray, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.inferenceInstanceCreateJson(path: String, workspace: String, name: String, model: String, kind: String, runtime: String, preset: String? = null, settingsJson: String? = null, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.inferenceInstanceUpdateJson(path: String, workspace: String, name: String, preset: String? = null, settingsJson: String? = null, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.inferenceInstanceDeleteJson(path: String, workspace: String, name: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.serveListenerConfigureJson(path: String, requestJson: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.serveListenerListJson(path: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.serveListenerSetEnabledJson(path: String, listenerId: String, enabled: Boolean, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.serveListenerRemoveJson(path: String, listenerId: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.serveWebRouteListJson(path: String, listenerId: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.serveWebRouteSetJson(path: String, requestJson: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String
expect fun Loom.serveWebRouteRemoveJson(path: String, listenerId: String, routeId: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): String




















