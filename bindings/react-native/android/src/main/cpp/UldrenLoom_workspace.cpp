#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeWorkspaceCreate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jstring facet, jbyteArray passphrase,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  const char *f = env->GetStringUTFChars(facet, nullptr);
  char *out = nullptr;
  st = loom_workspace_create(h, (n && n[0]) ? n : nullptr, (f && f[0]) ? f : nullptr, &out);
  env->ReleaseStringUTFChars(name, n);
  env->ReleaseStringUTFChars(facet, f);
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

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeWorkspaceListJson(
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
  st = loom_workspace_list_json(h, &out);
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

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeWorkspaceRename(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring newName, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *nn = env->GetStringUTFChars(newName, nullptr);
  st = loom_workspace_rename(h, n, nn);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(newName, nn);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeWorkspaceDelete(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
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
  st = loom_workspace_delete(h, n);
  env->ReleaseStringUTFChars(ns, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}
