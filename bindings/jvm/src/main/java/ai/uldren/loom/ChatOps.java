package ai.uldren.loom;

import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;

public final class ChatOps {
    private static final MethodHandle CREATE_CHANNEL_JSON = down("loom_chat_create_channel_json",
            descriptor(6));
    private static final MethodHandle RENAME_CHANNEL_JSON = down("loom_chat_rename_channel_json",
            descriptor(5));
    private static final MethodHandle LIST_CHANNELS_JSON = down("loom_chat_list_channels_json",
            descriptor(3));
    private static final MethodHandle POST_MESSAGE_JSON = down("loom_chat_post_message_json",
            descriptor(7));
    private static final MethodHandle EDIT_MESSAGE_JSON = down("loom_chat_edit_message_json",
            descriptor(6));
    private static final MethodHandle REDACT_MESSAGE_JSON = down("loom_chat_redact_message_json",
            descriptor(6));
    private static final MethodHandle CREATE_THREAD_JSON = down("loom_chat_create_thread_json",
            descriptor(6));
    private static final MethodHandle CREATE_TASK_JSON = down("loom_chat_create_task_json",
            descriptor(7));
    private static final MethodHandle CLAIM_TASK_JSON = down("loom_chat_claim_task_json",
            descriptor(7));
    private static final MethodHandle COMPLETE_TASK_JSON = down("loom_chat_complete_task_json",
            descriptor(7));
    private static final MethodHandle INVOKE_AGENT_JSON = down("loom_chat_invoke_agent_json",
            descriptor(8));
    private static final MethodHandle AGENT_REPLY_JSON = down("loom_chat_agent_reply_json",
            descriptor(6));
    private static final MethodHandle REQUEST_HANDOFF_JSON = down("loom_chat_request_handoff_json",
            descriptor(9));
    private static final MethodHandle ADD_REACTION_JSON = down("loom_chat_add_reaction_json",
            descriptor(6));
    private static final MethodHandle REMOVE_REACTION_JSON = down("loom_chat_remove_reaction_json",
            descriptor(6));
    private static final MethodHandle EMOJI_LIST_JSON = down("loom_chat_emoji_list_json",
            descriptor(3));
    private static final MethodHandle EMOJI_REGISTER_JSON = down("loom_chat_emoji_register_json",
            descriptor(4));
    private static final MethodHandle EMOJI_UNREGISTER_JSON = down("loom_chat_emoji_unregister_json",
            descriptor(4));
    private static final MethodHandle MESSAGES_JSON = down("loom_chat_messages_json",
            descriptor(4));
    private static final MethodHandle CURSOR_JSON = down("loom_chat_cursor_json",
            descriptor(4));
    private static final MethodHandle UPDATE_CURSOR_JSON = down("loom_chat_update_cursor_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));
    private static final MethodHandle FETCH_EVENTS_JSON = down("loom_chat_fetch_events_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    private final LoomSession session;

    ChatOps(LoomSession session) {
        this.session = session;
    }

    private static FunctionDescriptor descriptor(int addressArgsAfterHandle) {
        ValueLayout[] layouts = new ValueLayout[addressArgsAfterHandle + 1];
        layouts[0] = ValueLayout.ADDRESS;
        for (int i = 1; i < layouts.length; i++) {
            layouts[i] = ValueLayout.ADDRESS;
        }
        return FunctionDescriptor.of(ValueLayout.JAVA_INT, layouts);
    }

    private static MethodHandle down(String symbol, FunctionDescriptor descriptor) {
        return Loom.LINKER.downcallHandle(Loom.LOOKUP.find(symbol).orElseThrow(), descriptor);
    }

    public String createChannelJson(String workspace, String chatWorkspaceId, String channelId,
            String channelHandle, String name) {
        return string5("loom_chat_create_channel_json", CREATE_CHANNEL_JSON, workspace,
                chatWorkspaceId, channelId, channelHandle, name);
    }

    public String renameChannelJson(String workspace, String chatWorkspaceId, String selector,
            String channelHandle) {
        return string4("loom_chat_rename_channel_json", RENAME_CHANNEL_JSON, workspace,
                chatWorkspaceId, selector, channelHandle);
    }

    public String listChannelsJson(String workspace, String chatWorkspaceId) {
        return string2("loom_chat_list_channels_json", LIST_CHANNELS_JSON, workspace,
                chatWorkspaceId);
    }

    public String postMessageJson(String workspace, String chatWorkspaceId, String channelId,
            String messageId, String threadId, String bodyText) {
        return session.onHandle("loom_chat_post_message_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) POST_MESSAGE_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(chatWorkspaceId),
                    arena.allocateFrom(channelId), arena.allocateFrom(messageId),
                    nullable(arena, threadId), arena.allocateFrom(bodyText), out);
            return takeString("loom_chat_post_message_json", status, out);
        });
    }

