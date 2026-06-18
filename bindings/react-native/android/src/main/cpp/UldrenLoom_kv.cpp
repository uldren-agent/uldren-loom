#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeKvPut(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jbyteArray key,
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
  jsize klen = (key != nullptr) ? env->GetArrayLength(key) : 0;
  jbyte *k = (key != nullptr) ? env->GetByteArrayElements(key, nullptr) : nullptr;
  jsize vlen = (value != nullptr) ? env->GetArrayLength(value) : 0;
  jbyte *v = (value != nullptr) ? env->GetByteArrayElements(value, nullptr) : nullptr;
  st = loom_kv_put(h, n, m, reinterpret_cast<const unsigned char *>(k), static_cast<uintptr_t>(klen),
                   reinterpret_cast<const unsigned char *>(v), static_cast<uintptr_t>(vlen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  if (k) {
    env->ReleaseByteArrayElements(key, k, JNI_ABORT);
  }
  if (v) {
    env->ReleaseByteArrayElements(value, v, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeKvGet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jbyteArray key,
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
  jsize klen = (key != nullptr) ? env->GetArrayLength(key) : 0;
  jbyte *k = (key != nullptr) ? env->GetByteArrayElements(key, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_kv_get(h, n, m, reinterpret_cast<const unsigned char *>(k),
                   static_cast<uintptr_t>(klen), &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  if (k) {
    env->ReleaseByteArrayElements(key, k, JNI_ABORT);
  }
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

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeKvDelete(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jbyteArray key,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *m = env->GetStringUTFChars(collection, nullptr);
  jsize klen = (key != nullptr) ? env->GetArrayLength(key) : 0;
  jbyte *k = (key != nullptr) ? env->GetByteArrayElements(key, nullptr) : nullptr;
  int32_t found = 0;
  st = loom_kv_delete(h, n, m, reinterpret_cast<const unsigned char *>(k),
                      static_cast<uintptr_t>(klen), &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  if (k) {
    env->ReleaseByteArrayElements(key, k, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeKvList(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jbyteArray passphrase,
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
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *m = env->GetStringUTFChars(collection, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_kv_list_cbor(h, n, m, &ptr, &len);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeKvRange(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jbyteArray lo,
    jbyteArray hi, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *m = env->GetStringUTFChars(collection, nullptr);
  jsize lolen = (lo != nullptr) ? env->GetArrayLength(lo) : 0;
  jbyte *lob = (lo != nullptr) ? env->GetByteArrayElements(lo, nullptr) : nullptr;
  jsize hilen = (hi != nullptr) ? env->GetArrayLength(hi) : 0;
  jbyte *hib = (hi != nullptr) ? env->GetByteArrayElements(hi, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_kv_range_cbor(h, n, m, reinterpret_cast<const unsigned char *>(lob),
                          static_cast<uintptr_t>(lolen),
                          reinterpret_cast<const unsigned char *>(hib),
                          static_cast<uintptr_t>(hilen), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  if (lob) {
    env->ReleaseByteArrayElements(lo, lob, JNI_ABORT);
  }
  if (hib) {
    env->ReleaseByteArrayElements(hi, hib, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
