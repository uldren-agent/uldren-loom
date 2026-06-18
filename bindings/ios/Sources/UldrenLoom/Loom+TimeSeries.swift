import CUldrenLoom
import Foundation

extension Loom {
    /// Record `value` at timestamp `ts` in series `collection` of `workspace` (created with the `time-series`
    /// facet if absent). A repeated timestamp replaces the point.
    public func tsPut(workspace: String, collection: String, ts: Int64, value: Data) throws {
        let status = value.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_ts_put(session, workspace, collection, ts, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Fetch the point at timestamp `ts` in series `collection`, or nil if absent.
    public func tsGet(workspace: String, collection: String, ts: Int64) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_ts_get(session, workspace, collection, ts, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The points of series `collection` with `from <= ts < to` (half-open, time order) as the Loom Canonical
    /// CBOR array of `[ts, value]` pairs.
    public func tsRange(workspace: String, collection: String, from: Int64, to: Int64) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_ts_range_cbor(session, workspace, collection, from, to, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The most recent point of series `collection` as `(ts, value)`, or nil if absent/empty.
    public func tsLatest(workspace: String, collection: String) throws -> (ts: Int64, value: Data)? {
        var ts: Int64 = 0
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_ts_latest(session, workspace, collection, &ts, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        let value = (ptr != nil && len > 0)
            ? Data(UnsafeBufferPointer(start: ptr, count: Int(len))) : Data()
        return (ts: ts, value: value)
    }
}
