#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorCreate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jlong dim, jint metric,
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
  st = loom_vector_create(h, n, nm, static_cast<uintptr_t>(dim), static_cast<int32_t>(metric));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorUpsert(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring id,
    jbyteArray vector, jbyteArray metadata, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
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
  const char *i = env->GetStringUTFChars(id, nullptr);
  jsize vlen = (vector != nullptr) ? env->GetArrayLength(vector) : 0;
  jbyte *v = (vector != nullptr) ? env->GetByteArrayElements(vector, nullptr) : nullptr;
  jsize mlen = (metadata != nullptr) ? env->GetArrayLength(metadata) : 0;
  jbyte *m = (metadata != nullptr) ? env->GetByteArrayElements(metadata, nullptr) : nullptr;
  st = loom_vector_upsert(h, n, nm, i, reinterpret_cast<const unsigned char *>(v),
                          static_cast<uintptr_t>(vlen), reinterpret_cast<const unsigned char *>(m),
                          static_cast<uintptr_t>(mlen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  if (v) {
    env->ReleaseByteArrayElements(vector, v, JNI_ABORT);
  }
  if (m) {
    env->ReleaseByteArrayElements(metadata, m, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorUpsertSource(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring id,
    jbyteArray vector, jbyteArray metadata, jbyteArray sourceText, jstring modelId,
    jstring weightsDigest, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *i = env->GetStringUTFChars(id, nullptr);
  const char *model = modelId != nullptr ? env->GetStringUTFChars(modelId, nullptr) : nullptr;
  const char *weights =
      weightsDigest != nullptr ? env->GetStringUTFChars(weightsDigest, nullptr) : nullptr;
  jsize vlen = (vector != nullptr) ? env->GetArrayLength(vector) : 0;
  jbyte *v = (vector != nullptr) ? env->GetByteArrayElements(vector, nullptr) : nullptr;
  jsize mlen = (metadata != nullptr) ? env->GetArrayLength(metadata) : 0;
  jbyte *m = (metadata != nullptr) ? env->GetByteArrayElements(metadata, nullptr) : nullptr;
  jsize slen = (sourceText != nullptr) ? env->GetArrayLength(sourceText) : 0;
  jbyte *src = (sourceText != nullptr) ? env->GetByteArrayElements(sourceText, nullptr) : nullptr;
  st = loom_vector_upsert_source(
      h, n, nm, i, reinterpret_cast<const unsigned char *>(v), static_cast<uintptr_t>(vlen),
      reinterpret_cast<const unsigned char *>(m), static_cast<uintptr_t>(mlen),
      reinterpret_cast<const unsigned char *>(src), static_cast<uintptr_t>(slen), model,
      modelId != nullptr ? 1 : 0, weights, weightsDigest != nullptr ? 1 : 0);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  if (model) {
    env->ReleaseStringUTFChars(modelId, model);
  }
  if (weights) {
    env->ReleaseStringUTFChars(weightsDigest, weights);
  }
  if (v) {
    env->ReleaseByteArrayElements(vector, v, JNI_ABORT);
  }
  if (m) {
    env->ReleaseByteArrayElements(metadata, m, JNI_ABORT);
  }
  if (src) {
    env->ReleaseByteArrayElements(sourceText, src, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorGet(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring id,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *i = env->GetStringUTFChars(id, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_vector_get(h, n, nm, i, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorSourceText(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring id,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *i = env->GetStringUTFChars(id, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_vector_source_text(h, n, nm, i, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorEmbeddingModel(
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
  int32_t found = 0;
  st = loom_vector_embedding_model_cbor(h, n, nm, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorSearchPolicy(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray query, jlong k,
  jbyteArray filter, jint policy, jlong threshold, jlong ef, jlong pqM, jlong pqK, jlong pqIters,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  jsize qlen = (query != nullptr) ? env->GetArrayLength(query) : 0;
  jbyte *q = (query != nullptr) ? env->GetByteArrayElements(query, nullptr) : nullptr;
  jsize flen = (filter != nullptr) ? env->GetArrayLength(filter) : 0;
  jbyte *fl = (filter != nullptr) ? env->GetByteArrayElements(filter, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_vector_search_policy_cbor(
      h, n, nm, reinterpret_cast<const unsigned char *>(q), static_cast<uintptr_t>(qlen),
      static_cast<uintptr_t>(k), reinterpret_cast<const unsigned char *>(fl),
      static_cast<uintptr_t>(flen), static_cast<int32_t>(policy), static_cast<uintptr_t>(threshold),
      static_cast<uintptr_t>(ef), static_cast<uintptr_t>(pqM), static_cast<uintptr_t>(pqK),
      static_cast<uintptr_t>(pqIters), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (q) {
    env->ReleaseByteArrayElements(query, q, JNI_ABORT);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorIds(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring prefix,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *path = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, path, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, path);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *pf = (prefix != nullptr) ? env->GetStringUTFChars(prefix, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_vector_ids_cbor(h, n, nm, pf, prefix != nullptr ? 1 : 0, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (pf) {
    env->ReleaseStringUTFChars(prefix, pf);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorMetadataIndexKeys(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *path = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, path, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, path);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_vector_metadata_index_keys_cbor(h, n, nm, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorCreateMetadataIndex(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring key,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *path = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, path, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, path);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *k = env->GetStringUTFChars(key, nullptr);
  int32_t changed = 0;
  st = loom_vector_create_metadata_index(h, n, nm, k, &changed);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(key, k);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return changed != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorDropMetadataIndex(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring key,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *path = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, path, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, path);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *k = env->GetStringUTFChars(key, nullptr);
  int32_t changed = 0;
  st = loom_vector_drop_metadata_index(h, n, nm, k, &changed);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(key, k);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return changed != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorDelete(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring id,
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
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *i = env->GetStringUTFChars(id, nullptr);
  int32_t found = 0;
  st = loom_vector_delete(h, n, nm, i, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeVectorSearch(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jbyteArray query,
    jlong k, jbyteArray filter, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  jsize qlen = (query != nullptr) ? env->GetArrayLength(query) : 0;
  jbyte *q = (query != nullptr) ? env->GetByteArrayElements(query, nullptr) : nullptr;
  jsize flen = (filter != nullptr) ? env->GetArrayLength(filter) : 0;
  jbyte *fl = (filter != nullptr) ? env->GetByteArrayElements(filter, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_vector_search_cbor(h, n, nm, reinterpret_cast<const unsigned char *>(q),
                               static_cast<uintptr_t>(qlen), static_cast<uintptr_t>(k),
                               reinterpret_cast<const unsigned char *>(fl),
                               static_cast<uintptr_t>(flen), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  if (q) {
    env->ReleaseByteArrayElements(query, q, JNI_ABORT);
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
