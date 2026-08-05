#include "UldrenLoom_jni.h"

static jstring ownedString(JNIEnv *env, char *out) {
  jstring r = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return r;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAuditConfigShowJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray passphrase, jbyteArray kek,
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
  char *out = nullptr;
  st = loom_audit_config_show_json(h, &out);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAuditConfigSetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jdouble retentionDays, jboolean hasRetentionDays,
    jboolean legalHold, jboolean hasLegalHold, jbyteArray passphrase, jbyteArray kek,
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
  char *out = nullptr;
  st = loom_audit_config_set_json(h, (uint32_t)retentionDays, hasRetentionDays ? 1 : 0,
                                  legalHold ? 1 : 0, hasLegalHold ? 1 : 0, &out);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAuditListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray passphrase, jbyteArray kek,
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
  char *out = nullptr;
  st = loom_audit_list_json(h, &out);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAuditViewJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring record, jbyteArray passphrase,
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
  const char *r0 = env->GetStringUTFChars(record, nullptr);
  char *out = nullptr;
  st = loom_audit_view_json(h, r0, &out);
  env->ReleaseStringUTFChars(record, r0);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCertificateListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray passphrase, jbyteArray kek,
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
  char *out = nullptr;
  st = loom_certificate_list_json(h, &out);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCertificateImportJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jbyteArray certChainPem,
    jbyteArray privateKeyPem, jbyteArray trustBundlePem, jboolean hasTrustBundlePem,
    jboolean force, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  jbyte *cert = env->GetByteArrayElements(certChainPem, nullptr);
  jbyte *key = env->GetByteArrayElements(privateKeyPem, nullptr);
  jbyte *trust = hasTrustBundlePem ? env->GetByteArrayElements(trustBundlePem, nullptr) : nullptr;
  jsize certLen = env->GetArrayLength(certChainPem);
  jsize keyLen = env->GetArrayLength(privateKeyPem);
  jsize trustLen = hasTrustBundlePem ? env->GetArrayLength(trustBundlePem) : 0;
  unsigned char emptyTrust = 0;
  const unsigned char *trustArg = hasTrustBundlePem
                                      ? (trustLen == 0 ? &emptyTrust
                                                       : (const unsigned char *)trust)
                                      : nullptr;
  char *out = nullptr;
  st = loom_certificate_import_json(h, n, (const unsigned char *)cert, (uintptr_t)certLen,
                                    (const unsigned char *)key, (uintptr_t)keyLen,
                                    trustArg, (uintptr_t)trustLen, force ? 1 : 0, &out);
  env->ReleaseStringUTFChars(name, n);
  env->ReleaseByteArrayElements(certChainPem, cert, JNI_ABORT);
  env->ReleaseByteArrayElements(privateKeyPem, key, JNI_ABORT);
  if (trust != nullptr) {
    env->ReleaseByteArrayElements(trustBundlePem, trust, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCertificateExport(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jboolean includeCertChain,
    jboolean includePrivateKey, jboolean includeTrustBundle, jboolean force, jbyteArray passphrase,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_certificate_export(h, n, includeCertChain ? 1 : 0, includePrivateKey ? 1 : 0,
                               includeTrustBundle ? 1 : 0, force ? 1 : 0, &ptr, &len);
  env->ReleaseStringUTFChars(name, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCertificateGenerateSelfSignedJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jstring dnsNamesJson,
    jstring ipAddressesJson, jstring cn, jboolean hasCn, jdouble days, jstring algorithm,
    jboolean force, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  const char *dns = env->GetStringUTFChars(dnsNamesJson, nullptr);
  const char *ips = env->GetStringUTFChars(ipAddressesJson, nullptr);
  const char *cn0 = env->GetStringUTFChars(cn, nullptr);
  const char *cnArg = hasCn ? cn0 : nullptr;
  const char *alg = env->GetStringUTFChars(algorithm, nullptr);
  char *out = nullptr;
  st = loom_certificate_generate_self_signed_json(h, n, dns, ips, cnArg, (uint32_t)days, alg,
                                                  force ? 1 : 0, &out);
  env->ReleaseStringUTFChars(name, n);
  env->ReleaseStringUTFChars(dnsNamesJson, dns);
  env->ReleaseStringUTFChars(ipAddressesJson, ips);
  env->ReleaseStringUTFChars(cn, cn0);
  env->ReleaseStringUTFChars(algorithm, alg);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCertificateRemoveJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jbyteArray passphrase,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  st = loom_certificate_remove_json(h, n, &out);
  env->ReleaseStringUTFChars(name, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCertificateAuditJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jbyteArray passphrase,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  st = loom_certificate_audit_json(h, n, &out);
  env->ReleaseStringUTFChars(name, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeNetworkAccessListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray passphrase, jbyteArray kek,
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
  char *out = nullptr;
  st = loom_network_access_list_json(h, &out);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeNetworkAccessSetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jstring description,
    jboolean hasDescription, jstring defaultAction, jstring rulesJson, jbyteArray passphrase,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  const char *desc = env->GetStringUTFChars(description, nullptr);
  const char *descArg = hasDescription ? desc : nullptr;
  const char *action = env->GetStringUTFChars(defaultAction, nullptr);
  const char *rules = env->GetStringUTFChars(rulesJson, nullptr);
  char *out = nullptr;
  st = loom_network_access_set_json(h, n, descArg, action, rules, &out);
  env->ReleaseStringUTFChars(name, n);
  env->ReleaseStringUTFChars(description, desc);
  env->ReleaseStringUTFChars(defaultAction, action);
  env->ReleaseStringUTFChars(rulesJson, rules);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeNetworkAccessRemoveJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jbyteArray passphrase,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  st = loom_network_access_remove_json(h, n, &out);
  env->ReleaseStringUTFChars(name, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeNetworkAccessAuditJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring name, jbyteArray passphrase,
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
  const char *n = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  st = loom_network_access_audit_json(h, n, &out);
  env->ReleaseStringUTFChars(name, n);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedString(env, out);
}
