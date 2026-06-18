#include "UldrenLoom_jni.h"

static int32_t openTelemetry(JNIEnv *env, jstring loomPath, jbyteArray passphrase, jbyteArray kek,
                             jstring authPrincipal, jbyteArray authPassphrase, LoomSession **out) {
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, out);
  env->ReleaseStringUTFChars(loomPath, p);
  return st;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMetricsPutDescriptor(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jbyteArray descriptor,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  jsize len = descriptor ? env->GetArrayLength(descriptor) : 0;
  jbyte *bytes = descriptor ? env->GetByteArrayElements(descriptor, nullptr) : nullptr;
  st = loom_metrics_put_descriptor(h, n, reinterpret_cast<const unsigned char *>(bytes),
                                   static_cast<uintptr_t>(len));
  env->ReleaseStringUTFChars(ns, n);
  if (bytes) env->ReleaseByteArrayElements(descriptor, bytes, JNI_ABORT);
  loom_close(h);
  if (st != 0) throwLoom(env);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMetricsGetDescriptor(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *m = env->GetStringUTFChars(name, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_metrics_get_descriptor(h, n, m, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return found ? ownedBytes(env, ptr, len) : nullptr;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMetricsPutObservation(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring descriptorName,
    jbyteArray observation, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *d = env->GetStringUTFChars(descriptorName, nullptr);
  jsize len = observation ? env->GetArrayLength(observation) : 0;
  jbyte *bytes = observation ? env->GetByteArrayElements(observation, nullptr) : nullptr;
  st = loom_metrics_put_observation(h, n, d, reinterpret_cast<const unsigned char *>(bytes),
                                    static_cast<uintptr_t>(len));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(descriptorName, d);
  if (bytes) env->ReleaseByteArrayElements(observation, bytes, JNI_ABORT);
  loom_close(h);
  if (st != 0) throwLoom(env);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeMetricsQuery(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring descriptorName,
    jstring fromTimestampMs, jstring toTimestampMs, jdouble maxSeries, jdouble maxGroups,
    jdouble maxSamples, jstring maxOutputBytes, jstring nowTimestampMs, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t from = 0, to = 0, maxBytes = 0, now = 0;
  if (!parseU64String(env, fromTimestampMs, &from) || !parseU64String(env, toTimestampMs, &to) ||
      !parseU64String(env, maxOutputBytes, &maxBytes) || !parseU64String(env, nowTimestampMs, &now)) {
    return nullptr;
  }
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *d = env->GetStringUTFChars(descriptorName, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_metrics_query_cbor(h, n, d, from, to, static_cast<uint32_t>(maxSeries),
                               static_cast<uint32_t>(maxGroups), static_cast<uint32_t>(maxSamples),
                               maxBytes, now, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(descriptorName, d);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLogsPutRecord(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jbyteArray record,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  jsize len = record ? env->GetArrayLength(record) : 0;
  jbyte *bytes = record ? env->GetByteArrayElements(record, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t outLen = 0;
  st = loom_logs_put_record(h, n, reinterpret_cast<const unsigned char *>(bytes),
                            static_cast<uintptr_t>(len), &ptr, &outLen);
  env->ReleaseStringUTFChars(ns, n);
  if (bytes) env->ReleaseByteArrayElements(record, bytes, JNI_ABORT);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  std::string out(reinterpret_cast<char *>(ptr), static_cast<size_t>(outLen));
  loom_bytes_free(ptr, outLen);
  return env->NewStringUTF(out.c_str());
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLogsGetRecord(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring recordId, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *r = env->GetStringUTFChars(recordId, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_logs_get_record(h, n, r, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(recordId, r);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return found ? ownedBytes(env, ptr, len) : nullptr;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLogsQuery(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring fromTimeUnixNano,
    jstring toTimeUnixNano, jdouble maxRecords, jstring maxOutputBytes, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t from = 0, to = 0, maxBytes = 0;
  if (!parseU64String(env, fromTimeUnixNano, &from) || !parseU64String(env, toTimeUnixNano, &to) ||
      !parseU64String(env, maxOutputBytes, &maxBytes)) {
    return nullptr;
  }
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_logs_query_cbor(h, n, from, to, static_cast<uint32_t>(maxRecords), maxBytes, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTracesPutSpan(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jbyteArray span, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  jsize len = span ? env->GetArrayLength(span) : 0;
  jbyte *bytes = span ? env->GetByteArrayElements(span, nullptr) : nullptr;
  st = loom_traces_put_span(h, n, reinterpret_cast<const unsigned char *>(bytes),
                            static_cast<uintptr_t>(len));
  env->ReleaseStringUTFChars(ns, n);
  if (bytes) env->ReleaseByteArrayElements(span, bytes, JNI_ABORT);
  loom_close(h);
  if (st != 0) throwLoom(env);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTracesGetSpan(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring traceId, jstring spanId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *t = env->GetStringUTFChars(traceId, nullptr);
  const char *s = env->GetStringUTFChars(spanId, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_traces_get_span(h, n, t, s, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(traceId, t);
  env->ReleaseStringUTFChars(spanId, s);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return found ? ownedBytes(env, ptr, len) : nullptr;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTracesTraceSpans(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring traceId, jdouble maxSpans,
    jstring maxOutputBytes, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t maxBytes = 0;
  if (!parseU64String(env, maxOutputBytes, &maxBytes)) return nullptr;
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *t = env->GetStringUTFChars(traceId, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_traces_trace_spans_cbor(h, n, t, static_cast<uint32_t>(maxSpans), maxBytes, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(traceId, t);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTracesQuery(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring fromStartTimeNs,
    jstring toStartTimeNs, jdouble maxSpans, jstring maxOutputBytes, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t from = 0, to = 0, maxBytes = 0;
  if (!parseU64String(env, fromStartTimeNs, &from) || !parseU64String(env, toStartTimeNs, &to) ||
      !parseU64String(env, maxOutputBytes, &maxBytes)) {
    return nullptr;
  }
  LoomSession *h = nullptr;
  int32_t st = openTelemetry(env, loomPath, passphrase, kek, authPrincipal, authPassphrase, &h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_traces_query_cbor(h, n, from, to, static_cast<uint32_t>(maxSpans), maxBytes, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}
