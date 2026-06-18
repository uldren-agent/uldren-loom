import CUldrenLoom
import Foundation

extension Loom {
    private func chatString(_ call: (UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> Int32) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = call(&out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    public func chatCreateChannelJson(workspace: String, chatWorkspaceId: String,
                                      channelId: String, channelHandle: String,
                                      name: String) throws -> String {
        try chatString { loom_chat_create_channel_json(session, workspace, chatWorkspaceId, channelId, channelHandle, name, $0) }
    }

    public func chatRenameChannelJson(workspace: String, chatWorkspaceId: String,
                                      selector: String, channelHandle: String) throws -> String {
        try chatString { loom_chat_rename_channel_json(session, workspace, chatWorkspaceId, selector, channelHandle, $0) }
    }

    public func chatListChannelsJson(workspace: String, chatWorkspaceId: String) throws -> String {
        try chatString { loom_chat_list_channels_json(session, workspace, chatWorkspaceId, $0) }
    }

    public func chatPostMessageJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, messageId: String, threadId: String?,
                                    bodyText: String) throws -> String {
        try chatString {
            loom_chat_post_message_json(
                session, workspace, chatWorkspaceId, channelId, messageId, threadId ?? "",
                bodyText, $0
            )
        }
    }

    public func chatEditMessageJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, messageId: String,
                                    bodyText: String) throws -> String {
        try chatString {
            loom_chat_edit_message_json(
                session, workspace, chatWorkspaceId, channelId, messageId,
                bodyText, $0
            )
        }
    }

    public func chatRedactMessageJson(workspace: String, chatWorkspaceId: String,
                                      channelId: String, messageId: String,
                                      reason: String?) throws -> String {
        try chatString { loom_chat_redact_message_json(session, workspace, chatWorkspaceId, channelId, messageId, reason ?? "", $0) }
    }

    public func chatCreateThreadJson(workspace: String, chatWorkspaceId: String,
                                     channelId: String, threadId: String,
                                     parentMessageId: String) throws -> String {
        try chatString { loom_chat_create_thread_json(session, workspace, chatWorkspaceId, channelId, threadId, parentMessageId, $0) }
    }

    public func chatCreateTaskJson(workspace: String, chatWorkspaceId: String,
                                   channelId: String, taskId: String,
                                   messageId: String, title: String) throws -> String {
        try chatString { loom_chat_create_task_json(session, workspace, chatWorkspaceId, channelId, taskId, messageId, title, $0) }
    }

    public func chatClaimTaskJson(workspace: String, chatWorkspaceId: String,
                                  channelId: String, taskId: String,
                                  claimId: String, leaseToken: String?) throws -> String {
        try chatString { loom_chat_claim_task_json(session, workspace, chatWorkspaceId, channelId, taskId, claimId, leaseToken ?? "", $0) }
    }

    public func chatCompleteTaskJson(workspace: String, chatWorkspaceId: String,
                                     channelId: String, taskId: String, claimId: String,
                                     resultMessageId: String?) throws -> String {
        try chatString { loom_chat_complete_task_json(session, workspace, chatWorkspaceId, channelId, taskId, claimId, resultMessageId ?? "", $0) }
    }

    public func chatInvokeAgentJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, invocationId: String,
                                    agentPrincipal: String, sourceMessageIdsJson: String,
                                    promptText: String) throws -> String {
        try chatString {
            loom_chat_invoke_agent_json(
                session, workspace, chatWorkspaceId, channelId, invocationId,
                agentPrincipal, sourceMessageIdsJson, promptText, $0
            )
        }
    }

    public func chatAgentReplyJson(workspace: String, chatWorkspaceId: String,
                                   channelId: String, invocationId: String,
                                   messageId: String) throws -> String {
        try chatString { loom_chat_agent_reply_json(session, workspace, chatWorkspaceId, channelId, invocationId, messageId, $0) }
    }

    public func chatRequestHandoffJson(workspace: String, chatWorkspaceId: String,
                                       channelId: String, handoffId: String,
                                       fromAgentPrincipal: String, toPrincipal: String?,
                                       reason: String?) throws -> String {
        try chatString {
            loom_chat_request_handoff_json(
                session, workspace, chatWorkspaceId, channelId, handoffId,
                fromAgentPrincipal, toPrincipal ?? "", reason ?? "", $0
            )
        }
    }

    public func chatAddReactionJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, messageId: String,
                                    kind: String) throws -> String {
        try chatString { loom_chat_add_reaction_json(session, workspace, chatWorkspaceId, channelId, messageId, kind, $0) }
    }

    public func chatRemoveReactionJson(workspace: String, chatWorkspaceId: String,
                                       channelId: String, messageId: String,
                                       kind: String) throws -> String {
        try chatString { loom_chat_remove_reaction_json(session, workspace, chatWorkspaceId, channelId, messageId, kind, $0) }
    }

    public func chatEmojiListJson(workspace: String, chatWorkspaceId: String) throws -> String {
        try chatString { loom_chat_emoji_list_json(session, workspace, chatWorkspaceId, $0) }
    }

    public func chatEmojiRegisterJson(workspace: String, chatWorkspaceId: String,
                                      kind: String) throws -> String {
        try chatString { loom_chat_emoji_register_json(session, workspace, chatWorkspaceId, kind, $0) }
    }

    public func chatEmojiUnregisterJson(workspace: String, chatWorkspaceId: String,
                                        kind: String) throws -> String {
        try chatString { loom_chat_emoji_unregister_json(session, workspace, chatWorkspaceId, kind, $0) }
    }

    public func chatMessagesJson(workspace: String, chatWorkspaceId: String,
                                 channelId: String) throws -> String {
        try chatString { loom_chat_messages_json(session, workspace, chatWorkspaceId, channelId, $0) }
    }

    public func chatCursorJson(workspace: String, chatWorkspaceId: String,
                               channelId: String) throws -> String {
        try chatString { loom_chat_cursor_json(session, workspace, chatWorkspaceId, channelId, $0) }
    }

    public func chatUpdateCursorJson(workspace: String, chatWorkspaceId: String,
                                     channelId: String, nextSequence: UInt64) throws -> String {
        try chatString { loom_chat_update_cursor_json(session, workspace, chatWorkspaceId, channelId, nextSequence, $0) }
    }

    public func chatFetchEventsJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, fromSequence: UInt64,
                                    max: Int) throws -> String {
        try chatString { loom_chat_fetch_events_json(session, workspace, chatWorkspaceId, channelId, fromSequence, UInt(max), $0) }
    }
}
