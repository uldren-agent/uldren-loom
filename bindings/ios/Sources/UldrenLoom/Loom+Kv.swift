import CUldrenLoom
import Foundation

extension Loom {
    /// Put `value` at the typed `key` (Loom Canonical CBOR cell) in map `collection` of `workspace` (UUID or
    /// name, created with the `kv` facet if absent). A later put at the same key replaces the value.
    public func kvPut(workspace: String, collection: String, key: Data, value: Data) throws {
        let status = key.withUnsafeBytes { kraw -> Int32 in
            let kbase = kraw.bindMemory(to: UInt8.self).baseAddress
            return value.withUnsafeBytes { vraw -> Int32 in
                let vbase = vraw.bindMemory(to: UInt8.self).baseAddress
                return loom_kv_put(session, workspace, collection, kbase, UInt(kraw.count), vbase,
                                   UInt(vraw.count))
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Fetch the value at typed `key` in map `collection` of `workspace`, or nil if the key or map is absent.
    public func kvGet(workspace: String, collection: String, key: Data) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = key.withUnsafeBytes { kraw -> Int32 in
            let kbase = kraw.bindMemory(to: UInt8.self).baseAddress
            return loom_kv_get(session, workspace, collection, kbase, UInt(kraw.count), &ptr, &len, &found)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Remove the typed `key` from map `collection` of `workspace`; returns whether it was present.
    public func kvDelete(workspace: String, collection: String, key: Data) throws -> Bool {
        var found: Int32 = 0
        let status = key.withUnsafeBytes { kraw -> Int32 in
            let kbase = kraw.bindMemory(to: UInt8.self).baseAddress
            return loom_kv_delete(session, workspace, collection, kbase, UInt(kraw.count), &found)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// List map `collection` of `workspace` as the Loom Canonical CBOR array of `[key, value]` pairs in key
    /// order (an absent map is the empty array).
    public func kvList(workspace: String, collection: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_kv_list_cbor(session, workspace, collection, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The entries of map `collection` with `lo <= key < hi` (half-open, key order) as the Loom Canonical CBOR
    /// array of `[key, value]` pairs. `lo`/`hi` are typed-cell CBOR keys.
    public func kvRange(workspace: String, collection: String, lo: Data, hi: Data) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = lo.withUnsafeBytes { lraw -> Int32 in
            let lbase = lraw.bindMemory(to: UInt8.self).baseAddress
            return hi.withUnsafeBytes { hraw -> Int32 in
                let hbase = hraw.bindMemory(to: UInt8.self).baseAddress
                return loom_kv_range_cbor(session, workspace, collection, lbase, UInt(lraw.count), hbase,
                                          UInt(hraw.count), &ptr, &len)
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    // Tags: eviction 0 none/1 lru/2 lfu/3 random/4 fifo/5 ttl_priority; onEvict 0 drop/1 write_through;
    // backPressure 0 block/1 pressure/2 assisted. maxEntries/maxBytes/flushBatch 0 = unbounded;
    // flushHighWaterPct < 0 = only the hard bound.
    public func managementKvSetConfig(workspace: String, collection: String, tier: Int32,
                                      defaultTtlMs: UInt64 = 0, defaultIdleTtlMs: UInt64 = 0,
                                      readThrough: Bool = false, writeThrough: Bool = false,
                                      maxEntries: UInt64 = 0, maxBytes: UInt64 = 0,
                                      eviction: Int32 = 0, onEvict: Int32 = 0,
                                      writeBehind: Bool = false, writeAround: Bool = false,
                                      backPressure: Int32 = 0, flushHighWaterPct: Int32 = -1,
                                      flushBatch: UInt64 = 0) throws {
        let status = loom_management_kv_set_config(session, workspace, collection, tier, defaultTtlMs,
                                                   defaultIdleTtlMs, readThrough ? 1 : 0,
                                                   writeThrough ? 1 : 0, maxEntries, maxBytes,
                                                   eviction, onEvict, writeBehind ? 1 : 0,
                                                   writeAround ? 1 : 0, backPressure,
                                                   flushHighWaterPct, flushBatch)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func managementKvGetConfigJson(workspace: String, collection: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_management_kv_get_config_json(session, workspace, collection, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? "{}"
    }
}
