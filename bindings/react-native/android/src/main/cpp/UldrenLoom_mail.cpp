#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailCreateMailbox(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  const char *dn = env->GetStringUTFChars(displayName, nullptr);
  st = loom_mail_create_mailbox(h, n, pr, mb, dn);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
  env->ReleaseStringUTFChars(displayName, dn);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailDeleteMailbox(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  int32_t found = 0;
  st = loom_mail_delete_mailbox(h, n, pr, mb, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailListMailboxes(
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
  st = loom_mail_list_mailboxes(h, n, pr, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailIngestMessage(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
    jstring uid, jbyteArray raw, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  jsize rlen = (raw != nullptr) ? env->GetArrayLength(raw) : 0;
  jbyte *rb = (raw != nullptr) ? env->GetByteArrayElements(raw, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_mail_ingest_message(h, n, pr, mb, u, reinterpret_cast<const unsigned char *>(rb),
                                static_cast<uintptr_t>(rlen), &out);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
  env->ReleaseStringUTFChars(uid, u);
  if (rb) {
    env->ReleaseByteArrayElements(raw, rb, JNI_ABORT);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailGetMessage(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_mail_get_message(h, n, pr, mb, u, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
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

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailToEml(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_mail_to_eml(h, n, pr, mb, u, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailDeleteMessage(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  int32_t found = 0;
  st = loom_mail_delete_message(h, n, pr, mb, u, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
  env->ReleaseStringUTFChars(uid, u);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailListMessages(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_mail_list_messages(h, n, pr, mb, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailGetFlags(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_mail_get_flags(h, n, pr, mb, u, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
  env->ReleaseStringUTFChars(uid, u);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailSetFlags(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
    jstring uid, jbyteArray flags, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  const char *u = env->GetStringUTFChars(uid, nullptr);
  jsize flen = (flags != nullptr) ? env->GetArrayLength(flags) : 0;
  jbyte *fl = (flags != nullptr) ? env->GetByteArrayElements(flags, nullptr) : nullptr;
  st = loom_mail_set_flags(h, n, pr, mb, u, reinterpret_cast<const unsigned char *>(fl),
                           static_cast<uintptr_t>(flen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
  env->ReleaseStringUTFChars(uid, u);
  if (fl) {
    env->ReleaseByteArrayElements(flags, fl, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMailSearch(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring principal, jstring mailbox,
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
  const char *mb = env->GetStringUTFChars(mailbox, nullptr);
  const char *tx = env->GetStringUTFChars(text, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_mail_search(h, n, pr, mb, tx, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(mailbox, mb);
  env->ReleaseStringUTFChars(text, tx);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
