#include "UldrenLoom_jni.h"

static jstring finishChatString(JNIEnv *env, LoomSession *h, int32_t st, char *out) {
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

#define CHAT_OPEN() \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { throwLoom(env); return nullptr; } \
  const char *n = env->GetStringUTFChars(ns, nullptr); \
  const char *cw = env->GetStringUTFChars(chatWorkspaceId, nullptr)

#define CHAT_RELEASE_NS() \
  env->ReleaseStringUTFChars(ns, n); \
  env->ReleaseStringUTFChars(chatWorkspaceId, cw)

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatCreateChannelJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring channelHandle, jstring name, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *handle = env->GetStringUTFChars(channelHandle, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  st = loom_chat_create_channel_json(h, n, cw, channel, handle, nm, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(channelHandle, handle);
  env->ReleaseStringUTFChars(name, nm);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatRenameChannelJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring selector, jstring channelHandle, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *sel = env->GetStringUTFChars(selector, nullptr);
  const char *handle = env->GetStringUTFChars(channelHandle, nullptr);
  char *out = nullptr;
  st = loom_chat_rename_channel_json(h, n, cw, sel, handle, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(selector, sel);
  env->ReleaseStringUTFChars(channelHandle, handle);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatListChannelsJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  char *out = nullptr;
  st = loom_chat_list_channels_json(h, n, cw, &out);
  CHAT_RELEASE_NS();
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatPostMessageJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring messageId, jstring threadId, jstring bodyText,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *message = env->GetStringUTFChars(messageId, nullptr);
  const char *thread = threadId ? env->GetStringUTFChars(threadId, nullptr) : nullptr;
  const char *body = env->GetStringUTFChars(bodyText, nullptr);
  char *out = nullptr;
  st = loom_chat_post_message_json(h, n, cw, channel, message, thread, body, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(messageId, message);
  if (thread) env->ReleaseStringUTFChars(threadId, thread);
  env->ReleaseStringUTFChars(bodyText, body);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatEditMessageJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring messageId, jstring bodyText, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *message = env->GetStringUTFChars(messageId, nullptr);
  const char *body = env->GetStringUTFChars(bodyText, nullptr);
  char *out = nullptr;
  st = loom_chat_edit_message_json(h, n, cw, channel, message, body, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(messageId, message);
  env->ReleaseStringUTFChars(bodyText, body);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatRedactMessageJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring messageId, jstring reason, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *message = env->GetStringUTFChars(messageId, nullptr);
  const char *why = reason ? env->GetStringUTFChars(reason, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_chat_redact_message_json(h, n, cw, channel, message, why, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(messageId, message);
  if (why) env->ReleaseStringUTFChars(reason, why);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatCreateThreadJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring threadId, jstring parentMessageId, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *thread = env->GetStringUTFChars(threadId, nullptr);
  const char *parent = env->GetStringUTFChars(parentMessageId, nullptr);
  char *out = nullptr;
  st = loom_chat_create_thread_json(h, n, cw, channel, thread, parent, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(threadId, thread);
  env->ReleaseStringUTFChars(parentMessageId, parent);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatCreateTaskJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring taskId, jstring messageId, jstring title,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *task = env->GetStringUTFChars(taskId, nullptr);
  const char *message = env->GetStringUTFChars(messageId, nullptr);
  const char *ttl = env->GetStringUTFChars(title, nullptr);
  char *out = nullptr;
  st = loom_chat_create_task_json(h, n, cw, channel, task, message, ttl, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(taskId, task);
  env->ReleaseStringUTFChars(messageId, message);
  env->ReleaseStringUTFChars(title, ttl);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatClaimTaskJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring taskId, jstring claimId, jstring leaseToken,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *task = env->GetStringUTFChars(taskId, nullptr);
  const char *claim = env->GetStringUTFChars(claimId, nullptr);
  const char *lease = leaseToken ? env->GetStringUTFChars(leaseToken, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_chat_claim_task_json(h, n, cw, channel, task, claim, lease, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(taskId, task);
  env->ReleaseStringUTFChars(claimId, claim);
  if (lease) env->ReleaseStringUTFChars(leaseToken, lease);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatCompleteTaskJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring taskId, jstring claimId, jstring resultMessageId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *task = env->GetStringUTFChars(taskId, nullptr);
  const char *claim = env->GetStringUTFChars(claimId, nullptr);
  const char *result = resultMessageId ? env->GetStringUTFChars(resultMessageId, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_chat_complete_task_json(h, n, cw, channel, task, claim, result, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(taskId, task);
  env->ReleaseStringUTFChars(claimId, claim);
  if (result) env->ReleaseStringUTFChars(resultMessageId, result);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatInvokeAgentJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring invocationId, jstring agentPrincipal,
    jstring sourceMessageIdsJson, jstring promptText, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *invocation = env->GetStringUTFChars(invocationId, nullptr);
  const char *agent = env->GetStringUTFChars(agentPrincipal, nullptr);
  const char *sources = env->GetStringUTFChars(sourceMessageIdsJson, nullptr);
  const char *prompt = env->GetStringUTFChars(promptText, nullptr);
  char *out = nullptr;
  st = loom_chat_invoke_agent_json(h, n, cw, channel, invocation, agent, sources, prompt, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(invocationId, invocation);
  env->ReleaseStringUTFChars(agentPrincipal, agent);
  env->ReleaseStringUTFChars(sourceMessageIdsJson, sources);
  env->ReleaseStringUTFChars(promptText, prompt);
  return finishChatString(env, h, st, out);
}

#define CHAT_STRING5_FUNC(name, c_name) \
extern "C" JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId, \
    jstring channelId, jstring a, jstring b, jbyteArray passphrase, jbyteArray kek, \
    jstring authPrincipal, jbyteArray authPassphrase) { \
  (void)thiz; \
  CHAT_OPEN(); \
  const char *channel = env->GetStringUTFChars(channelId, nullptr); \
  const char *av = env->GetStringUTFChars(a, nullptr); \
  const char *bv = env->GetStringUTFChars(b, nullptr); \
  char *out = nullptr; \
  st = c_name(h, n, cw, channel, av, bv, &out); \
  CHAT_RELEASE_NS(); \
  env->ReleaseStringUTFChars(channelId, channel); \
  env->ReleaseStringUTFChars(a, av); \
  env->ReleaseStringUTFChars(b, bv); \
  return finishChatString(env, h, st, out); \
}

CHAT_STRING5_FUNC(nativeChatAgentReplyJson, loom_chat_agent_reply_json)
CHAT_STRING5_FUNC(nativeChatAddReactionJson, loom_chat_add_reaction_json)
CHAT_STRING5_FUNC(nativeChatRemoveReactionJson, loom_chat_remove_reaction_json)

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatRequestHandoffJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring handoffId, jstring fromAgentPrincipal, jstring toPrincipal,
    jstring reason, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  const char *handoff = env->GetStringUTFChars(handoffId, nullptr);
  const char *from = env->GetStringUTFChars(fromAgentPrincipal, nullptr);
  const char *to = toPrincipal ? env->GetStringUTFChars(toPrincipal, nullptr) : nullptr;
  const char *why = reason ? env->GetStringUTFChars(reason, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_chat_request_handoff_json(h, n, cw, channel, handoff, from, to, why, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  env->ReleaseStringUTFChars(handoffId, handoff);
  env->ReleaseStringUTFChars(fromAgentPrincipal, from);
  if (to) env->ReleaseStringUTFChars(toPrincipal, to);
  if (why) env->ReleaseStringUTFChars(reason, why);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatEmojiListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  char *out = nullptr;
  st = loom_chat_emoji_list_json(h, n, cw, &out);
  CHAT_RELEASE_NS();
  return finishChatString(env, h, st, out);
}

#define CHAT_KIND_FUNC(name, c_name) \
extern "C" JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId, \
    jstring kind, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, \
    jbyteArray authPassphrase) { \
  (void)thiz; \
  CHAT_OPEN(); \
  const char *kd = env->GetStringUTFChars(kind, nullptr); \
  char *out = nullptr; \
  st = c_name(h, n, cw, kd, &out); \
  CHAT_RELEASE_NS(); \
  env->ReleaseStringUTFChars(kind, kd); \
  return finishChatString(env, h, st, out); \
}

CHAT_KIND_FUNC(nativeChatEmojiRegisterJson, loom_chat_emoji_register_json)
CHAT_KIND_FUNC(nativeChatEmojiUnregisterJson, loom_chat_emoji_unregister_json)

#define CHAT_CHANNEL_FUNC(name, c_name) \
extern "C" JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId, \
    jstring channelId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, \
    jbyteArray authPassphrase) { \
  (void)thiz; \
  CHAT_OPEN(); \
  const char *channel = env->GetStringUTFChars(channelId, nullptr); \
  char *out = nullptr; \
  st = c_name(h, n, cw, channel, &out); \
  CHAT_RELEASE_NS(); \
  env->ReleaseStringUTFChars(channelId, channel); \
  return finishChatString(env, h, st, out); \
}

CHAT_CHANNEL_FUNC(nativeChatMessagesJson, loom_chat_messages_json)
CHAT_CHANNEL_FUNC(nativeChatCursorJson, loom_chat_cursor_json)

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatUpdateCursorJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring nextSequence, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  uint64_t next = 0;
  if (!parseU64String(env, nextSequence, &next)) {
    CHAT_RELEASE_NS();
    env->ReleaseStringUTFChars(channelId, channel);
    loom_close(h);
    return nullptr;
  }
  char *out = nullptr;
  st = loom_chat_update_cursor_json(h, n, cw, channel, next, &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  return finishChatString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeChatFetchEventsJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring chatWorkspaceId,
    jstring channelId, jstring fromSequence, jstring max, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  CHAT_OPEN();
  const char *channel = env->GetStringUTFChars(channelId, nullptr);
  uint64_t from = 0;
  uint64_t limit = 0;
  if (!parseU64String(env, fromSequence, &from) || !parseU64String(env, max, &limit)) {
    CHAT_RELEASE_NS();
    env->ReleaseStringUTFChars(channelId, channel);
    loom_close(h);
    return nullptr;
  }
  char *out = nullptr;
  st = loom_chat_fetch_events_json(h, n, cw, channel, from, static_cast<uintptr_t>(limit), &out);
  CHAT_RELEASE_NS();
  env->ReleaseStringUTFChars(channelId, channel);
  return finishChatString(env, h, st, out);
}

#undef CHAT_STRING5_FUNC
#undef CHAT_KIND_FUNC
#undef CHAT_CHANNEL_FUNC
#undef CHAT_OPEN
#undef CHAT_RELEASE_NS
