#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVcsBlame(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring branch,
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
  const char *b = env->GetStringUTFChars(branch, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_vcs_blame(h, n, b, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(branch, b);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVcsDiff(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring fromCommit,
    jstring toCommit, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *from = env->GetStringUTFChars(fromCommit, nullptr);
  const char *to = env->GetStringUTFChars(toCommit, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_vcs_diff(h, n, from, to, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(fromCommit, from);
  env->ReleaseStringUTFChars(toCommit, to);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeWatchSubscribe(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring branch,
    jstring facet, jstring pathPrefix, jstring changeKinds, jstring fromCommit,
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
  const char *b = env->GetStringUTFChars(branch, nullptr);
  const char *f = env->GetStringUTFChars(facet, nullptr);
  const char *prefix = env->GetStringUTFChars(pathPrefix, nullptr);
  const char *kinds = env->GetStringUTFChars(changeKinds, nullptr);
  const char *from = env->GetStringUTFChars(fromCommit, nullptr);
  char *out = nullptr;
  st = loom_watch_subscribe(h, n, b, f, prefix, kinds, from, &out);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(branch, b);
  env->ReleaseStringUTFChars(facet, f);
  env->ReleaseStringUTFChars(pathPrefix, prefix);
  env->ReleaseStringUTFChars(changeKinds, kinds);
  env->ReleaseStringUTFChars(fromCommit, from);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeWatchPoll(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring cursor, jint max,
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
  const char *c = env->GetStringUTFChars(cursor, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_watch_poll(h, c, (uint32_t)max, &ptr, &len);
  env->ReleaseStringUTFChars(cursor, c);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
