#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSearchCreate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray mapping,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  jsize mlen = (mapping != nullptr) ? env->GetArrayLength(mapping) : 0;
  jbyte *m = (mapping != nullptr) ? env->GetByteArrayElements(mapping, nullptr) : nullptr;
  st = loom_search_create(h, n, nm, reinterpret_cast<const unsigned char *>(m),
                          static_cast<uintptr_t>(mlen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (m) {
    env->ReleaseByteArrayElements(mapping, m, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSearchIndex(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray id,
    jbyteArray doc, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  jsize idlen = (id != nullptr) ? env->GetArrayLength(id) : 0;
  jbyte *i = (id != nullptr) ? env->GetByteArrayElements(id, nullptr) : nullptr;
  jsize dlen = (doc != nullptr) ? env->GetArrayLength(doc) : 0;
  jbyte *d = (doc != nullptr) ? env->GetByteArrayElements(doc, nullptr) : nullptr;
  st = loom_search_index(h, n, nm, reinterpret_cast<const unsigned char *>(i),
                         static_cast<uintptr_t>(idlen),
                         reinterpret_cast<const unsigned char *>(d),
                         static_cast<uintptr_t>(dlen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (i) {
    env->ReleaseByteArrayElements(id, i, JNI_ABORT);
  }
  if (d) {
    env->ReleaseByteArrayElements(doc, d, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSearchGet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray id,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  jsize idlen = (id != nullptr) ? env->GetArrayLength(id) : 0;
  jbyte *i = (id != nullptr) ? env->GetByteArrayElements(id, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_search_get(h, n, nm, reinterpret_cast<const unsigned char *>(i),
                       static_cast<uintptr_t>(idlen), &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (i) {
    env->ReleaseByteArrayElements(id, i, JNI_ABORT);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSearchDelete(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray id,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  jsize idlen = (id != nullptr) ? env->GetArrayLength(id) : 0;
  jbyte *i = (id != nullptr) ? env->GetByteArrayElements(id, nullptr) : nullptr;
  int32_t found = 0;
  st = loom_search_delete(h, n, nm, reinterpret_cast<const unsigned char *>(i),
                          static_cast<uintptr_t>(idlen), &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (i) {
    env->ReleaseByteArrayElements(id, i, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSearchIds(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray prefix,
    jboolean hasPrefix, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  jsize pflen = (prefix != nullptr) ? env->GetArrayLength(prefix) : 0;
  jbyte *pf = (prefix != nullptr) ? env->GetByteArrayElements(prefix, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_search_ids_cbor(h, n, nm, reinterpret_cast<const unsigned char *>(pf),
                            static_cast<uintptr_t>(pflen), (hasPrefix == JNI_TRUE) ? 1 : 0, &ptr,
                            &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (pf) {
    env->ReleaseByteArrayElements(prefix, pf, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSearchRemap(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray mapping,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  jsize mlen = (mapping != nullptr) ? env->GetArrayLength(mapping) : 0;
  jbyte *m = (mapping != nullptr) ? env->GetByteArrayElements(mapping, nullptr) : nullptr;
  st = loom_search_remap(h, n, nm, reinterpret_cast<const unsigned char *>(m),
                         static_cast<uintptr_t>(mlen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (m) {
    env->ReleaseByteArrayElements(mapping, m, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSearchQuery(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray request,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  jsize rlen = (request != nullptr) ? env->GetArrayLength(request) : 0;
  jbyte *r = (request != nullptr) ? env->GetByteArrayElements(request, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_search_query_cbor(h, n, nm, reinterpret_cast<const unsigned char *>(r),
                              static_cast<uintptr_t>(rlen), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (r) {
    env->ReleaseByteArrayElements(request, r, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