    public String editMessageJson(String workspace, String chatWorkspaceId, String channelId,
            String messageId, String bodyText) {
        return session.onHandle("loom_chat_edit_message_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) EDIT_MESSAGE_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(chatWorkspaceId),
                    arena.allocateFrom(channelId), arena.allocateFrom(messageId),
                    arena.allocateFrom(bodyText), out);
            return takeString("loom_chat_edit_message_json", status, out);
        });
    }

    public String redactMessageJson(String workspace, String chatWorkspaceId, String channelId,
            String messageId, String reason) {
        return string5("loom_chat_redact_message_json", REDACT_MESSAGE_JSON, workspace,
                chatWorkspaceId, channelId, messageId, nullableString(reason));
    }

    public String createThreadJson(String workspace, String chatWorkspaceId, String channelId,
            String threadId, String parentMessageId) {
        return string5("loom_chat_create_thread_json", CREATE_THREAD_JSON, workspace,
                chatWorkspaceId, channelId, threadId, parentMessageId);
    }

    public String createTaskJson(String workspace, String chatWorkspaceId, String channelId,
            String taskId, String messageId, String title) {
        return string6("loom_chat_create_task_json", CREATE_TASK_JSON, workspace,
                chatWorkspaceId, channelId, taskId, messageId, title);
    }

    public String claimTaskJson(String workspace, String chatWorkspaceId, String channelId,
            String taskId, String claimId, String leaseToken) {
        return string6("loom_chat_claim_task_json", CLAIM_TASK_JSON, workspace, chatWorkspaceId,
                channelId, taskId, claimId, nullableString(leaseToken));
    }

    public String completeTaskJson(String workspace, String chatWorkspaceId, String channelId,
            String taskId, String claimId, String resultMessageId) {
        return string6("loom_chat_complete_task_json", COMPLETE_TASK_JSON, workspace,
                chatWorkspaceId, channelId, taskId, claimId, nullableString(resultMessageId));
    }

    public String invokeAgentJson(String workspace, String chatWorkspaceId, String channelId,
            String invocationId, String agentPrincipal, String sourceMessageIdsJson,
            String promptText) {
        return session.onHandle("loom_chat_invoke_agent_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) INVOKE_AGENT_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(chatWorkspaceId),
                    arena.allocateFrom(channelId), arena.allocateFrom(invocationId),
                    arena.allocateFrom(agentPrincipal), arena.allocateFrom(sourceMessageIdsJson),
                    arena.allocateFrom(promptText), out);
            return takeString("loom_chat_invoke_agent_json", status, out);
        });
    }

    public String agentReplyJson(String workspace, String chatWorkspaceId, String channelId,
            String invocationId, String messageId) {
        return string5("loom_chat_agent_reply_json", AGENT_REPLY_JSON, workspace,
                chatWorkspaceId, channelId, invocationId, messageId);
    }

    public String requestHandoffJson(String workspace, String chatWorkspaceId, String channelId,
            String handoffId, String fromAgentPrincipal, String toPrincipal, String reason) {
        return string7("loom_chat_request_handoff_json", REQUEST_HANDOFF_JSON, workspace,
                chatWorkspaceId, channelId, handoffId, fromAgentPrincipal,
                nullableString(toPrincipal), nullableString(reason));
    }

    public String addReactionJson(String workspace, String chatWorkspaceId, String channelId,
            String messageId, String kind) {
        return string5("loom_chat_add_reaction_json", ADD_REACTION_JSON, workspace,
                chatWorkspaceId, channelId, messageId, kind);
    }

    public String removeReactionJson(String workspace, String chatWorkspaceId, String channelId,
            String messageId, String kind) {
        return string5("loom_chat_remove_reaction_json", REMOVE_REACTION_JSON, workspace,
                chatWorkspaceId, channelId, messageId, kind);
    }

    public String emojiListJson(String workspace, String chatWorkspaceId) {
        return string2("loom_chat_emoji_list_json", EMOJI_LIST_JSON, workspace,
                chatWorkspaceId);
    }

    public String emojiRegisterJson(String workspace, String chatWorkspaceId, String kind) {
        return string3("loom_chat_emoji_register_json", EMOJI_REGISTER_JSON, workspace,
                chatWorkspaceId, kind);
    }

    public String emojiUnregisterJson(String workspace, String chatWorkspaceId, String kind) {
        return string3("loom_chat_emoji_unregister_json", EMOJI_UNREGISTER_JSON, workspace,
                chatWorkspaceId, kind);
    }

    public String messagesJson(String workspace, String chatWorkspaceId, String channelId) {
        return string3("loom_chat_messages_json", MESSAGES_JSON, workspace, chatWorkspaceId,
                channelId);
    }

    public String cursorJson(String workspace, String chatWorkspaceId, String channelId) {
        return string3("loom_chat_cursor_json", CURSOR_JSON, workspace, chatWorkspaceId,
                channelId);
    }

    public String updateCursorJson(String workspace, String chatWorkspaceId, String channelId,
            long nextSequence) {
        return session.onHandle("loom_chat_update_cursor_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) UPDATE_CURSOR_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(chatWorkspaceId), arena.allocateFrom(channelId),
                    nextSequence, out);
            return takeString("loom_chat_update_cursor_json", status, out);
        });
    }

    public String fetchEventsJson(String workspace, String chatWorkspaceId, String channelId,
            long fromSequence, long max) {
        return session.onHandle("loom_chat_fetch_events_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) FETCH_EVENTS_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(chatWorkspaceId), arena.allocateFrom(channelId),
                    fromSequence, max, out);
            return takeString("loom_chat_fetch_events_json", status, out);
        });
    }

    private String string2(String symbol, MethodHandle method, String a, String b) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(a),
                    arena.allocateFrom(b), out);
            return takeString(symbol, status, out);
        });
    }

    private String string3(String symbol, MethodHandle method, String a, String b, String c) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(a),
                    arena.allocateFrom(b), arena.allocateFrom(c), out);
            return takeString(symbol, status, out);
        });
    }

    private String string4(String symbol, MethodHandle method, String a, String b, String c,
            String d) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(a),
                    arena.allocateFrom(b), arena.allocateFrom(c), arena.allocateFrom(d), out);
            return takeString(symbol, status, out);
        });
    }

    private String string5(String symbol, MethodHandle method, String a, String b, String c,
            String d, String e) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(a),
                    arena.allocateFrom(b), arena.allocateFrom(c), arena.allocateFrom(d),
                    nullable(arena, e), out);
            return takeString(symbol, status, out);
        });
    }

    private String string6(String symbol, MethodHandle method, String a, String b, String c,
            String d, String e, String f) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(a),
                    arena.allocateFrom(b), arena.allocateFrom(c), arena.allocateFrom(d),
                    arena.allocateFrom(e), nullable(arena, f), out);
            return takeString(symbol, status, out);
        });
    }

    private String string7(String symbol, MethodHandle method, String a, String b, String c,
            String d, String e, String f, String g) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(a),
                    arena.allocateFrom(b), arena.allocateFrom(c), arena.allocateFrom(d),
                    arena.allocateFrom(e), nullable(arena, f), nullable(arena, g), out);
            return takeString(symbol, status, out);
        });
    }

    private static MemorySegment nullable(java.lang.foreign.Arena arena, String value) {
        return value != null && !value.isEmpty() ? arena.allocateFrom(value) : MemorySegment.NULL;
    }

    private static String nullableString(String value) {
        return value != null ? value : "";
    }

    private static String takeString(String symbol, int status, MemorySegment out) throws Throwable {
        if (status != 0) {
            throw Loom.lastError(symbol);
        }
        return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
    }
}
