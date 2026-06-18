#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTsPut(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jlong ts,
    jbyteArray value, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *m = env->GetStringUTFChars(collection, nullptr);
  jsize vlen = (value != nullptr) ? env->GetArrayLength(value) : 0;
  jbyte *v = (value != nullptr) ? env->GetByteArrayElements(value, nullptr) : nullptr;
  st = loom_ts_put(h, n, m, static_cast<int64_t>(ts), reinterpret_cast<const unsigned char *>(v),
                   static_cast<uintptr_t>(vlen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  if (v) {
    env->ReleaseByteArrayElements(value, v, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTsGet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jlong ts,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_ts_get(h, n, m, static_cast<int64_t>(ts), &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  if (found == 0) {
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTsRange(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jlong from, jlong to,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_ts_range_cbor(h, n, m, static_cast<int64_t>(from), static_cast<int64_t>(to), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTsLatest(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jlongArray outTs,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  int64_t ts = 0;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_ts_latest(h, n, m, &ts, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  if (found == 0) {
    return nullptr;
  }
  if (outTs != nullptr) {
    jlong tsj = static_cast<jlong>(ts);
    env->SetLongArrayRegion(outTs, 0, 1, &tsj);
  }
  return ownedBytes(env, ptr, len);
}
