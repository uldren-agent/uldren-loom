#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMeetingsImportSnapshot(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring inputProfile,
    jbyteArray snapshot, jboolean dryRun, jbyteArray passphrase, jbyteArray kek,
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
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *profile = env->GetStringUTFChars(inputProfile, nullptr);
  jsize snapshotLen = (snapshot != nullptr) ? env->GetArrayLength(snapshot) : 0;
  jbyte *snapshotBytes = (snapshot != nullptr) ? env->GetByteArrayElements(snapshot, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_meetings_import_snapshot(
      h, n, profile, reinterpret_cast<const unsigned char *>(snapshotBytes),
      static_cast<uintptr_t>(snapshotLen), dryRun ? 1 : 0, &out);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(inputProfile, profile);
  if (snapshotBytes) {
    env->ReleaseByteArrayElements(snapshot, snapshotBytes, JNI_ABORT);
  }
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

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMeetingsSourceRead(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring sourceId, jstring leaf,
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
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *source = env->GetStringUTFChars(sourceId, nullptr);
  const char *payloadLeaf = env->GetStringUTFChars(leaf, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_meetings_source_read(h, n, source, payloadLeaf, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(sourceId, source);
  env->ReleaseStringUTFChars(leaf, payloadLeaf);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
