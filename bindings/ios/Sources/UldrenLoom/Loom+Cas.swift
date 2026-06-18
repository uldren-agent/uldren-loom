import CUldrenLoom
import Foundation

extension Loom {
    /// Store `content` in the `cas` facet of `workspace` (by UUID or name, created if absent); returns
    /// the content address (`"algo:hex"`). Idempotent: identical bytes yield the same address.
    public func casPut(workspace: String, content: Data) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = content.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_cas_put(session, workspace, base, UInt(raw.count), &out)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    /// Fetch the blob addressed by `digest` from `workspace`, or nil if absent. An invalid digest throws
    /// INVALID_ARGUMENT; a content/digest mismatch throws INTEGRITY_FAILURE.
    public func casGet(workspace: String, digest: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_cas_get(session, workspace, digest, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Whether a blob addressed by `digest` is present in `workspace`. An invalid digest throws
    /// INVALID_ARGUMENT.
    public func casHas(workspace: String, digest: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_cas_has(session, workspace, digest, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// Drop the blob addressed by `digest` from `workspace`'s working tree (unreachable going forward);
    /// returns whether it was present. CAS stays immutable: bytes are GC-reclaimed once unreferenced,
    /// and an earlier commit that held the blob still restores it.
    public func casDelete(workspace: String, digest: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_cas_delete(session, workspace, digest, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// List the content addresses in `workspace`'s `cas` facet as a sorted JSON string array.
    public func casListJson(workspace: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_cas_list_json(session, workspace, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? "[]"
    }
}
