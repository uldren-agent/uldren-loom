#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLedgerAppend(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jbyteArray payload,
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
  jsize plen = (payload != nullptr) ? env->GetArrayLength(payload) : 0;
  jbyte *pl = (payload != nullptr) ? env->GetByteArrayElements(payload, nullptr) : nullptr;
  uint64_t seq = 0;
  st = loom_ledger_append(h, n, m, reinterpret_cast<const unsigned char *>(pl),
                          static_cast<uintptr_t>(plen), &seq);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  if (pl) {
    env->ReleaseByteArrayElements(payload, pl, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return u64String(env, seq);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLedgerGet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring seqText,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t seq = 0;
  if (!parseU64String(env, seqText, &seq)) {
    return nullptr;
  }
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
  st = loom_ledger_get(h, n, m, seq, &ptr, &len, &found);
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

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLedgerHead(
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
  char *out = nullptr;
  int32_t found = 0;
  st = loom_ledger_head(h, n, m, &out, &found);
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
  jstring result = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLedgerLen(
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
  uint64_t out = 0;
  st = loom_ledger_len(h, n, m, &out);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return u64String(env, out);
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLedgerVerify(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jbyteArray passphrase,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  st = loom_ledger_verify(h, n, m);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

// --- Calendar facade (CalDAV collections + entries). Each call opens the loom for the op and closes. ---
