import CUldrenLoom
import Foundation

extension Loom {
    public func workspaceCreate(name: String? = nil, facet: String? = nil) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_workspace_create(session, name, facet, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    public func workspaceListJson() throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_workspace_list_json(session, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? "[]"
    }

    public func workspaceRename(_ workspace: String, to newName: String) throws {
        guard loom_workspace_rename(session, workspace, newName) == 0 else { throw LoomSql.lastError() }
    }

    public func workspaceDelete(_ workspace: String) throws {
        guard loom_workspace_delete(session, workspace) == 0 else { throw LoomSql.lastError() }
    }
}
