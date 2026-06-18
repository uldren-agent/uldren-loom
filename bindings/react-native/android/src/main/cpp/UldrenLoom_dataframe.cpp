#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDataframeCreate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray plan,
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
  jsize plen = (plan != nullptr) ? env->GetArrayLength(plan) : 0;
  jbyte *pl = (plan != nullptr) ? env->GetByteArrayElements(plan, nullptr) : nullptr;
  st = loom_dataframe_create(h, n, nm, reinterpret_cast<const unsigned char *>(pl),
                             static_cast<uintptr_t>(plen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (pl) {
    env->ReleaseByteArrayElements(plan, pl, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDataframeCollect(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray passphrase,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_dataframe_collect_cbor(h, n, nm, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDataframePreview(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jlong rows,
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
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_dataframe_preview_cbor(h, n, nm, static_cast<uint64_t>(rows), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDataframeMaterialize(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray passphrase,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  int32_t has_digest = 0;
  st = loom_dataframe_materialize(h, n, nm, &out, &has_digest);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    if (out) {
      loom_string_free(out);
    }
    throwLoom(env);
    return nullptr;
  }
  if (has_digest == 0) {
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDataframePlanDigest(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray passphrase,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  st = loom_dataframe_plan_digest(h, n, nm, &out);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    if (out) {
      loom_string_free(out);
    }
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDataframeSourceDigests(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray passphrase,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_dataframe_source_digests_cbor(h, n, nm, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
