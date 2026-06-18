#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarCreate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray columns,
    jlong targetSegmentRows, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  jsize clen = (columns != nullptr) ? env->GetArrayLength(columns) : 0;
  jbyte *c = (columns != nullptr) ? env->GetByteArrayElements(columns, nullptr) : nullptr;
  st = loom_columnar_create(h, n, nm, reinterpret_cast<const unsigned char *>(c),
                            static_cast<uintptr_t>(clen),
                            static_cast<uintptr_t>(targetSegmentRows));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (c) {
    env->ReleaseByteArrayElements(columns, c, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarAppend(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray row,
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
  jsize rlen = (row != nullptr) ? env->GetArrayLength(row) : 0;
  jbyte *r = (row != nullptr) ? env->GetByteArrayElements(row, nullptr) : nullptr;
  st = loom_columnar_append(h, n, nm, reinterpret_cast<const unsigned char *>(r),
                            static_cast<uintptr_t>(rlen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (r) {
    env->ReleaseByteArrayElements(row, r, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarScan(
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
  st = loom_columnar_scan_cbor(h, n, nm, &ptr, &len);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarColumns(
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
  st = loom_columnar_columns_cbor(h, n, nm, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jdouble JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarRows(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return 0;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  uint64_t count = 0;
  st = loom_columnar_rows(h, n, nm, &count);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return 0;
  }
  return static_cast<jdouble>(count);
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarCompact(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray passphrase,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  st = loom_columnar_compact(h, n, nm);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarInspect(
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
  st = loom_columnar_inspect_cbor(h, n, nm, &ptr, &len);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarSourceDigest(
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
  st = loom_columnar_source_digest_cbor(h, n, nm, &ptr, &len);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarSelect(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray columns,
    jbyteArray filter, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  jsize clen = (columns != nullptr) ? env->GetArrayLength(columns) : 0;
  jbyte *c = (columns != nullptr) ? env->GetByteArrayElements(columns, nullptr) : nullptr;
  jsize flen = (filter != nullptr) ? env->GetArrayLength(filter) : 0;
  jbyte *fl = (filter != nullptr) ? env->GetByteArrayElements(filter, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_columnar_select_cbor(h, n, nm, reinterpret_cast<const unsigned char *>(c),
                                 static_cast<uintptr_t>(clen),
                                 reinterpret_cast<const unsigned char *>(fl),
                                 static_cast<uintptr_t>(flen), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (c) {
    env->ReleaseByteArrayElements(columns, c, JNI_ABORT);
  }
  if (fl) {
    env->ReleaseByteArrayElements(filter, fl, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeColumnarAggregate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray aggregates,
    jbyteArray filter, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  jsize alen = (aggregates != nullptr) ? env->GetArrayLength(aggregates) : 0;
  jbyte *a = (aggregates != nullptr) ? env->GetByteArrayElements(aggregates, nullptr) : nullptr;
  jsize flen = (filter != nullptr) ? env->GetArrayLength(filter) : 0;
  jbyte *fl = (filter != nullptr) ? env->GetByteArrayElements(filter, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_columnar_aggregate_cbor(h, n, nm, reinterpret_cast<const unsigned char *>(a),
                                    static_cast<uintptr_t>(alen),
                                    reinterpret_cast<const unsigned char *>(fl),
                                    static_cast<uintptr_t>(flen), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (a) {
    env->ReleaseByteArrayElements(aggregates, a, JNI_ABORT);
  }
  if (fl) {
    env->ReleaseByteArrayElements(filter, fl, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
