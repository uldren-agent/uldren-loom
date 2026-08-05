#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVersion(JNIEnv *env, jobject thiz) {
  (void)thiz;
  char *v = loom_version();
  jstring out = env->NewStringUTF(v ? v : "");
  if (v) {
    loom_string_free(v);
  }
  return out;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeBlobDigest(JNIEnv *env, jobject thiz,
                                                         jbyteArray data) {
  (void)thiz;
  jsize len = env->GetArrayLength(data);
  jbyte *buf = env->GetByteArrayElements(data, nullptr);
  char *d = loom_blob_digest(reinterpret_cast<const unsigned char *>(buf), static_cast<size_t>(len));
  env->ReleaseByteArrayElements(data, buf, JNI_ABORT);
  jstring out = env->NewStringUTF(d ? d : "");
  if (d) {
    loom_string_free(d);
  }
  return out;
}

// Create a fresh `.loom` under an identity profile, optionally encrypted under a passphrase.
// An empty `suite` (or empty `passphrase` byte[]) means profile-default / unencrypted.

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCreate(JNIEnv *env, jobject thiz, jstring loomPath,
                                                     jstring profile, jstring suite,
                                                     jbyteArray passphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *prof = env->GetStringUTFChars(profile, nullptr);
  const char *su = env->GetStringUTFChars(suite, nullptr);
  const char *suiteArg = (su && su[0]) ? su : nullptr;
  jbyte *pass = nullptr;
  jsize plen = 0;
  if (passphrase != nullptr) {
    plen = env->GetArrayLength(passphrase);
    pass = env->GetByteArrayElements(passphrase, nullptr);
  }
  int32_t st = loom_create(p, prof, suiteArg, reinterpret_cast<const unsigned char *>(pass),
                           static_cast<uintptr_t>(plen));
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(profile, prof);
  env->ReleaseStringUTFChars(suite, su);
  if (pass) {
    env->ReleaseByteArrayElements(passphrase, pass, JNI_ABORT);
  }
  if (st != 0) {
    throwLoom(env);
  }
}

// As `nativeCreate`, but wraps the DEK under a host-supplied 256-bit KEK.

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCreateWithKek(JNIEnv *env, jobject thiz,
                                                            jstring loomPath, jstring profile,
                                                            jstring suite, jbyteArray kek) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *prof = env->GetStringUTFChars(profile, nullptr);
  const char *su = env->GetStringUTFChars(suite, nullptr);
  const char *suiteArg = (su && su[0]) ? su : nullptr;
  jbyte *k = nullptr;
  jsize klen = 0;
  if (kek != nullptr) {
    klen = env->GetArrayLength(kek);
    k = env->GetByteArrayElements(kek, nullptr);
  }
  int32_t st = loom_create_with_kek(p, prof, suiteArg, reinterpret_cast<const unsigned char *>(k),
                                    static_cast<uintptr_t>(klen));
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(profile, prof);
  env->ReleaseStringUTFChars(suite, su);
  if (k) {
    env->ReleaseByteArrayElements(kek, k, JNI_ABORT);
  }
  if (st != 0) {
    throwLoom(env);
  }
}

