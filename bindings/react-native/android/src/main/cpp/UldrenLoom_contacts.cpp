#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardCreateBook(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
    jstring displayName, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  const char *dn = env->GetStringUTFChars(displayName, nullptr);
  st = loom_card_create_book(h, n, pr, bk, dn);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
  env->ReleaseStringUTFChars(displayName, dn);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardDeleteBook(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  int32_t found = 0;
  st = loom_card_delete_book(h, n, pr, bk, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardListBooks(
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
  st = loom_card_list_books(h, n, pr, &ptr, &len);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardPutEntry(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  jsize elen = (entry != nullptr) ? env->GetArrayLength(entry) : 0;
  jbyte *e = (entry != nullptr) ? env->GetByteArrayElements(entry, nullptr) : nullptr;
  st = loom_card_put_entry(h, n, pr, bk, reinterpret_cast<const unsigned char *>(e),
                           static_cast<uintptr_t>(elen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
  if (e) {
    env->ReleaseByteArrayElements(entry, e, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardGetEntry(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_card_get_entry(h, n, pr, bk, u, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardDeleteEntry(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  int32_t found = 0;
  st = loom_card_delete_entry(h, n, pr, bk, u, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
  env->ReleaseStringUTFChars(uid, u);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardListEntries(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_card_list_entries(h, n, pr, bk, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardSearch(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
    jstring text, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  const char *tx = env->GetStringUTFChars(text, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_card_search(h, n, pr, bk, tx, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
  env->ReleaseStringUTFChars(text, tx);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardEntryVcard(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  char *out = nullptr;
  int32_t found = 0;
  st = loom_card_entry_vcard(h, n, pr, bk, u, &out, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCardPutVcard(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring book,
    jstring vcf, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *bk = env->GetStringUTFChars(book, nullptr);
  const char *vc = env->GetStringUTFChars(vcf, nullptr);
  char *out = nullptr;
  st = loom_card_put_vcard(h, n, pr, bk, vc, &out);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(book, bk);
  env->ReleaseStringUTFChars(vcf, vc);
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

// --- Mail facade (mailboxes + messages). Each call opens the loom for the op and closes. ---
