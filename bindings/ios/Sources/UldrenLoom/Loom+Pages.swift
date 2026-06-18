import CUldrenLoom
import Foundation

extension Loom {
    private func pagesString(_ call: (UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> Int32) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = call(&out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    public func spacesCreateJson(workspace: String, pageWorkspaceId: String,
                                 spaceId: String, title: String,
                                 expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_spaces_create_json(session, workspace, pageWorkspaceId, spaceId, title, expectedRoot ?? "", $0)
        }
    }

    public func spacesListJson(workspace: String, pageWorkspaceId: String) throws -> String {
        try pagesString { loom_spaces_list_json(session, workspace, pageWorkspaceId, $0) }
    }

    public func spacesGetJson(workspace: String, pageWorkspaceId: String, spaceId: String) throws -> String {
        try pagesString { loom_spaces_get_json(session, workspace, pageWorkspaceId, spaceId, $0) }
    }

    public func pagesCreateJson(workspace: String, pageWorkspaceId: String,
                                pageId: String, spaceId: String, parentPageId: String?,
                                title: String, expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_pages_create_json(
                session, workspace, pageWorkspaceId, pageId, spaceId, parentPageId ?? "",
                title, expectedRoot ?? "", $0
            )
        }
    }

    public func pagesUpdateJson(workspace: String, pageWorkspaceId: String,
                                pageId: String, bodyText: String,
                                expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_pages_update_json(
                session, workspace, pageWorkspaceId, pageId, bodyText,
                expectedRoot ?? "", $0
            )
        }
    }

    public func pagesPublishJson(workspace: String, pageWorkspaceId: String,
                                 pageId: String, expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_pages_publish_json(session, workspace, pageWorkspaceId, pageId, expectedRoot ?? "", $0)
        }
    }

    public func pagesGetJson(workspace: String, pageWorkspaceId: String, pageId: String) throws -> String {
        try pagesString { loom_pages_get_json(session, workspace, pageWorkspaceId, pageId, $0) }
    }

    public func pagesListJson(workspace: String, pageWorkspaceId: String) throws -> String {
        try pagesString { loom_pages_list_json(session, workspace, pageWorkspaceId, $0) }
    }

    public func pagesHistoryJson(workspace: String, pageWorkspaceId: String, pageId: String) throws -> String {
        try pagesString { loom_pages_history_json(session, workspace, pageWorkspaceId, pageId, $0) }
    }

    public func structuresCreateJson(workspace: String, pageWorkspaceId: String,
                                     structureId: String, spaceId: String, kind: String,
                                     title: String, expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_structures_create_json(
                session, workspace, pageWorkspaceId, structureId, spaceId, kind, title,
                expectedRoot ?? "", $0
            )
        }
    }

    public func structuresAddNodeJson(workspace: String, pageWorkspaceId: String,
                                      structureId: String, nodeId: String, kind: String,
                                      label: String, bodyDigest: String? = nil,
                                      entityRef: String? = nil, expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_structures_add_node_json(
                session, workspace, pageWorkspaceId, structureId, nodeId, kind, label,
                bodyDigest ?? "", entityRef ?? "", expectedRoot ?? "", $0
            )
        }
    }

    public func structuresUpdateNodeJson(workspace: String, pageWorkspaceId: String,
                                         structureId: String, nodeId: String, kind: String,
                                         label: String, bodyDigest: String? = nil,
                                         entityRef: String? = nil, expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_structures_update_node_json(
                session, workspace, pageWorkspaceId, structureId, nodeId, kind, label,
                bodyDigest ?? "", entityRef ?? "", expectedRoot ?? "", $0
            )
        }
    }

    public func structuresBindJson(workspace: String, pageWorkspaceId: String,
                                   structureId: String, nodeId: String,
                                   entityRef: String? = nil, expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_structures_bind_json(
                session, workspace, pageWorkspaceId, structureId, nodeId, entityRef ?? "",
                expectedRoot ?? "", $0
            )
        }
    }

    public func structuresMoveNodeJson(workspace: String, pageWorkspaceId: String,
                                       structureId: String, nodeId: String,
                                       parentNodeId: String? = nil, label: String? = nil,
                                       expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_structures_move_node_json(
                session, workspace, pageWorkspaceId, structureId, nodeId, parentNodeId ?? "",
                label ?? "", expectedRoot ?? "", $0
            )
        }
    }

    public func structuresLinkNodeJson(workspace: String, pageWorkspaceId: String,
                                       structureId: String, edgeId: String, srcNodeId: String,
                                       dstNodeId: String, label: String, targetRef: String? = nil,
                                       expectedRoot: String? = nil) throws -> String {
        try pagesString {
            loom_structures_link_node_json(
                session, workspace, pageWorkspaceId, structureId, edgeId, srcNodeId, dstNodeId,
                label, targetRef ?? "", expectedRoot ?? "", $0
            )
        }
    }

    public func structuresDecomposeToTicketsJson(workspace: String, pageWorkspaceId: String,
                                                 structureId: String, itemsJson: String) throws -> String {
        try pagesString {
            loom_structures_decompose_to_tickets_json(
                session, workspace, pageWorkspaceId, structureId, itemsJson, $0
            )
        }
    }

    public func structuresGetJson(workspace: String, pageWorkspaceId: String, structureId: String) throws -> String {
        try pagesString { loom_structures_get_json(session, workspace, pageWorkspaceId, structureId, $0) }
    }

    public func structuresListJson(workspace: String, pageWorkspaceId: String) throws -> String {
        try pagesString { loom_structures_list_json(session, workspace, pageWorkspaceId, $0) }
    }
}