// Open a session choosing the opener from the supplied key: a non-empty `kek`
// -> KEK unlock (the C ABI validates the 32-byte length); else a non-empty `passphrase` -> passphrase
// unlock; else the plain open. Returns the C status (0 = success, sets *out).

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeCapabilities(JNIEnv *env, jobject thiz) {
  (void)thiz;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  if (loom_capabilities(&ptr, &len) != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeRuntimeProfile(JNIEnv *env, jobject thiz) {
  (void)thiz;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  if (loom_runtime_profile(&ptr, &len) != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStudioSurfaceCatalogJson(
    JNIEnv *env, jobject thiz, jstring workspace, jstring set) {
  (void)thiz;
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *setChars = env->GetStringUTFChars(set, nullptr);
  char *out = nullptr;
  int32_t st = loom_studio_surface_catalog_json(workspaceChars, setChars, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(set, setChars);
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
extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeApplyCbor(
    JNIEnv *env, jobject thiz, jstring loomPath, jbyteArray request,
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
  jsize reqLen = env->GetArrayLength(request);
  jbyte *req = env->GetByteArrayElements(request, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_apply_cbor(
      h, reinterpret_cast<const unsigned char *>(req), static_cast<uintptr_t>(reqLen), &ptr, &len);
  env->ReleaseByteArrayElements(request, req, JNI_ABORT);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLifecycleDefineStandardJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring kind, jstring version,
    jstring completionPredicateDigest, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *kindChars = env->GetStringUTFChars(kind, nullptr);
  const char *versionChars = env->GetStringUTFChars(version, nullptr);
  const char *digestChars = env->GetStringUTFChars(completionPredicateDigest, nullptr);
  char *out = nullptr;
  st = loom_lifecycle_define_standard_json(
      h, workspaceChars, kindChars, versionChars, digestChars, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(kind, kindChars);
  env->ReleaseStringUTFChars(version, versionChars);
  env->ReleaseStringUTFChars(completionPredicateDigest, digestChars);
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

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLifecycleDefineJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jbyteArray definition,
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
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  jsize definitionLen = env->GetArrayLength(definition);
  jbyte *definitionBytes = env->GetByteArrayElements(definition, nullptr);
  char *out = nullptr;
  st = loom_lifecycle_define_json(
      h, workspaceChars, reinterpret_cast<const unsigned char *>(definitionBytes),
      static_cast<uintptr_t>(definitionLen), &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseByteArrayElements(definition, definitionBytes, JNI_ABORT);
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

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLifecycleInstantiateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring instanceId,
    jstring definitionId, jstring subjectRefsJson, jbyteArray passphrase, jbyteArray kek,
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
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *instanceChars = env->GetStringUTFChars(instanceId, nullptr);
  const char *definitionChars = env->GetStringUTFChars(definitionId, nullptr);
  const char *subjectsChars = env->GetStringUTFChars(subjectRefsJson, nullptr);
  char *out = nullptr;
  st = loom_lifecycle_instantiate_json(
      h, workspaceChars, instanceChars, definitionChars, subjectsChars, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(instanceId, instanceChars);
  env->ReleaseStringUTFChars(definitionId, definitionChars);
  env->ReleaseStringUTFChars(subjectRefsJson, subjectsChars);
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

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeLifecycleTransitionJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring instanceId,
    jstring transitionId, jstring toStageId, jstring actorPrincipalId, jstring gateEvaluationsJson,
    jstring snapshotDigest, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  const char *instanceChars = env->GetStringUTFChars(instanceId, nullptr);
  const char *transitionChars = env->GetStringUTFChars(transitionId, nullptr);
  const char *stageChars = env->GetStringUTFChars(toStageId, nullptr);
  const char *actorValue =
      actorPrincipalId ? env->GetStringUTFChars(actorPrincipalId, nullptr) : nullptr;
  const char *gateChars = env->GetStringUTFChars(gateEvaluationsJson, nullptr);
  const char *snapshotValue =
      snapshotDigest ? env->GetStringUTFChars(snapshotDigest, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_lifecycle_transition_json(
      h, workspaceChars, instanceChars, transitionChars, stageChars, actorValue, gateChars,
      snapshotValue, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
  env->ReleaseStringUTFChars(instanceId, instanceChars);
  env->ReleaseStringUTFChars(transitionId, transitionChars);
  env->ReleaseStringUTFChars(toStageId, stageChars);
  if (actorValue) {
    env->ReleaseStringUTFChars(actorPrincipalId, actorValue);
  }
  env->ReleaseStringUTFChars(gateEvaluationsJson, gateChars);
  if (snapshotValue) {
    env->ReleaseStringUTFChars(snapshotDigest, snapshotValue);
  }
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

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeRefsReconcileJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring workspace, jstring max, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t maxValue = 0;
  if (!parseU64String(env, max, &maxValue)) {
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
  const char *workspaceChars = env->GetStringUTFChars(workspace, nullptr);
  char *out = nullptr;
  st = loom_refs_reconcile_json(h, workspaceChars, maxValue, &out);
  env->ReleaseStringUTFChars(workspace, workspaceChars);
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
