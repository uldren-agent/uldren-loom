import CUldrenLoom
import Foundation

extension Loom {
    /// Create search collection `name` in `workspace` (created with the `search` facet if absent).
    /// `mapping` is a Loom Canonical CBOR map of `field -> [type_tag, stored, faceted]` (type 0 text,
    /// 1 keyword).
    public func searchCreate(workspace: String, name: String, mapping: Data) throws {
        let status = mapping.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_search_create(session, workspace, name, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Insert or replace the document at `id` (opaque bytes) in collection `name`. `doc` is a Loom
    /// Canonical CBOR map of `field -> value` (each value CBOR text or bytes).
    public func searchIndex(workspace: String, name: String, id: Data, doc: Data) throws {
        let status = id.withUnsafeBytes { iraw -> Int32 in
            let ibase = iraw.bindMemory(to: UInt8.self).baseAddress
            return doc.withUnsafeBytes { draw -> Int32 in
                let dbase = draw.bindMemory(to: UInt8.self).baseAddress
                return loom_search_index(session, workspace, name, ibase, UInt(iraw.count), dbase,
                                         UInt(draw.count))
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Fetch the document at `id` in collection `name` as a CBOR map of `field -> value`, or nil if the
    /// document is absent.
    public func searchGet(workspace: String, name: String, id: Data) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = id.withUnsafeBytes { iraw -> Int32 in
            let ibase = iraw.bindMemory(to: UInt8.self).baseAddress
            return loom_search_get(session, workspace, name, ibase, UInt(iraw.count), &ptr, &len, &found)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Remove the document at `id` from collection `name`; returns whether it was present.
    public func searchDelete(workspace: String, name: String, id: Data) throws -> Bool {
        var found: Int32 = 0
        let status = id.withUnsafeBytes { iraw -> Int32 in
            let ibase = iraw.bindMemory(to: UInt8.self).baseAddress
            return loom_search_delete(session, workspace, name, ibase, UInt(iraw.count), &found)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// The document ids of collection `name` as the Loom Canonical CBOR array of byte strings. When
    /// `hasPrefix` is true, only ids under `prefix` are returned.
    public func searchIds(workspace: String, name: String, prefix: Data = Data(),
                          hasPrefix: Bool = false) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = prefix.withUnsafeBytes { praw -> Int32 in
            let pbase = praw.bindMemory(to: UInt8.self).baseAddress
            return loom_search_ids_cbor(session, workspace, name, pbase, UInt(praw.count),
                                        hasPrefix ? 1 : 0, &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Replace the field `mapping` of collection `name`. `mapping` is a Loom Canonical CBOR map of
    /// `field -> [type_tag, stored, faceted]`.
    public func searchRemap(workspace: String, name: String, mapping: Data) throws {
        let status = mapping.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_search_remap(session, workspace, name, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Run the portable linear-scan query `request` against collection `name`. `request` is the Loom
    /// Canonical CBOR array `[query, limit, offset]`; the result is the response CBOR
    /// `[reduced, [[id, score_cell] ...]]`.
    public func searchQuery(workspace: String, name: String, request: Data) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = request.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_search_query_cbor(session, workspace, name, base, UInt(raw.count), &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }
}
