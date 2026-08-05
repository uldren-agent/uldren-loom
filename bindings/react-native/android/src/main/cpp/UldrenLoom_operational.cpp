#include "UldrenLoom_jni.h"

static jstring finishRnStoreString(JNIEnv *env, LoomSession *h, int32_t st, char *out) {
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

static jbyteArray finishRnStoreBytes(JNIEnv *env, LoomSession *h, int32_t st, unsigned char *ptr,
                                     uintptr_t len) {
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStudioReindexJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring profile,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *profileChars = env->GetStringUTFChars(profile, nullptr);
  char *out = nullptr;
  st = loom_studio_reindex_json(h, workspaceChars, profileChars, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(profile, profileChars);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStudioRevisionsRebuildJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring profile, jboolean dryRun,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *profileChars = env->GetStringUTFChars(profile, nullptr);
  char *out = nullptr;
  st = loom_studio_revisions_rebuild_json(h, workspaceChars, profileChars, dryRun ? 1 : 0, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(profile, profileChars);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStoreBundleImport(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray bundle, jboolean dryRun,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jsize bundleLen = env->GetArrayLength(bundle);
  jbyte *bundleBytes = env->GetByteArrayElements(bundle, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_store_bundle_import(h, reinterpret_cast<const unsigned char *>(bundleBytes),
                                static_cast<uintptr_t>(bundleLen), dryRun ? 1 : 0, &ptr, &len);
  env->ReleaseByteArrayElements(bundle, bundleBytes, JNI_ABORT);
  return finishRnStoreBytes(env, h, st, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAuditCompact(
    JNIEnv *env, jobject thiz, jstring loomPath, jlong throughSeq, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  if (throughSeq < 0) {
    jclass ex = env->FindClass("java/lang/IllegalArgumentException");
    env->ThrowNew(ex, "throughSeq must be non-negative");
    return nullptr;
  }
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_audit_compact(h, static_cast<uint64_t>(throughSeq), &ptr, &len);
  return finishRnStoreBytes(env, h, st, ptr, len);
}

#define RN_MAINTENANCE_BYTES(java_name, c_name, arg_name) \
extern "C" JNIEXPORT jbyteArray JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##java_name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray arg_name, jbyteArray passphrase, \
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) { \
  (void)thiz; \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { \
    throwLoom(env); \
    return nullptr; \
  } \
  jsize inputLen = env->GetArrayLength(arg_name); \
  jbyte *inputBytes = env->GetByteArrayElements(arg_name, nullptr); \
  unsigned char *ptr = nullptr; \
  uintptr_t len = 0; \
  st = c_name(h, reinterpret_cast<const unsigned char *>(inputBytes), \
              static_cast<uintptr_t>(inputLen), &ptr, &len); \
  env->ReleaseByteArrayElements(arg_name, inputBytes, JNI_ABORT); \
  return finishRnStoreBytes(env, h, st, ptr, len); \
}

RN_MAINTENANCE_BYTES(nativeStoreMaintenanceStatus, loom_store_maintenance_status, request)
RN_MAINTENANCE_BYTES(nativeStoreMaintenancePolicySet, loom_store_maintenance_policy_set, update)
RN_MAINTENANCE_BYTES(nativeStoreMaintenanceRun, loom_store_maintenance_run, request)

#undef RN_MAINTENANCE_BYTES

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeImportTableCsv(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring sourceScope,
    jbyteArray csvPayload, jstring database, jstring table, jstring schema, jstring primaryKey,
    jstring mode, jboolean commit, jstring author, jstring message, jboolean dryRun,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *scopeChars = env->GetStringUTFChars(sourceScope, nullptr);
  jsize payloadLen = env->GetArrayLength(csvPayload);
  jbyte *payloadBytes = env->GetByteArrayElements(csvPayload, nullptr);
  const char *databaseChars = env->GetStringUTFChars(database, nullptr);
  const char *tableChars = env->GetStringUTFChars(table, nullptr);
  const char *schemaChars = env->GetStringUTFChars(schema, nullptr);
  const char *primaryKeyChars = env->GetStringUTFChars(primaryKey, nullptr);
  const char *modeChars = env->GetStringUTFChars(mode, nullptr);
  const char *authorValue = author ? env->GetStringUTFChars(author, nullptr) : nullptr;
  const char *messageValue = message ? env->GetStringUTFChars(message, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_import_table_csv(
      h, workspaceChars, scopeChars, reinterpret_cast<const unsigned char *>(payloadBytes),
      static_cast<uintptr_t>(payloadLen), databaseChars, tableChars, schemaChars,
      primaryKeyChars, modeChars, commit ? 1 : 0, authorValue, messageValue, dryRun ? 1 : 0,
      &ptr, &len);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(sourceScope, scopeChars);
  env->ReleaseByteArrayElements(csvPayload, payloadBytes, JNI_ABORT);
  env->ReleaseStringUTFChars(database, databaseChars);
  env->ReleaseStringUTFChars(table, tableChars);
  env->ReleaseStringUTFChars(schema, schemaChars);
  env->ReleaseStringUTFChars(primaryKey, primaryKeyChars);
  env->ReleaseStringUTFChars(mode, modeChars);
  if (authorValue) {
    env->ReleaseStringUTFChars(author, authorValue);
  }
  if (messageValue) {
    env->ReleaseStringUTFChars(message, messageValue);
  }
  return finishRnStoreBytes(env, h, st, ptr, len);
}

#define RN_TICKET_IMPORT(java_name, c_name) \
extern "C" JNIEXPORT jbyteArray JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##java_name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring profile, \
    jstring sourceScope, jbyteArray snapshotPayload, jstring fieldPolicy, jboolean dryRun, \
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) { \
  (void)thiz; \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { \
    throwLoom(env); \
    return nullptr; \
  } \
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr); \
  const char *profileChars = env->GetStringUTFChars(profile, nullptr); \
  const char *scopeChars = env->GetStringUTFChars(sourceScope, nullptr); \
  jsize payloadLen = env->GetArrayLength(snapshotPayload); \
  jbyte *payloadBytes = env->GetByteArrayElements(snapshotPayload, nullptr); \
  const char *policyChars = env->GetStringUTFChars(fieldPolicy, nullptr); \
  unsigned char *ptr = nullptr; \
  uintptr_t len = 0; \
  st = c_name(h, workspaceChars, profileChars, scopeChars, \
              reinterpret_cast<const unsigned char *>(payloadBytes), \
              static_cast<uintptr_t>(payloadLen), policyChars, dryRun ? 1 : 0, &ptr, &len); \
  env->ReleaseStringUTFChars(workspace, workspaceChars); \
  env->ReleaseStringUTFChars(profile, profileChars); \
  env->ReleaseStringUTFChars(sourceScope, scopeChars); \
  env->ReleaseByteArrayElements(snapshotPayload, payloadBytes, JNI_ABORT); \
  env->ReleaseStringUTFChars(fieldPolicy, policyChars); \
  return finishRnStoreBytes(env, h, st, ptr, len); \
}

RN_TICKET_IMPORT(nativeImportRedmine, loom_import_redmine)
RN_TICKET_IMPORT(nativeImportAsana, loom_import_asana)
RN_TICKET_IMPORT(nativeImportJira, loom_import_jira)

#undef RN_TICKET_IMPORT

#define RN_STRING_IMPORT(java_name, c_name, payload_arg, text_arg) \
extern "C" JNIEXPORT jbyteArray JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##java_name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring profile, \
    jstring sourceScope, jbyteArray payload_arg, jstring text_arg, jboolean dryRun, \
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) { \
  (void)thiz; \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { \
    throwLoom(env); \
    return nullptr; \
  } \
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr); \
  const char *profileChars = env->GetStringUTFChars(profile, nullptr); \
  const char *scopeChars = env->GetStringUTFChars(sourceScope, nullptr); \
  jsize payloadLen = env->GetArrayLength(payload_arg); \
  jbyte *payloadBytes = env->GetByteArrayElements(payload_arg, nullptr); \
  const char *textChars = env->GetStringUTFChars(text_arg, nullptr); \
  unsigned char *ptr = nullptr; \
  uintptr_t len = 0; \
  st = c_name(h, workspaceChars, profileChars, scopeChars, \
              reinterpret_cast<const unsigned char *>(payloadBytes), \
              static_cast<uintptr_t>(payloadLen), textChars, dryRun ? 1 : 0, &ptr, &len); \
  env->ReleaseStringUTFChars(workspace, workspaceChars); \
  env->ReleaseStringUTFChars(profile, profileChars); \
  env->ReleaseStringUTFChars(sourceScope, scopeChars); \
  env->ReleaseByteArrayElements(payload_arg, payloadBytes, JNI_ABORT); \
  env->ReleaseStringUTFChars(text_arg, textChars); \
  return finishRnStoreBytes(env, h, st, ptr, len); \
}

RN_STRING_IMPORT(nativeImportConfluence, loom_import_confluence, snapshotPayload, defaultSpace)
RN_STRING_IMPORT(nativeImportMarkdown, loom_import_markdown, archivePayload, space)
RN_STRING_IMPORT(nativeImportNotion, loom_import_notion, snapshotPayload, defaultSpace)

#undef RN_STRING_IMPORT

#define RN_SIMPLE_IMPORT(java_name, c_name, payload_arg) \
extern "C" JNIEXPORT jbyteArray JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##java_name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring profile, \
    jstring sourceScope, jbyteArray payload_arg, jboolean dryRun, jbyteArray passphrase, \
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) { \
  (void)thiz; \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { \
    throwLoom(env); \
    return nullptr; \
  } \
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr); \
  const char *profileChars = env->GetStringUTFChars(profile, nullptr); \
  const char *scopeChars = env->GetStringUTFChars(sourceScope, nullptr); \
  jsize payloadLen = env->GetArrayLength(payload_arg); \
  jbyte *payloadBytes = env->GetByteArrayElements(payload_arg, nullptr); \
  unsigned char *ptr = nullptr; \
  uintptr_t len = 0; \
  st = c_name(h, workspaceChars, profileChars, scopeChars, \
              reinterpret_cast<const unsigned char *>(payloadBytes), \
              static_cast<uintptr_t>(payloadLen), dryRun ? 1 : 0, &ptr, &len); \
  env->ReleaseStringUTFChars(workspace, workspaceChars); \
  env->ReleaseStringUTFChars(profile, profileChars); \
  env->ReleaseStringUTFChars(sourceScope, scopeChars); \
  env->ReleaseByteArrayElements(payload_arg, payloadBytes, JNI_ABORT); \
  return finishRnStoreBytes(env, h, st, ptr, len); \
}

RN_SIMPLE_IMPORT(nativeImportSlack, loom_import_slack, snapshotPayload)
RN_SIMPLE_IMPORT(nativeImportDrive, loom_import_drive, archivePayload)

#undef RN_SIMPLE_IMPORT

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeInferenceInstanceCreateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring name, jstring model,
    jstring kind, jstring runtime, jstring preset, jstring settingsJson, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *nameChars = env->GetStringUTFChars(name, nullptr);
  const char *modelChars = env->GetStringUTFChars(model, nullptr);
  const char *kindChars = env->GetStringUTFChars(kind, nullptr);
  const char *runtimeChars = env->GetStringUTFChars(runtime, nullptr);
  const char *presetValue = preset ? env->GetStringUTFChars(preset, nullptr) : nullptr;
  const char *settingsValue = settingsJson ? env->GetStringUTFChars(settingsJson, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_inference_instance_create_json(
      h, workspaceChars, nameChars, modelChars, kindChars, runtimeChars,
      presetValue, settingsValue, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(name, nameChars);
  env->ReleaseStringUTFChars(model, modelChars);
  env->ReleaseStringUTFChars(kind, kindChars);
  env->ReleaseStringUTFChars(runtime, runtimeChars);
  if (presetValue) {
    env->ReleaseStringUTFChars(preset, presetValue);
  }
  if (settingsValue) {
    env->ReleaseStringUTFChars(settingsJson, settingsValue);
  }
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeInferenceInstanceUpdateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring name, jstring preset,
    jstring settingsJson, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *nameChars = env->GetStringUTFChars(name, nullptr);
  const char *presetValue = preset ? env->GetStringUTFChars(preset, nullptr) : nullptr;
  const char *settingsValue = settingsJson ? env->GetStringUTFChars(settingsJson, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_inference_instance_update_json(
      h, workspaceChars, nameChars, presetValue, settingsValue, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(name, nameChars);
  if (presetValue) {
    env->ReleaseStringUTFChars(preset, presetValue);
  }
  if (settingsValue) {
    env->ReleaseStringUTFChars(settingsJson, settingsValue);
  }
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeInferenceInstanceDeleteJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring name,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *nameChars = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  st = loom_inference_instance_delete_json(h, workspaceChars, nameChars, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(name, nameChars);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeServeListenerConfigureJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring requestJson, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *requestChars = env->GetStringUTFChars(requestJson, nullptr);
  char *out = nullptr;
  st = loom_serve_listener_configure_json(h, requestChars, &out);
  env->ReleaseStringUTFChars(requestJson, requestChars);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeServeListenerListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  char *out = nullptr;
  st = loom_serve_listener_list_json(h, &out);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeServeListenerSetEnabledJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring listenerId, jboolean enabled,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *listenerChars = env->GetStringUTFChars(listenerId, nullptr);
  char *out = nullptr;
  st = loom_serve_listener_set_enabled_json(h, listenerChars, enabled ? 1 : 0, &out);
  env->ReleaseStringUTFChars(listenerId, listenerChars);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeServeListenerRemoveJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring listenerId, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *listenerChars = env->GetStringUTFChars(listenerId, nullptr);
  char *out = nullptr;
  st = loom_serve_listener_remove_json(h, listenerChars, &out);
  env->ReleaseStringUTFChars(listenerId, listenerChars);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeServeWebRouteListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring listenerId, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *listenerChars = env->GetStringUTFChars(listenerId, nullptr);
  char *out = nullptr;
  st = loom_serve_web_route_list_json(h, listenerChars, &out);
  env->ReleaseStringUTFChars(listenerId, listenerChars);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeServeWebRouteSetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring requestJson, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *requestChars = env->GetStringUTFChars(requestJson, nullptr);
  char *out = nullptr;
  st = loom_serve_web_route_set_json(h, requestChars, &out);
  env->ReleaseStringUTFChars(requestJson, requestChars);
  return finishRnStoreString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeServeWebRouteRemoveJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring listenerId, jstring routeId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *listenerChars = env->GetStringUTFChars(listenerId, nullptr);
  const char *routeChars = env->GetStringUTFChars(routeId, nullptr);
  char *out = nullptr;
  st = loom_serve_web_route_remove_json(h, listenerChars, routeChars, &out);
  env->ReleaseStringUTFChars(listenerId, listenerChars);
  env->ReleaseStringUTFChars(routeId, routeChars);
  return finishRnStoreString(env, h, st, out);
}
