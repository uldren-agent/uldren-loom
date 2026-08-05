#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeFsImport(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring srcPath, jstring author,
    jstring message, jboolean commit, jboolean dryRun, jbyteArray passphrase, jbyteArray kek,
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
  const char *src = env->GetStringUTFChars(srcPath, nullptr);
  const char *a = author != nullptr ? env->GetStringUTFChars(author, nullptr) : nullptr;
  const char *m = message != nullptr ? env->GetStringUTFChars(message, nullptr) : nullptr;
  const char *authorArg = (a != nullptr && a[0] != '\0') ? a : nullptr;
  const char *messageArg = (m != nullptr && m[0] != '\0') ? m : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_fs_import(h, n, src, authorArg, messageArg, commit ? 1 : 0, dryRun ? 1 : 0, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(srcPath, src);
  if (a != nullptr) {
    env->ReleaseStringUTFChars(author, a);
  }
  if (m != nullptr) {
    env->ReleaseStringUTFChars(message, m);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeFsExport(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring dstPath, jstring revision,
    jboolean dryRun, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *dst = env->GetStringUTFChars(dstPath, nullptr);
  const char *rev = revision != nullptr ? env->GetStringUTFChars(revision, nullptr) : nullptr;
  const char *revArg = (rev != nullptr && rev[0] != '\0') ? rev : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_fs_export(h, n, dst, revArg, dryRun ? 1 : 0, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(dstPath, dst);
  if (rev != nullptr) {
    env->ReleaseStringUTFChars(revision, rev);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeArchiveImport(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring srcPath, jstring kind,
    jstring gzipOutputPath, jboolean commit, jstring author, jstring message, jboolean dryRun,
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
  const char *src = env->GetStringUTFChars(srcPath, nullptr);
  const char *k = env->GetStringUTFChars(kind, nullptr);
  const char *gzip = gzipOutputPath != nullptr ? env->GetStringUTFChars(gzipOutputPath, nullptr) : nullptr;
  const char *a = author != nullptr ? env->GetStringUTFChars(author, nullptr) : nullptr;
  const char *m = message != nullptr ? env->GetStringUTFChars(message, nullptr) : nullptr;
  const char *gzipArg = (gzip != nullptr && gzip[0] != '\0') ? gzip : nullptr;
  const char *authorArg = (a != nullptr && a[0] != '\0') ? a : nullptr;
  const char *messageArg = (m != nullptr && m[0] != '\0') ? m : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_archive_import(h, n, src, k, gzipArg, commit ? 1 : 0, authorArg, messageArg,
                           dryRun ? 1 : 0, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(srcPath, src);
  env->ReleaseStringUTFChars(kind, k);
  if (gzip != nullptr) {
    env->ReleaseStringUTFChars(gzipOutputPath, gzip);
  }
  if (a != nullptr) {
    env->ReleaseStringUTFChars(author, a);
  }
  if (m != nullptr) {
    env->ReleaseStringUTFChars(message, m);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeArchiveExport(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring dstPath, jstring kind,
    jstring revision, jboolean dryRun, jbyteArray passphrase, jbyteArray kek,
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
  const char *dst = env->GetStringUTFChars(dstPath, nullptr);
  const char *k = env->GetStringUTFChars(kind, nullptr);
  const char *rev = revision != nullptr ? env->GetStringUTFChars(revision, nullptr) : nullptr;
  const char *revArg = (rev != nullptr && rev[0] != '\0') ? rev : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_archive_export(h, n, dst, k, revArg, dryRun ? 1 : 0, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(dstPath, dst);
  env->ReleaseStringUTFChars(kind, k);
  if (rev != nullptr) {
    env->ReleaseStringUTFChars(revision, rev);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCarImport(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring srcPath, jboolean dryRun,
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
  const char *src = env->GetStringUTFChars(srcPath, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_car_import(h, src, dryRun ? 1 : 0, &ptr, &len);
  env->ReleaseStringUTFChars(srcPath, src);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCarExport(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring dstPath, jboolean dryRun,
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
  const char *dst = env->GetStringUTFChars(dstPath, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_car_export(h, n, dst, dryRun ? 1 : 0, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(dstPath, dst);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
