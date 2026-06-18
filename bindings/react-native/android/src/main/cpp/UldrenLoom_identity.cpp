#include "UldrenLoom_jni.h"

struct RnAclScopes {
  jsize count = 0;
  jint *kinds = nullptr;
  std::vector<jbyteArray> arrays;
  std::vector<jbyte *> bytes;
  std::vector<const unsigned char *> prefixes;
  std::vector<uintptr_t> lengths;
};

static bool aclScopesAcquire(JNIEnv *env, jintArray scopeKinds, jobjectArray scopePrefixes,
                             RnAclScopes *out) {
  out->count = scopeKinds ? env->GetArrayLength(scopeKinds) : 0;
  jsize prefixCount = scopePrefixes ? env->GetArrayLength(scopePrefixes) : 0;
  if (out->count != prefixCount) {
    throwIllegalArgument(env, "scope kind and prefix counts differ");
    return false;
  }
  if (out->count == 0) {
    return true;
  }
  out->kinds = env->GetIntArrayElements(scopeKinds, nullptr);
  if (!out->kinds) {
    return false;
  }
  out->arrays.resize(static_cast<size_t>(out->count));
  out->bytes.resize(static_cast<size_t>(out->count));
  out->prefixes.resize(static_cast<size_t>(out->count));
  out->lengths.resize(static_cast<size_t>(out->count));
  for (jsize i = 0; i < out->count; i++) {
    out->arrays[static_cast<size_t>(i)] =
        static_cast<jbyteArray>(env->GetObjectArrayElement(scopePrefixes, i));
    if (!out->arrays[static_cast<size_t>(i)]) {
      throwIllegalArgument(env, "scope prefix is null");
      return false;
    }
    out->lengths[static_cast<size_t>(i)] =
        static_cast<uintptr_t>(env->GetArrayLength(out->arrays[static_cast<size_t>(i)]));
    out->bytes[static_cast<size_t>(i)] =
        env->GetByteArrayElements(out->arrays[static_cast<size_t>(i)], nullptr);
    if (!out->bytes[static_cast<size_t>(i)]) {
      return false;
    }
    out->prefixes[static_cast<size_t>(i)] =
        reinterpret_cast<const unsigned char *>(out->bytes[static_cast<size_t>(i)]);
  }
  return true;
}

