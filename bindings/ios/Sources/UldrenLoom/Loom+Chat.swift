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
                                      name: String, expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_create_channel_json(session, workspace, chatWorkspaceId, channelId, channelHandle, name, expectedEntityTag, $0) }
    }

    public func chatRenameChannelJson(workspace: String, chatWorkspaceId: String,
                                      selector: String, channelHandle: String,
                                      expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_rename_channel_json(session, workspace, chatWorkspaceId, selector, channelHandle, expectedEntityTag, $0) }
    }

    public func chatListChannelsJson(workspace: String, chatWorkspaceId: String) throws -> String {
        try chatString { loom_chat_list_channels_json(session, workspace, chatWorkspaceId, $0) }
    }

    public func chatPostMessageJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, messageId: String, threadId: String?,
                                    bodyText: String, expectedEntityTag: String? = nil) throws -> String {
        try chatString {
            loom_chat_post_message_json(
                session, workspace, chatWorkspaceId, channelId, messageId, threadId,
                bodyText, expectedEntityTag, $0
            )
        }
    }

    public func chatPostMessageBytesJson(workspace: String, chatWorkspaceId: String,
                                         channelId: String, messageId: String, threadId: String?,
                                         body: [UInt8], expectedEntityTag: String? = nil) throws -> String {
        var bytes = body
        return try bytes.withUnsafeMutableBytes { buf in
            try chatString {
                loom_chat_post_message_bytes_json(
                    session, workspace, chatWorkspaceId, channelId, messageId, threadId,
                    buf.baseAddress?.assumingMemoryBound(to: UInt8.self), UInt(buf.count),
                    expectedEntityTag, $0
                )
            }
        }
    }

    public func chatEditMessageJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, messageId: String,
                                    bodyText: String, expectedEntityTag: String? = nil) throws -> String {
        try chatString {
            loom_chat_edit_message_json(
                session, workspace, chatWorkspaceId, channelId, messageId,
                bodyText, expectedEntityTag, $0
            )
        }
    }

    public func chatEditMessageBytesJson(workspace: String, chatWorkspaceId: String,
                                         channelId: String, messageId: String,
                                         body: [UInt8], expectedEntityTag: String? = nil) throws -> String {
        var bytes = body
        return try bytes.withUnsafeMutableBytes { buf in
            try chatString {
                loom_chat_edit_message_bytes_json(
                    session, workspace, chatWorkspaceId, channelId, messageId,
                    buf.baseAddress?.assumingMemoryBound(to: UInt8.self), UInt(buf.count),
                    expectedEntityTag, $0
                )
            }
        }
    }

    public func chatRedactMessageJson(workspace: String, chatWorkspaceId: String,
                                      channelId: String, messageId: String,
                                      reason: String?, expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_redact_message_json(session, workspace, chatWorkspaceId, channelId, messageId, reason, expectedEntityTag, $0) }
    }

    public func chatCreateThreadJson(workspace: String, chatWorkspaceId: String,
                                     channelId: String, threadId: String,
                                     parentMessageId: String,
                                     expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_create_thread_json(session, workspace, chatWorkspaceId, channelId, threadId, parentMessageId, expectedEntityTag, $0) }
    }

    public func chatCreateTaskJson(workspace: String, chatWorkspaceId: String,
                                   channelId: String, taskId: String,
                                   messageId: String, title: String,
                                   expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_create_task_json(session, workspace, chatWorkspaceId, channelId, taskId, messageId, title, expectedEntityTag, $0) }
    }

    public func chatClaimTaskJson(workspace: String, chatWorkspaceId: String,
                                  channelId: String, taskId: String,
                                  claimId: String, leaseToken: String?,
                                  expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_claim_task_json(session, workspace, chatWorkspaceId, channelId, taskId, claimId, leaseToken, expectedEntityTag, $0) }
    }

    public func chatCompleteTaskJson(workspace: String, chatWorkspaceId: String,
                                     channelId: String, taskId: String, claimId: String,
                                     resultMessageId: String?,
                                     expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_complete_task_json(session, workspace, chatWorkspaceId, channelId, taskId, claimId, resultMessageId, expectedEntityTag, $0) }
    }

    public func chatInvokeAgentJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, invocationId: String,
                                    agentPrincipal: String, sourceMessageIdsJson: String,
                                    promptText: String,
                                    expectedEntityTag: String? = nil) throws -> String {
        try chatString {
            loom_chat_invoke_agent_json(
                session, workspace, chatWorkspaceId, channelId, invocationId,
                agentPrincipal, sourceMessageIdsJson, promptText, expectedEntityTag, $0
            )
        }
    }

    public func chatInvokeAgentBytesJson(workspace: String, chatWorkspaceId: String,
                                         channelId: String, invocationId: String,
                                         agentPrincipal: String, sourceMessageIdsJson: String,
                                         prompt: [UInt8],
                                         expectedEntityTag: String? = nil) throws -> String {
        var bytes = prompt
        return try bytes.withUnsafeMutableBytes { buf in
            try chatString {
                loom_chat_invoke_agent_bytes_json(
                    session, workspace, chatWorkspaceId, channelId, invocationId,
                    agentPrincipal, sourceMessageIdsJson,
                    buf.baseAddress?.assumingMemoryBound(to: UInt8.self), UInt(buf.count),
                    expectedEntityTag, $0
                )
            }
        }
    }

    public func chatAgentReplyJson(workspace: String, chatWorkspaceId: String,
                                   channelId: String, invocationId: String,
                                   messageId: String,
                                   expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_agent_reply_json(session, workspace, chatWorkspaceId, channelId, invocationId, messageId, expectedEntityTag, $0) }
    }

    public func chatRequestHandoffJson(workspace: String, chatWorkspaceId: String,
                                       channelId: String, handoffId: String,
                                       fromAgentPrincipal: String, toPrincipal: String?,
                                       reason: String?,
                                       expectedEntityTag: String? = nil) throws -> String {
        try chatString {
            loom_chat_request_handoff_json(
                session, workspace, chatWorkspaceId, channelId, handoffId,
                fromAgentPrincipal, toPrincipal, reason, expectedEntityTag, $0
            )
        }
    }

    public func chatAddReactionJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, messageId: String,
                                    kind: String,
                                    expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_add_reaction_json(session, workspace, chatWorkspaceId, channelId, messageId, kind, expectedEntityTag, $0) }
    }

    public func chatRemoveReactionJson(workspace: String, chatWorkspaceId: String,
                                       channelId: String, messageId: String,
                                       kind: String,
                                       expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_remove_reaction_json(session, workspace, chatWorkspaceId, channelId, messageId, kind, expectedEntityTag, $0) }
    }

    public func chatEmojiListJson(workspace: String, chatWorkspaceId: String) throws -> String {
        try chatString { loom_chat_emoji_list_json(session, workspace, chatWorkspaceId, $0) }
    }

    public func chatEmojiRegisterJson(workspace: String, chatWorkspaceId: String,
                                      kind: String,
                                      expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_emoji_register_json(session, workspace, chatWorkspaceId, kind, expectedEntityTag, $0) }
    }

    public func chatEmojiUnregisterJson(workspace: String, chatWorkspaceId: String,
                                        kind: String,
                                        expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_emoji_unregister_json(session, workspace, chatWorkspaceId, kind, expectedEntityTag, $0) }
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
                                     channelId: String, nextSequence: UInt64,
                                     expectedEntityTag: String? = nil) throws -> String {
        try chatString { loom_chat_update_cursor_json(session, workspace, chatWorkspaceId, channelId, nextSequence, expectedEntityTag, $0) }
    }

    public func chatFetchEventsJson(workspace: String, chatWorkspaceId: String,
                                    channelId: String, fromSequence: UInt64,
                                    max: UInt64) throws -> String {
        try chatString { loom_chat_fetch_events_json(session, workspace, chatWorkspaceId, channelId, fromSequence, max, $0) }
    }
}
