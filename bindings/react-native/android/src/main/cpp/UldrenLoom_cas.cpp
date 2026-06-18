#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCasPut(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jbyteArray content,
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
  jsize clen = (content != nullptr) ? env->GetArrayLength(content) : 0;
  jbyte *c = (content != nullptr) ? env->GetByteArrayElements(content, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_cas_put(h, n, reinterpret_cast<const unsigned char *>(c), static_cast<uintptr_t>(clen),
                    &out);
  env->ReleaseStringUTFChars(ns, n);
  if (c) {
    env->ReleaseByteArrayElements(content, c, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring r = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return r;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCasGet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring digest, jbyteArray passphrase,
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
  const char *dg = env->GetStringUTFChars(digest, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_cas_get(h, n, dg, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(digest, dg);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCasHas(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring digest, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *dg = env->GetStringUTFChars(digest, nullptr);
  int32_t found = 0;
  st = loom_cas_has(h, n, dg, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(digest, dg);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCasListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jbyteArray passphrase, jbyteArray kek,
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
  char *out = nullptr;
  st = loom_cas_list_json(h, n, &out);
  env->ReleaseStringUTFChars(ns, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring r = env->NewStringUTF(out ? out : "[]");
  if (out) {
    loom_string_free(out);
  }
  return r;
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCasDelete(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring digest, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *d = env->GetStringUTFChars(digest, nullptr);
  int32_t found = 0;
  st = loom_cas_delete(h, n, d, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(digest, d);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}
