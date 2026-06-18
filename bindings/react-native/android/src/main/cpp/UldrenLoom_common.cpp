#include "UldrenLoom_jni.h"

void throwLoom(JNIEnv *env) {
  int32_t code = 0;
  char *msg = nullptr;
  uintptr_t len = 0;
  loom_last_error(&code, &msg, &len);
  jclass ex = env->FindClass("java/lang/RuntimeException");
  env->ThrowNew(ex, msg ? msg : "loom error");
  if (msg) {
    loom_string_free(msg);
  }
}

void throwIllegalArgument(JNIEnv *env, const char *message) {
  jclass ex = env->FindClass("java/lang/IllegalArgumentException");
  env->ThrowNew(ex, message);
}

bool parseU64String(JNIEnv *env, jstring value, uint64_t *out) {
  if (value == nullptr) {
    throwIllegalArgument(env, "queue sequence must be an unsigned 64-bit decimal string");
    return false;
  }
  const char *chars = env->GetStringUTFChars(value, nullptr);
  if (chars == nullptr) {
    return false;
  }
  bool valid = chars[0] != '\0' && chars[0] != '+' && chars[0] != '-';
  for (const char *p = chars; valid && *p != '\0'; ++p) {
    valid = (*p >= '0' && *p <= '9');
  }
  errno = 0;
  char *end = nullptr;
  unsigned long long parsed = std::strtoull(chars, &end, 10);
  valid = valid && errno != ERANGE && end != nullptr && *end == '\0';
  env->ReleaseStringUTFChars(value, chars);
  if (!valid) {
    throwIllegalArgument(env, "queue sequence must be an unsigned 64-bit decimal string");
    return false;
  }
  *out = static_cast<uint64_t>(parsed);
  return true;
}

jstring u64String(JNIEnv *env, uint64_t value) {
  std::string text = std::to_string(value);
  return env->NewStringUTF(text.c_str());
}

int32_t openSessionKeyed(JNIEnv *env, const char *p, const char *n, const char *d,
                                jbyteArray passphrase, jbyteArray kek, LoomSqlSession **out) {
  jsize klen = (kek != nullptr) ? env->GetArrayLength(kek) : 0;
  jsize plen = (passphrase != nullptr) ? env->GetArrayLength(passphrase) : 0;
  if (klen > 0) {
    jbyte *k = env->GetByteArrayElements(kek, nullptr);
    int32_t st =
        loom_sql_open_with_kek(p, n, d, reinterpret_cast<const unsigned char *>(k),
                               static_cast<uintptr_t>(klen), out);
    env->ReleaseByteArrayElements(kek, k, JNI_ABORT);
    return st;
  }
  if (plen > 0) {
    jbyte *pass = env->GetByteArrayElements(passphrase, nullptr);
    int32_t st =
        loom_sql_open_keyed(p, n, d, reinterpret_cast<const unsigned char *>(pass),
                            static_cast<uintptr_t>(plen), out);
    env->ReleaseByteArrayElements(passphrase, pass, JNI_ABORT);
    return st;
  }
  return loom_sql_open(p, n, d, out);
}

