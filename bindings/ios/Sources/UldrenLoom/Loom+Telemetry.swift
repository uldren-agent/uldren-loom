import CUldrenLoom
import Foundation

extension Loom {
    public func metricsPutDescriptor(workspace: String, descriptor: Data) throws {
        let status = descriptor.withUnsafeBytes { raw -> Int32 in
            loom_metrics_put_descriptor(session, workspace, raw.bindMemory(to: UInt8.self).baseAddress, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func metricsGetDescriptor(workspace: String, name: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_metrics_get_descriptor(session, workspace, name, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        return takeTelemetryData(ptr, len)
    }

    public func metricsPutObservation(workspace: String, descriptorName: String, observation: Data) throws {
        let status = observation.withUnsafeBytes { raw -> Int32 in
            loom_metrics_put_observation(
                session,
                workspace,
                descriptorName,
                raw.bindMemory(to: UInt8.self).baseAddress,
                UInt(raw.count)
            )
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func metricsQuery(
        workspace: String,
        descriptorName: String,
        fromTimestampMs: UInt64,
        toTimestampMs: UInt64,
        maxSeries: UInt32,
        maxGroups: UInt32,
        maxSamples: UInt32,
        maxOutputBytes: UInt64,
        nowTimestampMs: UInt64
    ) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_metrics_query_cbor(
            session,
            workspace,
            descriptorName,
            fromTimestampMs,
            toTimestampMs,
            maxSeries,
            maxGroups,
            maxSamples,
            maxOutputBytes,
            nowTimestampMs,
            &ptr,
            &len
        )
        guard status == 0 else { throw LoomSql.lastError() }
        return takeTelemetryData(ptr, len)
    }

    public func logsPutRecord(workspace: String, record: Data) throws -> String {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = record.withUnsafeBytes { raw -> Int32 in
            loom_logs_put_record(session, workspace, raw.bindMemory(to: UInt8.self).baseAddress, UInt(raw.count), &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        let data = takeTelemetryData(ptr, len)
        guard let value = String(data: data, encoding: .utf8) else {
            throw LoomError(code: -1, message: "log record id is not UTF-8")
        }
        return value
    }

    public func logsGetRecord(workspace: String, recordId: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_logs_get_record(session, workspace, recordId, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        return takeTelemetryData(ptr, len)
    }

    public func logsQuery(
        workspace: String,
        fromTimeUnixNano: UInt64,
        toTimeUnixNano: UInt64,
        maxRecords: UInt32,
        maxOutputBytes: UInt64
    ) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_logs_query_cbor(
            session,
            workspace,
            fromTimeUnixNano,
            toTimeUnixNano,
            maxRecords,
            maxOutputBytes,
            &ptr,
            &len
        )
        guard status == 0 else { throw LoomSql.lastError() }
        return takeTelemetryData(ptr, len)
    }

    public func tracesPutSpan(workspace: String, span: Data) throws {
        let status = span.withUnsafeBytes { raw -> Int32 in
            loom_traces_put_span(session, workspace, raw.bindMemory(to: UInt8.self).baseAddress, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func tracesGetSpan(workspace: String, traceId: String, spanId: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_traces_get_span(session, workspace, traceId, spanId, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        return takeTelemetryData(ptr, len)
    }

    public func tracesTraceSpans(
        workspace: String,
        traceId: String,
        maxSpans: UInt32,
        maxOutputBytes: UInt64
    ) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_traces_trace_spans_cbor(session, workspace, traceId, maxSpans, maxOutputBytes, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return takeTelemetryData(ptr, len)
    }

    public func tracesQuery(
        workspace: String,
        fromStartTimeNs: UInt64,
        toStartTimeNs: UInt64,
        maxSpans: UInt32,
        maxOutputBytes: UInt64
    ) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_traces_query_cbor(
            session,
            workspace,
            fromStartTimeNs,
            toStartTimeNs,
            maxSpans,
            maxOutputBytes,
            &ptr,
            &len
        )
        guard status == 0 else { throw LoomSql.lastError() }
        return takeTelemetryData(ptr, len)
    }

    private func takeTelemetryData(_ ptr: UnsafeMutablePointer<UInt8>?, _ len: UInt) -> Data {
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }
}
