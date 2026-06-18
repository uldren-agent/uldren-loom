import CUldrenLoom
import Foundation

extension Loom {
    /// Create dataframe frame `name` from canonical `DataframePlan` CBOR.
    public func dataframeCreate(workspace: String, name: String, plan: Data) throws {
        let status = plan.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_dataframe_create(session, workspace, name, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Execute dataframe frame `name` and return canonical CBOR `[columns, rows]`.
    public func dataframeCollect(workspace: String, name: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_dataframe_collect_cbor(session, workspace, name, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Execute dataframe frame `name` and return at most `rows` rows as canonical CBOR `[columns, rows]`.
    public func dataframePreview(workspace: String, name: String, rows: UInt64) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_dataframe_preview_cbor(session, workspace, name, rows, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Materialize dataframe frame `name`; returns a CAS digest when the materialization target emits one.
    public func dataframeMaterialize(workspace: String, name: String) throws -> String? {
        var out: UnsafeMutablePointer<CChar>?
        var hasDigest: Int32 = 0
        let status = loom_dataframe_materialize(session, workspace, name, &out, &hasDigest)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        guard hasDigest != 0, let out else { return nil }
        return String(cString: out)
    }

    /// Canonical dataframe plan digest as `algo:hex`.
    public func dataframePlanDigest(workspace: String, name: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_dataframe_plan_digest(session, workspace, name, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        guard let out else { return "" }
        return String(cString: out)
    }

    /// Source digests pinned in the dataframe plan as canonical CBOR array of `algo:hex` strings.
    public func dataframeSourceDigests(workspace: String, name: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_dataframe_source_digests_cbor(session, workspace, name, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }
}