int32_t openAuthenticatedSessionKeyed(JNIEnv *env, const char *p, const char *n, const char *d,
                                      jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
                                      jbyteArray authPassphrase, LoomSqlSession **out) {
  if (authPrincipal == nullptr || authPassphrase == nullptr ||
      env->GetArrayLength(authPassphrase) == 0) {
    return openSessionKeyed(env, p, n, d, passphrase, kek, out);
  }
  const char *principal = env->GetStringUTFChars(authPrincipal, nullptr);
  if (principal == nullptr || principal[0] == '\0') {
    if (principal != nullptr) {
      env->ReleaseStringUTFChars(authPrincipal, principal);
    }
    return openSessionKeyed(env, p, n, d, passphrase, kek, out);
  }
  jbyte *auth = env->GetByteArrayElements(authPassphrase, nullptr);
  jsize authLen = env->GetArrayLength(authPassphrase);
  jsize klen = (kek != nullptr) ? env->GetArrayLength(kek) : 0;
  jsize plen = (passphrase != nullptr) ? env->GetArrayLength(passphrase) : 0;
  int32_t st;
  if (klen > 0) {
    jbyte *k = env->GetByteArrayElements(kek, nullptr);
    st = loom_sql_open_with_kek_authenticated(
        p, n, d, reinterpret_cast<const unsigned char *>(k), static_cast<uintptr_t>(klen),
        principal, reinterpret_cast<const unsigned char *>(auth), static_cast<uintptr_t>(authLen),
        out);
    env->ReleaseByteArrayElements(kek, k, JNI_ABORT);
  } else if (plen > 0) {
    jbyte *pass = env->GetByteArrayElements(passphrase, nullptr);
    st = loom_sql_open_keyed_authenticated(
        p, n, d, reinterpret_cast<const unsigned char *>(pass), static_cast<uintptr_t>(plen),
        principal, reinterpret_cast<const unsigned char *>(auth), static_cast<uintptr_t>(authLen),
        out);
    env->ReleaseByteArrayElements(passphrase, pass, JNI_ABORT);
  } else {
    st = loom_sql_open_authenticated(
        p, n, d, principal, reinterpret_cast<const unsigned char *>(auth),
        static_cast<uintptr_t>(authLen), out);
  }
  env->ReleaseStringUTFChars(authPrincipal, principal);
  env->ReleaseByteArrayElements(authPassphrase, auth, JNI_ABORT);
  return st;
}

int32_t openStoreKeyed(JNIEnv *env, const char *p, jbyteArray passphrase, jbyteArray kek,
                              LoomSession **out) {
  jsize klen = (kek != nullptr) ? env->GetArrayLength(kek) : 0;
  jsize plen = (passphrase != nullptr) ? env->GetArrayLength(passphrase) : 0;
  if (klen > 0) {
    jbyte *k = env->GetByteArrayElements(kek, nullptr);
    int32_t st = loom_open_with_kek(p, reinterpret_cast<const unsigned char *>(k),
                                    static_cast<uintptr_t>(klen), out);
    env->ReleaseByteArrayElements(kek, k, JNI_ABORT);
    return st;
  }
  if (plen > 0) {
    jbyte *pass = env->GetByteArrayElements(passphrase, nullptr);
    int32_t st = loom_open_keyed(p, reinterpret_cast<const unsigned char *>(pass),
                                 static_cast<uintptr_t>(plen), out);
    env->ReleaseByteArrayElements(passphrase, pass, JNI_ABORT);
    return st;
  }
  return loom_open(p, out);
}

