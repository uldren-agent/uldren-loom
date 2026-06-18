package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.nio.charset.StandardCharsets;

/**
 * Native telemetry facet operations for metrics, logs, and traces. Arguments and return values use
 * canonical CBOR bytes for facet-native records.
 */
public final class TelemetryOps {
    private final LoomSession session;

    TelemetryOps(LoomSession session) {
        this.session = session;
    }

    public void metricsPutDescriptor(String workspace, byte[] descriptor) {
        session.onHandle("loom_metrics_put_descriptor", (arena, handle) -> {
            int status = (int) Loom.LOOM_METRICS_PUT_DESCRIPTOR.invokeExact(
                    handle, arena.allocateFrom(workspace), Loom.bytesOrNull(arena, descriptor),
                    (long) (descriptor != null ? descriptor.length : 0));
            if (status != 0) {
                throw Loom.lastError("loom_metrics_put_descriptor");
            }
            return null;
        });
    }

    public byte[] metricsGetDescriptor(String workspace, String name) {
        return session.onHandle("loom_metrics_get_descriptor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
            int status = (int) Loom.LOOM_METRICS_GET_DESCRIPTOR.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen,
                    outFound);
            if (status != 0) {
                throw Loom.lastError("loom_metrics_get_descriptor");
            }
            if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                return null;
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public void metricsPutObservation(String workspace, String descriptorName, byte[] observation) {
        session.onHandle("loom_metrics_put_observation", (arena, handle) -> {
            int status = (int) Loom.LOOM_METRICS_PUT_OBSERVATION.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(descriptorName),
                    Loom.bytesOrNull(arena, observation), (long) (observation != null ? observation.length : 0));
            if (status != 0) {
                throw Loom.lastError("loom_metrics_put_observation");
            }
            return null;
        });
    }

    public byte[] metricsQuery(String workspace, String descriptorName, long fromTimestampMs,
            long toTimestampMs, int maxSeries, int maxGroups, int maxSamples, long maxOutputBytes,
            long nowTimestampMs) {
        return session.onHandle("loom_metrics_query_cbor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_METRICS_QUERY_CBOR.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(descriptorName),
                    fromTimestampMs, toTimestampMs, maxSeries, maxGroups, maxSamples, maxOutputBytes,
                    nowTimestampMs, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_metrics_query_cbor");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public String logsPutRecord(String workspace, byte[] record) {
        return session.onHandle("loom_logs_put_record", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_LOGS_PUT_RECORD.invokeExact(
                    handle, arena.allocateFrom(workspace), Loom.bytesOrNull(arena, record),
                    (long) (record != null ? record.length : 0), outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_logs_put_record");
            }
            return new String(Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0)), StandardCharsets.UTF_8);
        });
    }

    public byte[] logsGetRecord(String workspace, String recordId) {
        return session.onHandle("loom_logs_get_record", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
            int status = (int) Loom.LOOM_LOGS_GET_RECORD.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(recordId), outPtr, outLen,
                    outFound);
            if (status != 0) {
                throw Loom.lastError("loom_logs_get_record");
            }
            if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                return null;
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] logsQuery(String workspace, long fromTimeUnixNano, long toTimeUnixNano,
            int maxRecords, long maxOutputBytes) {
        return session.onHandle("loom_logs_query_cbor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_LOGS_QUERY_CBOR.invokeExact(
                    handle, arena.allocateFrom(workspace), fromTimeUnixNano, toTimeUnixNano, maxRecords,
                    maxOutputBytes, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_logs_query_cbor");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public void tracesPutSpan(String workspace, byte[] span) {
        session.onHandle("loom_traces_put_span", (arena, handle) -> {
            int status = (int) Loom.LOOM_TRACES_PUT_SPAN.invokeExact(
                    handle, arena.allocateFrom(workspace), Loom.bytesOrNull(arena, span),
                    (long) (span != null ? span.length : 0));
            if (status != 0) {
                throw Loom.lastError("loom_traces_put_span");
            }
            return null;
        });
    }

    public byte[] tracesGetSpan(String workspace, String traceId, String spanId) {
        return session.onHandle("loom_traces_get_span", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
            int status = (int) Loom.LOOM_TRACES_GET_SPAN.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(traceId),
                    arena.allocateFrom(spanId), outPtr, outLen, outFound);
            if (status != 0) {
                throw Loom.lastError("loom_traces_get_span");
            }
            if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                return null;
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] tracesTraceSpans(String workspace, String traceId, int maxSpans,
            long maxOutputBytes) {
        return session.onHandle("loom_traces_trace_spans_cbor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_TRACES_TRACE_SPANS_CBOR.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(traceId), maxSpans,
                    maxOutputBytes, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_traces_trace_spans_cbor");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] tracesQuery(String workspace, long fromStartTimeNs, long toStartTimeNs,
            int maxSpans, long maxOutputBytes) {
        return session.onHandle("loom_traces_query_cbor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_TRACES_QUERY_CBOR.invokeExact(
                    handle, arena.allocateFrom(workspace), fromStartTimeNs, toStartTimeNs, maxSpans,
                    maxOutputBytes, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_traces_query_cbor");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }
}
