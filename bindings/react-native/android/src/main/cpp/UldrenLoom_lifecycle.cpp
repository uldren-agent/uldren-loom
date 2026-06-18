#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVersion(JNIEnv *env, jobject thiz) {
  (void)thiz;
  char *v = loom_version();
  jstring out = env->NewStringUTF(v ? v : "");
  if (v) {
    loom_string_free(v);
  }
  return out;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeBlobDigest(JNIEnv *env, jobject thiz,
                                                         jbyteArray data) {
  (void)thiz;
  jsize len = env->GetArrayLength(data);
  jbyte *buf = env->GetByteArrayElements(data, nullptr);
  char *d = loom_blob_digest(reinterpret_cast<const unsigned char *>(buf), static_cast<size_t>(len));
  env->ReleaseByteArrayElements(data, buf, JNI_ABORT);
  jstring out = env->NewStringUTF(d ? d : "");
  if (d) {
    loom_string_free(d);
  }
  return out;
}

// Create a fresh `.loom` under an identity profile, optionally encrypted under a passphrase.
// An empty `suite` (or empty `passphrase` byte[]) means profile-default / unencrypted.

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCreate(JNIEnv *env, jobject thiz, jstring loomPath,
                                                     jstring profile, jstring suite,
                                                     jbyteArray passphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *prof = env->GetStringUTFChars(profile, nullptr);
  const char *su = env->GetStringUTFChars(suite, nullptr);
  const char *suiteArg = (su && su[0]) ? su : nullptr;
  jbyte *pass = nullptr;
  jsize plen = 0;
  if (passphrase != nullptr) {
    plen = env->GetArrayLength(passphrase);
    pass = env->GetByteArrayElements(passphrase, nullptr);
  }
  int32_t st = loom_create(p, prof, suiteArg, reinterpret_cast<const unsigned char *>(pass),
                           static_cast<uintptr_t>(plen));
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(profile, prof);
  env->ReleaseStringUTFChars(suite, su);
  if (pass) {
    env->ReleaseByteArrayElements(passphrase, pass, JNI_ABORT);
  }
  if (st != 0) {
    throwLoom(env);
  }
}

// As `nativeCreate`, but wraps the DEK under a host-supplied 256-bit KEK.

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCreateWithKek(JNIEnv *env, jobject thiz,
                                                            jstring loomPath, jstring profile,
                                                            jstring suite, jbyteArray kek) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *prof = env->GetStringUTFChars(profile, nullptr);
  const char *su = env->GetStringUTFChars(suite, nullptr);
  const char *suiteArg = (su && su[0]) ? su : nullptr;
  jbyte *k = nullptr;
  jsize klen = 0;
  if (kek != nullptr) {
    klen = env->GetArrayLength(kek);
    k = env->GetByteArrayElements(kek, nullptr);
  }
  int32_t st = loom_create_with_kek(p, prof, suiteArg, reinterpret_cast<const unsigned char *>(k),
                                    static_cast<uintptr_t>(klen));
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(profile, prof);
  env->ReleaseStringUTFChars(suite, su);
  if (k) {
    env->ReleaseByteArrayElements(kek, k, JNI_ABORT);
  }
  if (st != 0) {
    throwLoom(env);
  }
}

// Open a session choosing the opener from the supplied key: a non-empty `kek`
// -> KEK unlock (the C ABI validates the 32-byte length); else a non-empty `passphrase` -> passphrase
// unlock; else the plain open. Returns the C status (0 = success, sets *out).

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCapabilities(JNIEnv *env, jobject thiz) {
  (void)thiz;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  if (loom_capabilities(&ptr, &len) != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeRuntimeProfile(JNIEnv *env, jobject thiz) {
  (void)thiz;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  if (loom_runtime_profile(&ptr, &len) != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStudioSurfaceCatalogJson(
    JNIEnv *env, jobject thiz, jstring workspace, jstring set) {
  (void)thiz;
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *setChars = env->GetStringUTFChars(set, nullptr);
  char *out = nullptr;
  int32_t st = loom_studio_surface_catalog_json(workspaceChars, setChars, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(set, setChars);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeExecCbor(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray request,
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
  jsize reqLen = env->GetArrayLength(request);
  jbyte *req = env->GetByteArrayElements(request, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_exec_cbor(
      h, reinterpret_cast<const unsigned char *>(req), static_cast<uintptr_t>(reqLen), &ptr, &len);
  env->ReleaseByteArrayElements(request, req, JNI_ABORT);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
