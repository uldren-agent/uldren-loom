#include "UldrenLoom_jni.h"

#include <cstring>

static jobjectArray documentTextResult(JNIEnv *env, char *text, char *digest, char *entityTag) {
  jclass objectClass = env->FindClass("java/lang/Object");
  jobjectArray result = env->NewObjectArray(3, objectClass, nullptr);
  jstring textString = env->NewStringUTF(text != nullptr ? text : "");
  jstring digestString = env->NewStringUTF(digest != nullptr ? digest : "");
  jstring entityTagString = env->NewStringUTF(entityTag != nullptr ? entityTag : "");
  env->SetObjectArrayElement(result, 0, textString);
  env->SetObjectArrayElement(result, 1, digestString);
  env->SetObjectArrayElement(result, 2, entityTagString);
  if (text != nullptr) {
    loom_string_free(text);
  }
  if (digest != nullptr) {
    loom_string_free(digest);
  }
  if (entityTag != nullptr) {
    loom_string_free(entityTag);
  }
  return result;
}

static jobjectArray documentBinaryResult(JNIEnv *env, unsigned char *bytes, uintptr_t len, char *digest, char *entityTag) {
  jclass objectClass = env->FindClass("java/lang/Object");
  jobjectArray result = env->NewObjectArray(3, objectClass, nullptr);
  jbyteArray value = ownedBytes(env, bytes, len);
  jstring digestString = env->NewStringUTF(digest != nullptr ? digest : "");
  jstring entityTagString = env->NewStringUTF(entityTag != nullptr ? entityTag : "");
  env->SetObjectArrayElement(result, 0, value);
  env->SetObjectArrayElement(result, 1, digestString);
  env->SetObjectArrayElement(result, 2, entityTagString);
  if (digest != nullptr) {
    loom_string_free(digest);
  }
  if (entityTag != nullptr) {
    loom_string_free(entityTag);
  }
  return result;
}

static jobjectArray documentPutResult(JNIEnv *env, char *digest, char *entityTag) {
  jclass objectClass = env->FindClass("java/lang/Object");
  jobjectArray result = env->NewObjectArray(2, objectClass, nullptr);
  jstring digestString = env->NewStringUTF(digest != nullptr ? digest : "");
  jstring entityTagString = env->NewStringUTF(entityTag != nullptr ? entityTag : "");
  env->SetObjectArrayElement(result, 0, digestString);
  env->SetObjectArrayElement(result, 1, entityTagString);
  if (digest != nullptr) {
    loom_string_free(digest);
  }
  if (entityTag != nullptr) {
    loom_string_free(entityTag);
  }
  return result;
}

extern "C" JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocPutText(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring id,
    jstring text, jstring expectedEntityTag, jbyteArray passphrase, jbyteArray kek,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *i = env->GetStringUTFChars(id, nullptr);
  const char *t = env->GetStringUTFChars(text, nullptr);
  const char *expected = env->GetStringUTFChars(expectedEntityTag, nullptr);
  char *digest = nullptr;
  char *entityTag = nullptr;
  st = loom_doc_put_text(h, n, m, i, t, expected[0] == '\0' ? nullptr : expected, &digest, &entityTag);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(id, i);
  env->ReleaseStringUTFChars(text, t);
  env->ReleaseStringUTFChars(expectedEntityTag, expected);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return documentPutResult(env, digest, entityTag);
}

