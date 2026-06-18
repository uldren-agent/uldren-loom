import CUldrenLoom
import Foundation

public struct LoomDocumentText: Equatable {
    public let text: String
    public let digest: String
    public let entityTag: String
}

public struct LoomDocumentBinary: Equatable {
    public let bytes: Data
    public let digest: String
    public let entityTag: String
}

public struct LoomDocumentPutResult: Equatable {
    public let digest: String
    public let entityTag: String
}

extension Loom {
    public func docPutText(
        workspace: String,
        collection: String,
        id: String,
        text: String,
        expectedEntityTag: String? = nil
    ) throws -> LoomDocumentPutResult {
        var digest: UnsafeMutablePointer<CChar>?
        var entityTag: UnsafeMutablePointer<CChar>?
        let status = loom_doc_put_text(session, workspace, collection, id, text, expectedEntityTag, &digest, &entityTag)
        guard status == 0 else { throw LoomSql.lastError() }
        return LoomDocumentPutResult(digest: try takeDocumentString(digest), entityTag: try takeDocumentString(entityTag))
    }

    public func docGetText(workspace: String, collection: String, id: String) throws -> LoomDocumentText? {
        var text: UnsafeMutablePointer<CChar>?
        var digest: UnsafeMutablePointer<CChar>?
        var entityTag: UnsafeMutablePointer<CChar>?
        var found: Int32 = 0
        let status = loom_doc_get_text(session, workspace, collection, id, &text, &digest, &entityTag, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        return LoomDocumentText(text: try takeDocumentString(text), digest: try takeDocumentString(digest), entityTag: try takeDocumentString(entityTag))
    }

    public func docPutBinary(
        workspace: String,
        collection: String,
        id: String,
        bytes: Data,
        expectedEntityTag: String? = nil
    ) throws -> LoomDocumentPutResult {
        var digest: UnsafeMutablePointer<CChar>?
        var entityTag: UnsafeMutablePointer<CChar>?
        let status = bytes.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_doc_put_binary(session, workspace, collection, id, base, UInt(raw.count), expectedEntityTag, &digest, &entityTag)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return LoomDocumentPutResult(digest: try takeDocumentString(digest), entityTag: try takeDocumentString(entityTag))
    }

    public func docGetBinary(workspace: String, collection: String, id: String) throws -> LoomDocumentBinary? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var digest: UnsafeMutablePointer<CChar>?
        var entityTag: UnsafeMutablePointer<CChar>?
        var found: Int32 = 0
        let status = loom_doc_get_binary(session, workspace, collection, id, &ptr, &len, &digest, &entityTag, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        let bytes: Data
        if let ptr, len > 0 {
            bytes = Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
        } else {
            bytes = Data()
        }
        return LoomDocumentBinary(bytes: bytes, digest: try takeDocumentString(digest), entityTag: try takeDocumentString(entityTag))
    }

    /// Remove `id` from collection `collection`; returns whether it was present.
    public func docDelete(workspace: String, collection: String, id: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_doc_delete(session, workspace, collection, id, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    public func docListBinary(workspace: String, collection: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_doc_list_binary_cbor(session, workspace, collection, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    public func docIndexCreate(
        workspace: String,
        collection: String,
        name: String,
        path: String,
        unique: Bool = false
    ) throws {
        let status = loom_doc_index_create(
            session,
            workspace,
            collection,
            name,
            path,
            unique ? 1 : 0
        )
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func docIndexCreateJson(
        workspace: String,
        collection: String,
        declarationJson: Data
    ) throws {
        let status = declarationJson.withUnsafeBytes { bytes in
            loom_doc_index_create_json(
                session,
                workspace,
                collection,
                bytes.bindMemory(to: UInt8.self).baseAddress,
                UInt(declarationJson.count)
            )
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func docIndexDrop(workspace: String, collection: String, name: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_doc_index_drop(session, workspace, collection, name, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    public func docIndexRebuild(workspace: String, collection: String, name: String) throws {
        let status = loom_doc_index_rebuild(session, workspace, collection, name)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func docIndexListJson(workspace: String, collection: String) throws -> String {
        try documentJsonResult { ptr, len in
            loom_doc_index_list_json(session, workspace, collection, &ptr, &len)
        }
    }

    public func docIndexStatusJson(workspace: String, collection: String) throws -> String {
        try documentJsonResult { ptr, len in
            loom_doc_index_status_json(session, workspace, collection, &ptr, &len)
        }
    }

    public func docFindJson(
        workspace: String,
        collection: String,
        index: String,
        valueJson: String
    ) throws -> String {
        let bytes = Array(valueJson.utf8)
        return try bytes.withUnsafeBufferPointer { rawValue in
            try documentJsonResult { ptr, len in
                loom_doc_find_json(
                    session,
                    workspace,
                    collection,
                    index,
                    rawValue.baseAddress,
                    UInt(rawValue.count),
                    &ptr,
                    &len
                )
            }
        }
    }

    public func docQueryJson(workspace: String, collection: String, queryJson: String) throws -> String {
        let bytes = Array(queryJson.utf8)
        return try bytes.withUnsafeBufferPointer { rawQuery in
            try documentJsonResult { ptr, len in
                loom_doc_query_json(
                    session,
                    workspace,
                    collection,
                    rawQuery.baseAddress,
                    UInt(rawQuery.count),
                    &ptr,
                    &len
                )
            }
        }
    }

    private func documentJsonResult(_ body: (inout UnsafeMutablePointer<UInt8>?, inout UInt) -> Int32) throws -> String {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = body(&ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return "" }
        let data = Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
        guard let value = String(data: data, encoding: .utf8) else {
            throw LoomError(code: -1, message: "document JSON result is not UTF-8")
        }
        return value
    }

    private func takeDocumentString(_ ptr: UnsafeMutablePointer<CChar>?) throws -> String {
        guard let ptr else { return "" }
        defer { loom_string_free(ptr) }
        return String(cString: ptr)
    }
}
