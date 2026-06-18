package ai.uldren.loom

expect fun Loom.metricsPutDescriptor(
    path: String,
    workspace: String,
    descriptor: ByteArray,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.metricsGetDescriptor(
    path: String,
    workspace: String,
    name: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray?

expect fun Loom.metricsPutObservation(
    path: String,
    workspace: String,
    descriptorName: String,
    observation: ByteArray,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.metricsQuery(
    path: String,
    workspace: String,
    descriptorName: String,
    fromTimestampMs: Long,
    toTimestampMs: Long,
    maxSeries: Int,
    maxGroups: Int,
    maxSamples: Int,
    maxOutputBytes: Long,
    nowTimestampMs: Long,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

expect fun Loom.logsPutRecord(
    path: String,
    workspace: String,
    record: ByteArray,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): String

expect fun Loom.logsGetRecord(
    path: String,
    workspace: String,
    recordId: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray?

expect fun Loom.logsQuery(
    path: String,
    workspace: String,
    fromTimeUnixNano: Long,
    toTimeUnixNano: Long,
    maxRecords: Int,
    maxOutputBytes: Long,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

expect fun Loom.tracesPutSpan(
    path: String,
    workspace: String,
    span: ByteArray,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
)

expect fun Loom.tracesGetSpan(
    path: String,
    workspace: String,
    traceId: String,
    spanId: String,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray?

expect fun Loom.tracesTraceSpans(
    path: String,
    workspace: String,
    traceId: String,
    maxSpans: Int,
    maxOutputBytes: Long,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

expect fun Loom.tracesQuery(
    path: String,
    workspace: String,
    fromStartTimeNs: Long,
    toStartTimeNs: Long,
    maxSpans: Int,
    maxOutputBytes: Long,
    passphrase: String? = null,
    kek: ByteArray? = null,
    authPrincipal: String? = null,
    authPassphrase: String? = null,
): ByteArray

class TelemetryOps(private val s: LoomSession) {
    fun metricsPutDescriptor(workspace: String, descriptor: ByteArray) =
        Loom.metricsPutDescriptor(s.path, workspace, descriptor, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun metricsGetDescriptor(workspace: String, name: String): ByteArray? =
        Loom.metricsGetDescriptor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun metricsPutObservation(workspace: String, descriptorName: String, observation: ByteArray) =
        Loom.metricsPutObservation(
            s.path,
            workspace,
            descriptorName,
            observation,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun metricsQuery(
        workspace: String,
        descriptorName: String,
        fromTimestampMs: Long,
        toTimestampMs: Long,
        maxSeries: Int,
        maxGroups: Int,
        maxSamples: Int,
        maxOutputBytes: Long,
        nowTimestampMs: Long,
    ): ByteArray = Loom.metricsQuery(
        s.path,
        workspace,
        descriptorName,
        fromTimestampMs,
        toTimestampMs,
        maxSeries,
        maxGroups,
        maxSamples,
        maxOutputBytes,
        nowTimestampMs,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )

    fun logsPutRecord(workspace: String, record: ByteArray): String =
        Loom.logsPutRecord(s.path, workspace, record, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun logsGetRecord(workspace: String, recordId: String): ByteArray? =
        Loom.logsGetRecord(s.path, workspace, recordId, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun logsQuery(
        workspace: String,
        fromTimeUnixNano: Long,
        toTimeUnixNano: Long,
        maxRecords: Int,
        maxOutputBytes: Long,
    ): ByteArray = Loom.logsQuery(
        s.path,
        workspace,
        fromTimeUnixNano,
        toTimeUnixNano,
        maxRecords,
        maxOutputBytes,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )

    fun tracesPutSpan(workspace: String, span: ByteArray) =
        Loom.tracesPutSpan(s.path, workspace, span, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun tracesGetSpan(workspace: String, traceId: String, spanId: String): ByteArray? =
        Loom.tracesGetSpan(s.path, workspace, traceId, spanId, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun tracesTraceSpans(workspace: String, traceId: String, maxSpans: Int, maxOutputBytes: Long): ByteArray =
        Loom.tracesTraceSpans(
            s.path,
            workspace,
            traceId,
            maxSpans,
            maxOutputBytes,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun tracesQuery(
        workspace: String,
        fromStartTimeNs: Long,
        toStartTimeNs: Long,
        maxSpans: Int,
        maxOutputBytes: Long,
    ): ByteArray = Loom.tracesQuery(
        s.path,
        workspace,
        fromStartTimeNs,
        toStartTimeNs,
        maxSpans,
        maxOutputBytes,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )
}