extern "C" JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocGetText(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring id,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *i = env->GetStringUTFChars(id, nullptr);
  char *text = nullptr;
  char *digest = nullptr;
  char *entityTag = nullptr;
  int32_t found = 0;
  st = loom_doc_get_text(h, n, m, i, &text, &digest, &entityTag, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(id, i);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  if (found == 0) {
    return nullptr;
  }
  return documentTextResult(env, text, digest, entityTag);
}

extern "C" JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocPutBinary(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring id,
    jbyteArray bytes, jstring expectedEntityTag, jbyteArray passphrase, jbyteArray kek,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *i = env->GetStringUTFChars(id, nullptr);
  const char *expected = env->GetStringUTFChars(expectedEntityTag, nullptr);
  jsize dlen = (bytes != nullptr) ? env->GetArrayLength(bytes) : 0;
  jbyte *d = (bytes != nullptr) ? env->GetByteArrayElements(bytes, nullptr) : nullptr;
  char *digest = nullptr;
  char *entityTag = nullptr;
  st = loom_doc_put_binary(h, n, m, i, reinterpret_cast<const unsigned char *>(d),
                           static_cast<uintptr_t>(dlen), expected[0] == '\0' ? nullptr : expected,
                           &digest, &entityTag);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(id, i);
  env->ReleaseStringUTFChars(expectedEntityTag, expected);
  if (d) {
    env->ReleaseByteArrayElements(bytes, d, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return documentPutResult(env, digest, entityTag);
}

extern "C" JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocGetBinary(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring id,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *i = env->GetStringUTFChars(id, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  char *digest = nullptr;
  char *entityTag = nullptr;
  int32_t found = 0;
  st = loom_doc_get_binary(h, n, m, i, &ptr, &len, &digest, &entityTag, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(id, i);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  if (found == 0) {
    return nullptr;
  }
  return documentBinaryResult(env, ptr, len, digest, entityTag);
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocDelete(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring id,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *i = env->GetStringUTFChars(id, nullptr);
  int32_t found = 0;
  st = loom_doc_delete(h, n, m, i, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(id, i);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocListBinary(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jbyteArray passphrase,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_doc_list_binary_cbor(h, n, m, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocIndexCreate(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring name,
    jstring fieldPath, jboolean unique, jbyteArray passphrase, jbyteArray kek,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *idx = env->GetStringUTFChars(name, nullptr);
  const char *path = env->GetStringUTFChars(fieldPath, nullptr);
  st = loom_doc_index_create(h, n, m, idx, path, unique ? 1 : 0);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(name, idx);
  env->ReleaseStringUTFChars(fieldPath, path);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocIndexCreateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection,
    jbyteArray declarationJson, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  jbyte *d = declarationJson != nullptr ? env->GetByteArrayElements(declarationJson, nullptr) : nullptr;
  jsize dLen = declarationJson != nullptr ? env->GetArrayLength(declarationJson) : 0;
  st = loom_doc_index_create_json(h, n, m, reinterpret_cast<const uint8_t *>(d),
                                  static_cast<uintptr_t>(dLen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  if (d != nullptr) {
    env->ReleaseByteArrayElements(declarationJson, d, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocIndexDrop(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring name,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *idx = env->GetStringUTFChars(name, nullptr);
  int32_t found = 0;
  st = loom_doc_index_drop(h, n, m, idx, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(name, idx);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return JNI_FALSE;
  }
  return found != 0 ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocIndexRebuild(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring name,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *idx = env->GetStringUTFChars(name, nullptr);
  st = loom_doc_index_rebuild(h, n, m, idx);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(name, idx);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocIndexListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_doc_index_list_json(h, n, m, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedJsonString(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocIndexStatusJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_doc_index_status_json(h, n, m, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedJsonString(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocFindJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring index,
    jstring valueJson, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *idx = env->GetStringUTFChars(index, nullptr);
  const char *value = env->GetStringUTFChars(valueJson, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_doc_find_json(h, n, m, idx, reinterpret_cast<const unsigned char *>(value),
                          static_cast<uintptr_t>(std::strlen(value)), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(index, idx);
  env->ReleaseStringUTFChars(valueJson, value);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedJsonString(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDocQueryJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring collection, jstring queryJson,
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
  const char *m = env->GetStringUTFChars(collection, nullptr);
  const char *query = env->GetStringUTFChars(queryJson, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_doc_query_json(h, n, m, reinterpret_cast<const unsigned char *>(query),
                           static_cast<uintptr_t>(std::strlen(query)), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(collection, m);
  env->ReleaseStringUTFChars(queryJson, query);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedJsonString(env, ptr, len);
}