static void aclScopesRelease(JNIEnv *env, jintArray scopeKinds, RnAclScopes *scopes) {
  for (size_t i = 0; i < scopes->arrays.size(); i++) {
    if (scopes->arrays[i]) {
      if (i < scopes->bytes.size() && scopes->bytes[i]) {
        env->ReleaseByteArrayElements(scopes->arrays[i], scopes->bytes[i], JNI_ABORT);
      }
      env->DeleteLocalRef(scopes->arrays[i]);
    }
  }
  if (scopes->kinds) {
    env->ReleaseIntArrayElements(scopeKinds, scopes->kinds, JNI_ABORT);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAuthenticatePassphrase(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principal, jbyteArray principalPassphrase,
    jbyteArray passphrase, jbyteArray kek) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  jbyte *principalPass = env->GetByteArrayElements(principalPassphrase, nullptr);
  jsize principalPassLen = env->GetArrayLength(principalPassphrase);
  st = loom_authenticate_passphrase(h, principalChars,
                                    reinterpret_cast<const unsigned char *>(principalPass),
                                    static_cast<uintptr_t>(principalPassLen));
  env->ReleaseStringUTFChars(principal, principalChars);
  env->ReleaseByteArrayElements(principalPassphrase, principalPass, JNI_ABORT);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  char *out = nullptr;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_list_json(h, &out);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "{}");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityAddPrincipal(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principalHandle, jstring name, jstring kind, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *handleChars = env->GetStringUTFChars(principalHandle, nullptr);
  const char *nameChars = env->GetStringUTFChars(name, nullptr);
  const char *kindChars = env->GetStringUTFChars(kind, nullptr);
  char *out = nullptr;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_add_principal(h, handleChars, nameChars, kindChars, &out);
  }
  env->ReleaseStringUTFChars(principalHandle, handleChars);
  env->ReleaseStringUTFChars(name, nameChars);
  env->ReleaseStringUTFChars(kind, kindChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityRenamePrincipalHandle(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principal, jstring principalHandle,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *pathChars = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, pathChars, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, pathChars);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  const char *handleChars = env->GetStringUTFChars(principalHandle, nullptr);
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_rename_principal_handle(h, principalChars, handleChars);
  }
  env->ReleaseStringUTFChars(principal, principalChars);
  env->ReleaseStringUTFChars(principalHandle, handleChars);
  loom_close(h);
  if (st != 0) throwLoom(env);
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentitySetPassphrase(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principal, jbyteArray principalPassphrase,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  jbyte *principalPass = env->GetByteArrayElements(principalPassphrase, nullptr);
  jsize principalPassLen = env->GetArrayLength(principalPassphrase);
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_set_passphrase(h, principalChars,
                                      reinterpret_cast<const unsigned char *>(principalPass),
                                      static_cast<uintptr_t>(principalPassLen));
  }
  env->ReleaseStringUTFChars(principal, principalChars);
  env->ReleaseByteArrayElements(principalPassphrase, principalPass, JNI_ABORT);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityRemovePrincipal(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principal, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_remove_principal(h, principalChars);
  }
  env->ReleaseStringUTFChars(principal, principalChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityAssignRole(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principal, jstring role,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  const char *roleChars = env->GetStringUTFChars(role, nullptr);
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_assign_role(h, principalChars, roleChars);
  }
  env->ReleaseStringUTFChars(principal, principalChars);
  env->ReleaseStringUTFChars(role, roleChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityRevokeRole(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principal, jstring role,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  const char *roleChars = env->GetStringUTFChars(role, nullptr);
  int32_t removed = 0;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_revoke_role(h, principalChars, roleChars, &removed);
  }
  env->ReleaseStringUTFChars(principal, principalChars);
  env->ReleaseStringUTFChars(role, roleChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return removed ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityCreateExternalCredential(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principal, jstring kind, jstring label,
    jstring issuer, jstring subject, jstring materialDigest, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  const char *kindChars = env->GetStringUTFChars(kind, nullptr);
  const char *labelChars = env->GetStringUTFChars(label, nullptr);
  const char *issuerChars = env->GetStringUTFChars(issuer, nullptr);
  const char *subjectChars = env->GetStringUTFChars(subject, nullptr);
  const char *materialDigestChars = env->GetStringUTFChars(materialDigest, nullptr);
  char *out = nullptr;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    const char *digest = materialDigestChars[0] ? materialDigestChars : nullptr;
    st = loom_identity_create_external_credential(
        h, principalChars, kindChars, labelChars, issuerChars, subjectChars, digest, &out);
  }
  env->ReleaseStringUTFChars(principal, principalChars);
  env->ReleaseStringUTFChars(kind, kindChars);
  env->ReleaseStringUTFChars(label, labelChars);
  env->ReleaseStringUTFChars(issuer, issuerChars);
  env->ReleaseStringUTFChars(subject, subjectChars);
  env->ReleaseStringUTFChars(materialDigest, materialDigestChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityRevokeExternalCredential(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring credential, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *credentialChars = env->GetStringUTFChars(credential, nullptr);
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_revoke_external_credential(h, credentialChars);
  }
  env->ReleaseStringUTFChars(credential, credentialChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityAddPublicKey(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring principal, jstring label,
    jstring algorithm, jstring publicKeyHex, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *principalChars = env->GetStringUTFChars(principal, nullptr);
  const char *labelChars = env->GetStringUTFChars(label, nullptr);
  const char *algorithmChars = env->GetStringUTFChars(algorithm, nullptr);
  const char *keyChars = env->GetStringUTFChars(publicKeyHex, nullptr);
  char *out = nullptr;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_add_public_key(h, principalChars, labelChars, algorithmChars, keyChars, &out);
  }
  env->ReleaseStringUTFChars(principal, principalChars);
  env->ReleaseStringUTFChars(label, labelChars);
  env->ReleaseStringUTFChars(algorithm, algorithmChars);
  env->ReleaseStringUTFChars(publicKeyHex, keyChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeIdentityRevokePublicKey(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring key, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *keyChars = env->GetStringUTFChars(key, nullptr);
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_identity_revoke_public_key(h, keyChars);
  }
  env->ReleaseStringUTFChars(key, keyChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAclListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  char *out = nullptr;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_acl_list_json(h, &out);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "[]");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAclGrant(
    JNIEnv *env, jobject thiz, jstring loomPath, jdouble effect, jstring subject, jstring workspace_,
    jstring domain, jdouble rightsMask, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *subjectChars = env->GetStringUTFChars(subject, nullptr);
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *facetChars = env->GetStringUTFChars(domain, nullptr);
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_acl_grant(h, static_cast<int32_t>(effect), subjectChars,
                        workspaceChars && workspaceChars[0] ? workspaceChars : nullptr,
                        facetChars && facetChars[0] ? facetChars : nullptr,
                        static_cast<uint32_t>(rightsMask));
  }
  env->ReleaseStringUTFChars(subject, subjectChars);
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(domain, facetChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAclGrantScoped(
    JNIEnv *env, jobject thiz, jstring loomPath, jdouble effect, jstring subject, jstring workspace_,
    jstring domain, jdouble rightsMask, jstring refGlob, jintArray scopeKinds, jobjectArray scopePrefixes,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *subjectChars = env->GetStringUTFChars(subject, nullptr);
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *facetChars = env->GetStringUTFChars(domain, nullptr);
  const char *refGlobChars = env->GetStringUTFChars(refGlob, nullptr);
  RnAclScopes scopes;
  if (!aclScopesAcquire(env, scopeKinds, scopePrefixes, &scopes)) {
    aclScopesRelease(env, scopeKinds, &scopes);
    env->ReleaseStringUTFChars(subject, subjectChars);
    env->ReleaseStringUTFChars(workspace_, workspaceChars);
    env->ReleaseStringUTFChars(domain, facetChars);
    env->ReleaseStringUTFChars(refGlob, refGlobChars);
    loom_close(h);
    return;
  }
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_acl_grant_scoped(
        h, static_cast<int32_t>(effect), subjectChars,
        workspaceChars && workspaceChars[0] ? workspaceChars : nullptr,
        facetChars && facetChars[0] ? facetChars : nullptr,
        static_cast<uint32_t>(rightsMask),
        refGlobChars && refGlobChars[0] ? refGlobChars : nullptr,
        static_cast<uintptr_t>(scopes.count), scopes.kinds, scopes.prefixes.data(),
        scopes.lengths.data());
  }
  aclScopesRelease(env, scopeKinds, &scopes);
  env->ReleaseStringUTFChars(subject, subjectChars);
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(domain, facetChars);
  env->ReleaseStringUTFChars(refGlob, refGlobChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAclGrantScopedPredicate(
    JNIEnv *env, jobject thiz, jstring loomPath, jdouble effect, jstring subject, jstring workspace_,
    jstring domain, jdouble rightsMask, jstring refGlob, jintArray scopeKinds, jobjectArray scopePrefixes,
    jstring predicateCel, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *subjectChars = env->GetStringUTFChars(subject, nullptr);
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *facetChars = env->GetStringUTFChars(domain, nullptr);
  const char *refGlobChars = env->GetStringUTFChars(refGlob, nullptr);
  const char *predicateChars = env->GetStringUTFChars(predicateCel, nullptr);
  RnAclScopes scopes;
  if (!aclScopesAcquire(env, scopeKinds, scopePrefixes, &scopes)) {
    aclScopesRelease(env, scopeKinds, &scopes);
    env->ReleaseStringUTFChars(subject, subjectChars);
    env->ReleaseStringUTFChars(workspace_, workspaceChars);
    env->ReleaseStringUTFChars(domain, facetChars);
    env->ReleaseStringUTFChars(refGlob, refGlobChars);
    env->ReleaseStringUTFChars(predicateCel, predicateChars);
    loom_close(h);
    return;
  }
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_acl_grant_scoped_predicate(
        h, static_cast<int32_t>(effect), subjectChars,
        workspaceChars && workspaceChars[0] ? workspaceChars : nullptr,
        facetChars && facetChars[0] ? facetChars : nullptr,
        static_cast<uint32_t>(rightsMask),
        refGlobChars && refGlobChars[0] ? refGlobChars : nullptr,
        static_cast<uintptr_t>(scopes.count), scopes.kinds, scopes.prefixes.data(),
        scopes.lengths.data(), predicateChars && predicateChars[0] ? "cel" : nullptr,
        predicateChars && predicateChars[0] ? predicateChars : nullptr);
  }
  aclScopesRelease(env, scopeKinds, &scopes);
  env->ReleaseStringUTFChars(subject, subjectChars);
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(domain, facetChars);
  env->ReleaseStringUTFChars(refGlob, refGlobChars);
  env->ReleaseStringUTFChars(predicateCel, predicateChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAclRevoke(
    JNIEnv *env, jobject thiz, jstring loomPath, jdouble effect, jstring subject, jstring workspace_,
    jstring domain, jdouble rightsMask, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  const char *subjectChars = env->GetStringUTFChars(subject, nullptr);
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *facetChars = env->GetStringUTFChars(domain, nullptr);
  int32_t removed = 0;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_acl_revoke(h, static_cast<int32_t>(effect), subjectChars,
                         workspaceChars && workspaceChars[0] ? workspaceChars : nullptr,
                         facetChars && facetChars[0] ? facetChars : nullptr,
                         static_cast<uint32_t>(rightsMask), &removed);
  }
  env->ReleaseStringUTFChars(subject, subjectChars);
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(domain, facetChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return removed ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAclRevokeScoped(
    JNIEnv *env, jobject thiz, jstring loomPath, jdouble effect, jstring subject, jstring workspace_,
    jstring domain, jdouble rightsMask, jstring refGlob, jintArray scopeKinds, jobjectArray scopePrefixes,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  const char *subjectChars = env->GetStringUTFChars(subject, nullptr);
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *facetChars = env->GetStringUTFChars(domain, nullptr);
  const char *refGlobChars = env->GetStringUTFChars(refGlob, nullptr);
  RnAclScopes scopes;
  if (!aclScopesAcquire(env, scopeKinds, scopePrefixes, &scopes)) {
    aclScopesRelease(env, scopeKinds, &scopes);
    env->ReleaseStringUTFChars(subject, subjectChars);
    env->ReleaseStringUTFChars(workspace_, workspaceChars);
    env->ReleaseStringUTFChars(domain, facetChars);
    env->ReleaseStringUTFChars(refGlob, refGlobChars);
    loom_close(h);
    return JNI_FALSE;
  }
  int32_t removed = 0;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_acl_revoke_scoped(
        h, static_cast<int32_t>(effect), subjectChars,
        workspaceChars && workspaceChars[0] ? workspaceChars : nullptr,
        facetChars && facetChars[0] ? facetChars : nullptr,
        static_cast<uint32_t>(rightsMask),
        refGlobChars && refGlobChars[0] ? refGlobChars : nullptr,
        static_cast<uintptr_t>(scopes.count), scopes.kinds, scopes.prefixes.data(),
        scopes.lengths.data(), &removed);
  }
  aclScopesRelease(env, scopeKinds, &scopes);
  env->ReleaseStringUTFChars(subject, subjectChars);
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(domain, facetChars);
  env->ReleaseStringUTFChars(refGlob, refGlobChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return removed ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeAclRevokeScopedPredicate(
    JNIEnv *env, jobject thiz, jstring loomPath, jdouble effect, jstring subject, jstring workspace_,
    jstring domain, jdouble rightsMask, jstring refGlob, jintArray scopeKinds, jobjectArray scopePrefixes,
    jstring predicateCel, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  const char *subjectChars = env->GetStringUTFChars(subject, nullptr);
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *facetChars = env->GetStringUTFChars(domain, nullptr);
  const char *refGlobChars = env->GetStringUTFChars(refGlob, nullptr);
  const char *predicateChars = env->GetStringUTFChars(predicateCel, nullptr);
  RnAclScopes scopes;
  if (!aclScopesAcquire(env, scopeKinds, scopePrefixes, &scopes)) {
    aclScopesRelease(env, scopeKinds, &scopes);
    env->ReleaseStringUTFChars(subject, subjectChars);
    env->ReleaseStringUTFChars(workspace_, workspaceChars);
    env->ReleaseStringUTFChars(domain, facetChars);
    env->ReleaseStringUTFChars(refGlob, refGlobChars);
    env->ReleaseStringUTFChars(predicateCel, predicateChars);
    loom_close(h);
    return JNI_FALSE;
  }
  int32_t removed = 0;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_acl_revoke_scoped_predicate(
        h, static_cast<int32_t>(effect), subjectChars,
        workspaceChars && workspaceChars[0] ? workspaceChars : nullptr,
        facetChars && facetChars[0] ? facetChars : nullptr,
        static_cast<uint32_t>(rightsMask),
        refGlobChars && refGlobChars[0] ? refGlobChars : nullptr,
        static_cast<uintptr_t>(scopes.count), scopes.kinds, scopes.prefixes.data(),
        scopes.lengths.data(), predicateChars && predicateChars[0] ? "cel" : nullptr,
        predicateChars && predicateChars[0] ? predicateChars : nullptr, &removed);
  }
  aclScopesRelease(env, scopeKinds, &scopes);
  env->ReleaseStringUTFChars(subject, subjectChars);
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(domain, facetChars);
  env->ReleaseStringUTFChars(refGlob, refGlobChars);
  env->ReleaseStringUTFChars(predicateCel, predicateChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return removed ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeProtectedRefListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace_, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  char *out = nullptr;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_protected_ref_list_json(h, workspaceChars, &out);
  }
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "[]");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeProtectedRefGetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace_, jstring refName,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *refChars = env->GetStringUTFChars(refName, nullptr);
  char *out = nullptr;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_protected_ref_get_json(h, workspaceChars, refChars, &out);
  }
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(refName, refChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "null");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeProtectedRefSet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace_, jstring refName,
    jboolean fastForwardOnly, jboolean signedCommitsRequired, jboolean signedRefAdvanceRequired,
    jdouble requiredReviewCount, jboolean retentionLock, jboolean governanceLock,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *refChars = env->GetStringUTFChars(refName, nullptr);
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_protected_ref_set(
        h, workspaceChars, refChars, fastForwardOnly == JNI_TRUE,
        signedCommitsRequired == JNI_TRUE, signedRefAdvanceRequired == JNI_TRUE,
        static_cast<uint32_t>(requiredReviewCount), retentionLock == JNI_TRUE,
        governanceLock == JNI_TRUE);
  }
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(refName, refChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeProtectedRefRemove(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace_, jstring refName,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openStoreKeyed(env, p, passphrase, kek, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  const char *workspaceChars = env->GetStringUTFChars(workspace_, nullptr);
  const char *refChars = env->GetStringUTFChars(refName, nullptr);
  int32_t removed = 0;
  st = authenticateStore(env, h, authPrincipal, authPassphrase);
  if (st == 0) {
    st = loom_protected_ref_remove(h, workspaceChars, refChars, &removed);
  }
  env->ReleaseStringUTFChars(workspace_, workspaceChars);
  env->ReleaseStringUTFChars(refName, refChars);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return removed ? JNI_TRUE : JNI_FALSE;
}
