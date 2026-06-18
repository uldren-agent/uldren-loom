#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphUpsertNode(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring id,
    jbyteArray props, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  jsize plen = (props != nullptr) ? env->GetArrayLength(props) : 0;
  jbyte *pr = (props != nullptr) ? env->GetByteArrayElements(props, nullptr) : nullptr;
  st = loom_graph_upsert_node(h, n, nm, i, reinterpret_cast<const unsigned char *>(pr),
                              static_cast<uintptr_t>(plen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  if (pr) {
    env->ReleaseByteArrayElements(props, pr, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphGetNode(
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
  st = loom_graph_get_node(h, n, nm, i, &ptr, &len, &found);
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

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphRemoveNode(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring id,
    jboolean cascade, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  st = loom_graph_remove_node(h, n, nm, i, (cascade == JNI_TRUE) ? 1 : 0);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT void JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphUpsertEdge(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring id, jstring src,
    jstring dst, jstring label, jbyteArray props, jbyteArray passphrase, jbyteArray kek,
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
  const char *s = env->GetStringUTFChars(src, nullptr);
  const char *d = env->GetStringUTFChars(dst, nullptr);
  const char *l = env->GetStringUTFChars(label, nullptr);
  jsize plen = (props != nullptr) ? env->GetArrayLength(props) : 0;
  jbyte *pr = (props != nullptr) ? env->GetByteArrayElements(props, nullptr) : nullptr;
  st = loom_graph_upsert_edge(h, n, nm, i, s, d, l, reinterpret_cast<const unsigned char *>(pr),
                              static_cast<uintptr_t>(plen));
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  env->ReleaseStringUTFChars(src, s);
  env->ReleaseStringUTFChars(dst, d);
  env->ReleaseStringUTFChars(label, l);
  if (pr) {
    env->ReleaseByteArrayElements(props, pr, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
  }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphGetEdge(
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
  st = loom_graph_get_edge(h, n, nm, i, &ptr, &len, &found);
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

extern "C" JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphRemoveEdge(
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
  st = loom_graph_remove_edge(h, n, nm, i, &found);
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
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphNeighbors(
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
  st = loom_graph_neighbors_cbor(h, n, nm, i, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphOutEdges(
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
  st = loom_graph_out_edges_cbor(h, n, nm, i, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphInEdges(
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
  st = loom_graph_in_edges_cbor(h, n, nm, i, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(id, i);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphReachable(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring start,
    jlong maxDepth, jstring viaLabel, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *s = env->GetStringUTFChars(start, nullptr);
  const char *via = (viaLabel != nullptr) ? env->GetStringUTFChars(viaLabel, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_graph_reachable_cbor(h, n, nm, s, static_cast<int64_t>(maxDepth), via, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(start, s);
  if (via) {
    env->ReleaseStringUTFChars(viaLabel, via);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeGraphShortestPath(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring name, jstring from, jstring to,
    jstring viaLabel, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
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
  const char *f = env->GetStringUTFChars(from, nullptr);
  const char *t = env->GetStringUTFChars(to, nullptr);
  const char *via = (viaLabel != nullptr) ? env->GetStringUTFChars(viaLabel, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  int32_t found = 0;
  st = loom_graph_shortest_path_cbor(h, n, nm, f, t, via, &ptr, &len, &found);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(from, f);
  env->ReleaseStringUTFChars(to, t);
  if (via) {
    env->ReleaseStringUTFChars(viaLabel, via);
  }
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
