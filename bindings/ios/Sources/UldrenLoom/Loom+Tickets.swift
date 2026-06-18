import CUldrenLoom
import Foundation

extension Loom {
    private func ticketsString(_ call: (UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> Int32) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = call(&out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    public func ticketsProjectCreateJson(workspace: String, ticketWorkspaceId: String,
                                         projectId: String, keyPrefix: String, name: String,
                                         expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_project_create_json(
                session, workspace, ticketWorkspaceId, projectId, keyPrefix, name, expectedRoot, $0
            )
        }
    }

    public func ticketsProjectRekeyJson(workspace: String, ticketWorkspaceId: String,
                                        projectId: String, keyPrefix: String,
                                        expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_project_rekey_json(session, workspace, ticketWorkspaceId, projectId, keyPrefix, expectedRoot, $0)
        }
    }

    public func ticketsProjectSettingsGetJson(workspace: String, ticketWorkspaceId: String,
                                              projectId: String) throws -> String {
        try ticketsString { loom_tickets_project_settings_get_json(session, workspace, ticketWorkspaceId, projectId, $0) }
    }

    public func ticketsProjectSettingsSetJson(workspace: String, ticketWorkspaceId: String,
                                              projectId: String, defaultProjection: String?,
                                              enableProjectionsJson: String = "[]",
                                              disableProjectionsJson: String = "[]",
                                              actorEnforcement: String?,
                                              projectOwnerPrincipal: String?,
                                              clearProjectOwnerPrincipal: Bool = false,
                                              acceptanceAuthoritiesJson: String?,
                                              expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_project_settings_set_json(
                session, workspace, ticketWorkspaceId, projectId, defaultProjection ?? "",
                enableProjectionsJson, disableProjectionsJson, actorEnforcement ?? "",
                projectOwnerPrincipal ?? "", clearProjectOwnerPrincipal,
                acceptanceAuthoritiesJson ?? "", expectedRoot, $0
            )
        }
    }

    public func ticketsFieldsJson(workspace: String, ticketWorkspaceId: String,
                                  projectId: String, projection: String = "native",
                                  operation: String = "create") throws -> String {
        try ticketsString { loom_tickets_fields_json(session, workspace, ticketWorkspaceId, projectId, projection, operation, $0) }
    }

    public func ticketsFieldPutJson(workspace: String, ticketWorkspaceId: String,
                                    projectId: String, fieldId: String, key: String,
                                    name: String, description: String?, fieldType: String,
                                    optionSet: String?, maxLength: UInt32?,
                                    required: Bool = false, searchable: Bool = true,
                                    orderable: Bool = false, cardinality: String = "optional",
                                    applicableTypeIdsJson: String = "[]",
                                    expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_field_put_json(
                session, workspace, ticketWorkspaceId, projectId, fieldId, key, name,
                description ?? "", fieldType, optionSet ?? "", maxLength ?? 0, maxLength != nil,
                required, searchable, orderable, cardinality, applicableTypeIdsJson,
                expectedRoot, $0
            )
        }
    }

    public func ticketsFieldRetireJson(workspace: String, ticketWorkspaceId: String,
                                       projectId: String, fieldId: String,
                                       expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_field_retire_json(session, workspace, ticketWorkspaceId, projectId, fieldId, expectedRoot, $0)
        }
    }

    public func ticketsCreateJson(workspace: String, ticketWorkspaceId: String,
                                  projectId: String, ticketType: String, externalSource: String?,
                                  externalId: String?, fieldsJson: String,
                                  policyLabelsJson: String = "[]",
                                  expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_create_json(
                session, workspace, ticketWorkspaceId, projectId, ticketType, externalSource ?? "",
                externalId ?? "", fieldsJson, policyLabelsJson, expectedRoot, $0
            )
        }
    }

    public func ticketsUpdateJson(workspace: String, ticketWorkspaceId: String, ticketId: String,
                                  setFieldsJson: String = "{}", deleteFieldsJson: String = "[]",
                                  action: String?, targetStatus: String?,
                                  observedSourceStatus: String?,
                                  observedWorkflowVersion: String?, assignee: String?,
                                  commentId: String? = nil, commentType: String? = nil,
                                  commentBody: String? = nil,
                                  expectedRoot: String, commentsJson: String? = nil,
                                  relationSetsJson: String? = nil,
                                  relationRemovesJson: String? = nil) throws -> String {
        try ticketsString {
            loom_tickets_update_json(
                session, workspace, ticketWorkspaceId, ticketId, setFieldsJson, deleteFieldsJson,
                action ?? "", targetStatus ?? "", observedSourceStatus ?? "",
                observedWorkflowVersion ?? "", assignee ?? "", commentId ?? "", commentType ?? "",
                commentBody ?? "", expectedRoot, commentsJson ?? "", relationSetsJson ?? "",
                relationRemovesJson ?? "", $0
            )
        }
    }

    public func ticketsDeleteJson(workspace: String, ticketWorkspaceId: String,
                                  ticketId: String, expectedRoot: String) throws -> String {
        try ticketsString { loom_tickets_delete_json(session, workspace, ticketWorkspaceId, ticketId, expectedRoot, $0) }
    }

    public func ticketsCommentsJson(workspace: String, ticketWorkspaceId: String,
                                    ticketId: String) throws -> String {
        try ticketsString { loom_tickets_comments_json(session, workspace, ticketWorkspaceId, ticketId, $0) }
    }

    public func ticketsCommentAddJson(workspace: String, ticketWorkspaceId: String,
                                      ticketId: String, commentId: String?,
                                      commentType: String?, body: String,
                                      expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_comment_add_json(
                session, workspace, ticketWorkspaceId, ticketId, commentId ?? "",
                commentType ?? "", body, expectedRoot, $0
            )
        }
    }

    public func ticketsCommentUpdateJson(workspace: String, ticketWorkspaceId: String,
                                         ticketId: String, commentId: String,
                                         commentType: String?, body: String?,
                                         expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_comment_update_json(
                session, workspace, ticketWorkspaceId, ticketId, commentId,
                commentType ?? "", body ?? "", expectedRoot, $0
            )
        }
    }

    public func ticketsCommentDeleteJson(workspace: String, ticketWorkspaceId: String,
                                         ticketId: String, commentId: String,
                                         expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_comment_delete_json(session, workspace, ticketWorkspaceId, ticketId, commentId, expectedRoot, $0)
        }
    }

    public func ticketsRelationSetJson(workspace: String, ticketWorkspaceId: String,
                                       ticketId: String, relationId: String, kind: String,
                                       targetId: String, expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_relation_set_json(
                session, workspace, ticketWorkspaceId, ticketId, relationId, kind, targetId, expectedRoot, $0
            )
        }
    }

    public func ticketsRelationRemoveJson(workspace: String, ticketWorkspaceId: String,
                                          ticketId: String, relationId: String,
                                          expectedRoot: String) throws -> String {
        try ticketsString {
            loom_tickets_relation_remove_json(session, workspace, ticketWorkspaceId, ticketId, relationId, expectedRoot, $0)
        }
    }

    public func ticketsGetJson(workspace: String, ticketWorkspaceId: String,
                               ticketId: String, projection: String = "native") throws -> String {
        try ticketsString { loom_tickets_get_json(session, workspace, ticketWorkspaceId, ticketId, projection, $0) }
    }

    public func ticketsListJson(workspace: String, ticketWorkspaceId: String,
                                projection: String = "native") throws -> String {
        try ticketsString { loom_tickets_list_json(session, workspace, ticketWorkspaceId, projection, $0) }
    }

    public func ticketsHistoryJson(workspace: String, ticketWorkspaceId: String,
                                   ticketId: String) throws -> String {
        try ticketsString { loom_tickets_history_json(session, workspace, ticketWorkspaceId, ticketId, $0) }
    }
}
