#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalCreateCollection(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
    jstring displayName, jstring components, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  const char *dn = env->GetStringUTFChars(displayName, nullptr);
  const char *cmp = env->GetStringUTFChars(components, nullptr);
  st = loom_cal_create_collection(h, n, pr, col, dn, cmp);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  env->ReleaseStringUTFChars(displayName, dn);
  env->ReleaseStringUTFChars(components, cmp);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalDeleteCollection(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  int32_t found = 0;
  st = loom_cal_delete_collection(h, n, pr, col, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalListCollections(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jbyteArray passphrase,
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_cal_list_collections(h, n, pr, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalPutEntry(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
    jbyteArray entry, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  jsize elen = (entry != nullptr) ? env->GetArrayLength(entry) : 0;
  jbyte *e = (entry != nullptr) ? env->GetByteArrayElements(entry, nullptr) : nullptr;
  st = loom_cal_put_entry(h, n, pr, col, reinterpret_cast<const unsigned char *>(e),
                          static_cast<uintptr_t>(elen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  if (e) {
    env->ReleaseByteArrayElements(entry, e, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalGetEntry(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
    jstring uid, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_cal_get_entry(h, n, pr, col, u, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  env->ReleaseStringUTFChars(uid, u);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalDeleteEntry(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
    jstring uid, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  int32_t found = 0;
  st = loom_cal_delete_entry(h, n, pr, col, u, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  env->ReleaseStringUTFChars(uid, u);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalListEntries(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_cal_list_entries(h, n, pr, col, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalRange(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
    jstring from, jstring to, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  const char *fr = env->GetStringUTFChars(from, nullptr);
  const char *t = env->GetStringUTFChars(to, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_cal_range(h, n, pr, col, fr, t, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  env->ReleaseStringUTFChars(from, fr);
  env->ReleaseStringUTFChars(to, t);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalSearch(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
    jstring component, jstring text, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  const char *cmp = env->GetStringUTFChars(component, nullptr);
  const char *tx = env->GetStringUTFChars(text, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_cal_search(h, n, pr, col, cmp, tx, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  env->ReleaseStringUTFChars(component, cmp);
  env->ReleaseStringUTFChars(text, tx);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalEntryIcs(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
    jstring uid, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  char *out = nullptr;
  int32_t found = 0;
  st = loom_cal_entry_ics(h, n, pr, col, u, &out, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  env->ReleaseStringUTFChars(uid, u);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  if (found == 0) {
    return nullptr;
  }
  jstring r = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return r;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCalPutIcs(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring collection,
    jstring ics, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *col = env->GetStringUTFChars(collection, nullptr);
  const char *ic = env->GetStringUTFChars(ics, nullptr);
  char *out = nullptr;
  st = loom_cal_put_ics(h, n, pr, col, ic, &out);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(collection, col);
  env->ReleaseStringUTFChars(ics, ic);
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

// --- Contacts facade (CardDAV address books + contacts). Each call opens the loom for the op and closes. ---
