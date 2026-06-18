import CUldrenLoom
import Foundation

private func withOptionalCString<T>(_ value: String?, _ body: (UnsafePointer<CChar>?) -> T) -> T {
    guard let value, !value.isEmpty else {
        return body(nil)
    }
    return value.withCString(body)
}

extension Loom {
    public func vcsBlame(workspace: String, branch: String) throws -> LoomResult {
        try Loom.openResult(vcsBlameBytes(workspace: workspace, branch: branch))
    }

    public func vcsBlameBytes(workspace: String, branch: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_vcs_blame(session, workspace, branch, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func vcsDiff(workspace: String, fromCommit: String,
                        toCommit: String) throws -> LoomResult {
        try Loom.openResult(
            vcsDiffBytes(workspace: workspace, fromCommit: fromCommit, toCommit: toCommit)
        )
    }

    public func vcsDiffBytes(workspace: String, fromCommit: String,
                             toCommit: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_vcs_diff(session, workspace, fromCommit, toCommit, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func watchSubscribe(workspace: String, branch: String, facet: String? = nil,
                               pathPrefix: String? = nil, changeKinds: [String] = [],
                               fromCommit: String? = nil) throws -> String {
        let joinedKinds = changeKinds.joined(separator: ",")
        var out: UnsafeMutablePointer<CChar>?
        let status = withOptionalCString(facet) { facetPtr in
            withOptionalCString(pathPrefix) { pathPtr in
                withOptionalCString(joinedKinds.isEmpty ? nil : joinedKinds) { kindsPtr in
                    withOptionalCString(fromCommit) { fromPtr in
                        loom_watch_subscribe(
                            session, workspace, branch, facetPtr, pathPtr, kindsPtr, fromPtr, &out
                        )
                    }
                }
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
        guard let out else { return "" }
        defer { loom_string_free(out) }
        return String(cString: out)
    }

    public func watchPollBytes(cursor: String, max: UInt32) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_watch_poll(session, cursor, max, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }
}