int32_t authenticateStore(JNIEnv *env, LoomSession *h, jstring principal, jbyteArray passphrase) {
  if (principal == nullptr || passphrase == nullptr || env->GetArrayLength(passphrase) == 0) {
    return 0;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  if (principalChars == nullptr || principalChars[0] == '\0') {
    if (principalChars != nullptr) {
      env->ReleaseStringUTFChars(principal, principalChars);
    }
    return 0;
  }
  jbyte *principalPass = env->GetByteArrayElements(passphrase, nullptr);
  jsize principalPassLen = env->GetArrayLength(passphrase);
  int32_t st = loom_authenticate_passphrase(
      h, principalChars, reinterpret_cast<const unsigned char *>(principalPass),
      static_cast<uintptr_t>(principalPassLen));
  env->ReleaseStringUTFChars(principal, principalChars);
  env->ReleaseByteArrayElements(passphrase, principalPass, JNI_ABORT);
  return st;
}

int32_t openAuthenticatedStoreKeyed(JNIEnv *env, const char *p, jbyteArray passphrase, jbyteArray kek,
                                    jstring authPrincipal, jbyteArray authPassphrase, LoomSession **out) {
  int32_t st = openStoreKeyed(env, p, passphrase, kek, out);
  if (st != 0 || *out == nullptr) {
    return st;
  }
  st = authenticateStore(env, *out, authPrincipal, authPassphrase);
  if (st != 0) {
    loom_close(*out);
    *out = nullptr;
  }
  return st;
}

jbyteArray ownedBytes(JNIEnv *env, unsigned char *ptr, uintptr_t len) {
  jbyteArray result = env->NewByteArray(static_cast<jsize>(len));
  if (result != nullptr && len > 0) {
    env->SetByteArrayRegion(result, 0, static_cast<jsize>(len),
                            reinterpret_cast<const jbyte *>(ptr));
  }
  loom_bytes_free(ptr, len);
  return result;
}

jstring ownedJsonString(JNIEnv *env, unsigned char *ptr, uintptr_t len) {
  std::string text;
  if (ptr != nullptr && len > 0) {
    text.assign(reinterpret_cast<const char *>(ptr), static_cast<size_t>(len));
  }
  if (ptr != nullptr) {
    loom_bytes_free(ptr, len);
  }
  return env->NewStringUTF(text.c_str());
}

int32_t beginBatchKeyed(JNIEnv *env, const char *p, const char *n, const char *d,
                               jbyteArray passphrase, jbyteArray kek, LoomSqlBatch **out) {
  jsize klen = (kek != nullptr) ? env->GetArrayLength(kek) : 0;
  jsize plen = (passphrase != nullptr) ? env->GetArrayLength(passphrase) : 0;
  if (klen > 0) {
    jbyte *k = env->GetByteArrayElements(kek, nullptr);
    int32_t st =
        loom_sql_batch_begin_with_kek(p, n, d, reinterpret_cast<const unsigned char *>(k),
                                      static_cast<uintptr_t>(klen), out);
    env->ReleaseByteArrayElements(kek, k, JNI_ABORT);
    return st;
  }
  if (plen > 0) {
    jbyte *pass = env->GetByteArrayElements(passphrase, nullptr);
    int32_t st =
        loom_sql_batch_begin_keyed(p, n, d, reinterpret_cast<const unsigned char *>(pass),
                                   static_cast<uintptr_t>(plen), out);
    env->ReleaseByteArrayElements(passphrase, pass, JNI_ABORT);
    return st;
  }
  return loom_sql_batch_begin(p, n, d, out);
}

int32_t beginAuthenticatedBatchKeyed(JNIEnv *env, const char *p, const char *n, const char *d,
                                     jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
                                     jbyteArray authPassphrase, LoomSqlBatch **out) {
  if (authPrincipal == nullptr || authPassphrase == nullptr ||
      env->GetArrayLength(authPassphrase) == 0) {
    return beginBatchKeyed(env, p, n, d, passphrase, kek, out);
  }
  const char *principal = env->GetStringUTFChars(authPrincipal, nullptr);
  if (principal == nullptr || principal[0] == '\0') {
    if (principal != nullptr) {
      env->ReleaseStringUTFChars(authPrincipal, principal);
    }
    return beginBatchKeyed(env, p, n, d, passphrase, kek, out);
  }
  jbyte *auth = env->GetByteArrayElements(authPassphrase, nullptr);
  jsize authLen = env->GetArrayLength(authPassphrase);
  jsize klen = (kek != nullptr) ? env->GetArrayLength(kek) : 0;
  jsize plen = (passphrase != nullptr) ? env->GetArrayLength(passphrase) : 0;
  int32_t st;
  if (klen > 0) {
    jbyte *k = env->GetByteArrayElements(kek, nullptr);
    st = loom_sql_batch_begin_with_kek_authenticated(
        p, n, d, reinterpret_cast<const unsigned char *>(k), static_cast<uintptr_t>(klen),
        principal, reinterpret_cast<const unsigned char *>(auth), static_cast<uintptr_t>(authLen),
        out);
    env->ReleaseByteArrayElements(kek, k, JNI_ABORT);
  } else if (plen > 0) {
    jbyte *pass = env->GetByteArrayElements(passphrase, nullptr);
    st = loom_sql_batch_begin_keyed_authenticated(
        p, n, d, reinterpret_cast<const unsigned char *>(pass), static_cast<uintptr_t>(plen),
        principal, reinterpret_cast<const unsigned char *>(auth), static_cast<uintptr_t>(authLen),
        out);
    env->ReleaseByteArrayElements(passphrase, pass, JNI_ABORT);
  } else {
    st = loom_sql_batch_begin_authenticated(
        p, n, d, principal, reinterpret_cast<const unsigned char *>(auth),
        static_cast<uintptr_t>(authLen), out);
  }
  env->ReleaseStringUTFChars(authPrincipal, principal);
  env->ReleaseByteArrayElements(authPassphrase, auth, JNI_ABORT);
  return st;
}

// Stateless one-shot SQL: open the loom (keyed if the store is encrypted), run, close.
