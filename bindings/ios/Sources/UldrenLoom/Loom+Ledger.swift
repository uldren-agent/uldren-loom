import CUldrenLoom
import Foundation

extension Loom {
    /// Append `payload` to ledger `collection` of `workspace` (created with the `ledger` facet if absent);
    /// returns the new entry's zero-based sequence.
    public func ledgerAppend(workspace: String, collection: String, payload: Data) throws -> UInt64 {
        var seq: UInt64 = 0
        let status = payload.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_ledger_append(session, workspace, collection, base, UInt(raw.count), &seq)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return seq
    }

    /// Fetch the payload at `seq` in ledger `collection`, or nil if absent.
    public func ledgerGet(workspace: String, collection: String, seq: UInt64) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_ledger_get(session, workspace, collection, seq, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The head chain hash of ledger `collection` as an `"algo:hex"` string, or nil when absent or empty.
    public func ledgerHead(workspace: String, collection: String) throws -> String? {
        var out: UnsafeMutablePointer<CChar>?
        var found: Int32 = 0
        let status = loom_ledger_head(session, workspace, collection, &out, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) }
    }

    /// The number of entries in ledger `collection` (0 when absent).
    public func ledgerLen(workspace: String, collection: String) throws -> UInt64 {
        var out: UInt64 = 0
        let status = loom_ledger_len(session, workspace, collection, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        return out
    }

    /// Recompute ledger `collection`'s chain from genesis and confirm every stored hash matches; an altered
    /// payload or broken link throws.
    public func ledgerVerify(workspace: String, collection: String) throws {
        let status = loom_ledger_verify(session, workspace, collection)
        guard status == 0 else { throw LoomSql.lastError() }
    }
}
