#include "UldrenLoom_jni.h"

static jbyteArray finishLanesBytes(JNIEnv *env, LoomSession *h, int32_t st, unsigned char *ptr,
                                   uintptr_t len) {
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

#define LANES_OPEN() \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { throwLoom(env); return nullptr; } \
  const char *n = env->GetStringUTFChars(ns, nullptr)

#define LANES_RELEASE_NS() \
  env->ReleaseStringUTFChars(ns, n)

static const char *optionalString(JNIEnv *env, jstring value) {
  return value ? env->GetStringUTFChars(value, nullptr) : nullptr;
}

static void releaseOptionalString(JNIEnv *env, jstring value, const char *chars) {
  if (value && chars) {
    env->ReleaseStringUTFChars(value, chars);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLanesCreate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jbyteArray lane,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LANES_OPEN();
  jbyte *bytes = env->GetByteArrayElements(lane, nullptr);
  jsize inLen = env->GetArrayLength(lane);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_lanes_create_cbor(h, n, reinterpret_cast<const unsigned char *>(bytes),
                              static_cast<uintptr_t>(inLen), &ptr, &len);
  LANES_RELEASE_NS();
  env->ReleaseByteArrayElements(lane, bytes, JNI_ABORT);
  return finishLanesBytes(env, h, st, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLanesGet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring laneId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LANES_OPEN();
  const char *lane = env->GetStringUTFChars(laneId, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_lanes_get_cbor(h, n, lane, &ptr, &len, &found);
  LANES_RELEASE_NS();
  env->ReleaseStringUTFChars(laneId, lane);
  if (st != 0) {
    return finishLanesBytes(env, h, st, ptr, len);
  }
  if (!found) {
    loom_close(h);
    return nullptr;
  }
  return finishLanesBytes(env, h, st, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLanesList(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LANES_OPEN();
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_lanes_list_cbor(h, n, &ptr, &len);
  LANES_RELEASE_NS();
  return finishLanesBytes(env, h, st, ptr, len);
}

#define LANES_STRING_MUTATION(java_name, c_name) \
extern "C" JNIEXPORT jbyteArray JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##java_name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring laneId, \
    jstring value, jstring updatedBy, jbyteArray passphrase, jbyteArray kek, \
    jstring authPrincipal, jbyteArray authPassphrase) { \
  (void)thiz; \
  LANES_OPEN(); \
  const char *lane = env->GetStringUTFChars(laneId, nullptr); \
  const char *val = env->GetStringUTFChars(value, nullptr); \
  const char *actor = env->GetStringUTFChars(updatedBy, nullptr); \
  unsigned char *ptr = nullptr; \
  uintptr_t len = 0; \
  st = c_name(h, n, lane, val, actor, &ptr, &len); \
  LANES_RELEASE_NS(); \
  env->ReleaseStringUTFChars(laneId, lane); \
  env->ReleaseStringUTFChars(value, val); \
  env->ReleaseStringUTFChars(updatedBy, actor); \
  return finishLanesBytes(env, h, st, ptr, len); \
}

LANES_STRING_MUTATION(nativeLanesTicketRemove, loom_lanes_ticket_remove_cbor)

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLanesUpdate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring laneId, jstring title,
    jstring description, jstring laneStatus, jstring statusReport, jstring reviewerFeedback,
    jstring updatedBy, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  LANES_OPEN();
  const char *lane = env->GetStringUTFChars(laneId, nullptr);
  const char *titleChars = optionalString(env, title);
  const char *descriptionChars = optionalString(env, description);
  const char *laneStatusChars = optionalString(env, laneStatus);
  const char *statusReportChars = optionalString(env, statusReport);
  const char *reviewerFeedbackChars = optionalString(env, reviewerFeedback);
  const char *actor = env->GetStringUTFChars(updatedBy, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_lanes_update_cbor(h, n, lane, titleChars, descriptionChars, laneStatusChars,
                              statusReportChars, reviewerFeedbackChars, actor, &ptr, &len);
  LANES_RELEASE_NS();
  env->ReleaseStringUTFChars(laneId, lane);
  releaseOptionalString(env, title, titleChars);
  releaseOptionalString(env, description, descriptionChars);
  releaseOptionalString(env, laneStatus, laneStatusChars);
  releaseOptionalString(env, statusReport, statusReportChars);
  releaseOptionalString(env, reviewerFeedback, reviewerFeedbackChars);
  env->ReleaseStringUTFChars(updatedBy, actor);
  return finishLanesBytes(env, h, st, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLanesTicketAdd(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring laneId, jstring ticketId,
    jstring updatedBy, jstring placement, jstring anchor, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LANES_OPEN();
  const char *lane = env->GetStringUTFChars(laneId, nullptr);
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *actor = env->GetStringUTFChars(updatedBy, nullptr);
  const char *place = placement ? env->GetStringUTFChars(placement, nullptr) : nullptr;
  const char *anchorChars = anchor ? env->GetStringUTFChars(anchor, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_lanes_ticket_add_cbor(h, n, lane, ticket, actor, place, anchorChars, &ptr, &len);
  LANES_RELEASE_NS();
  env->ReleaseStringUTFChars(laneId, lane);
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(updatedBy, actor);
  if (placement) env->ReleaseStringUTFChars(placement, place);
  if (anchor) env->ReleaseStringUTFChars(anchor, anchorChars);
  return finishLanesBytes(env, h, st, ptr, len);
}

#undef LANES_OPEN
#undef LANES_RELEASE_NS
#undef LANES_STRING_MUTATION
