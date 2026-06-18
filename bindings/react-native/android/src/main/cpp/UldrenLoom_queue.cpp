#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeQueueAppend(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring stream, jbyteArray entry,
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
  const char *s = env->GetStringUTFChars(stream, nullptr);
  jsize elen = (entry != nullptr) ? env->GetArrayLength(entry) : 0;
  jbyte *e = (entry != nullptr) ? env->GetByteArrayElements(entry, nullptr) : nullptr;
  uint64_t seq = 0;
  st = loom_queue_append(h, n, s, reinterpret_cast<const unsigned char *>(e),
                         static_cast<uintptr_t>(elen), &seq);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(stream, s);
  if (e) {
    env->ReleaseByteArrayElements(entry, e, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return u64String(env, seq);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeQueueGet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring stream, jstring seqText,
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
  const char *s = env->GetStringUTFChars(stream, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_queue_get(h, n, s, seq, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(stream, s);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  if (found == 0) {
    return nullptr;
  }
  jbyteArray result = env->NewByteArray(static_cast<jsize>(len));
  if (result != nullptr && len > 0) {
    env->SetByteArrayRegion(result, 0, static_cast<jsize>(len), reinterpret_cast<const jbyte *>(ptr));
  }
  loom_bytes_free(ptr, len);
  return result;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeQueueRange(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring stream, jstring loText,
    jstring hiText, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t lo = 0;
  uint64_t hi = 0;
  if (!parseU64String(env, loText, &lo) || !parseU64String(env, hiText, &hi)) {
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
  const char *s = env->GetStringUTFChars(stream, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_queue_range(h, n, s, lo, hi, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(stream, s);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jbyteArray result = env->NewByteArray(static_cast<jsize>(len));
  if (result != nullptr && len > 0) {
    env->SetByteArrayRegion(result, 0, static_cast<jsize>(len), reinterpret_cast<const jbyte *>(ptr));
  }
  loom_bytes_free(ptr, len);
  return result;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeQueueLen(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring stream, jbyteArray passphrase,
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
  const char *s = env->GetStringUTFChars(stream, nullptr);
  uint64_t len = 0;
  st = loom_queue_len(h, n, s, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(stream, s);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return u64String(env, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeQueueConsumerPosition(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring stream, jstring consumerId,
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
  const char *s = env->GetStringUTFChars(stream, nullptr);
  const char *c = env->GetStringUTFChars(consumerId, nullptr);
  uint64_t seq = 0;
  st = loom_queue_consumer_position(h, n, s, c, &seq);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(stream, s);
  env->ReleaseStringUTFChars(consumerId, c);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return u64String(env, seq);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeQueueConsumerRead(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring stream, jstring consumerId,
    jdouble max, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *s = env->GetStringUTFChars(stream, nullptr);
  const char *c = env->GetStringUTFChars(consumerId, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_queue_consumer_read(h, n, s, c, static_cast<uint32_t>(max), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(stream, s);
  env->ReleaseStringUTFChars(consumerId, c);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jbyteArray result = env->NewByteArray(static_cast<jsize>(len));
  if (result != nullptr && len > 0) {
    env->SetByteArrayRegion(result, 0, static_cast<jsize>(len), reinterpret_cast<const jbyte *>(ptr));
  }
  loom_bytes_free(ptr, len);
  return result;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeQueueConsumerAdvance(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring stream, jstring consumerId,
    jstring nextSeqText, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t nextSeq = 0;
  if (!parseU64String(env, nextSeqText, &nextSeq)) {
    return;
  }
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *s = env->GetStringUTFChars(stream, nullptr);
  const char *c = env->GetStringUTFChars(consumerId, nullptr);
  st = loom_queue_consumer_advance(h, n, s, c, nextSeq);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(stream, s);
  env->ReleaseStringUTFChars(consumerId, c);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeQueueConsumerReset(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring stream, jstring consumerId,
    jstring nextSeqText, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t nextSeq = 0;
  if (!parseU64String(env, nextSeqText, &nextSeq)) {
    return;
  }
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *s = env->GetStringUTFChars(stream, nullptr);
  const char *c = env->GetStringUTFChars(consumerId, nullptr);
  st = loom_queue_consumer_reset(h, n, s, c, nextSeq);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(stream, s);
  env->ReleaseStringUTFChars(consumerId, c);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

// As `openSessionKeyed`, but begins a held-open batch.
