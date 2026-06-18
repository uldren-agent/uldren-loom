#include "UldrenLoom_jni.h"

static jstring finishPagesString(JNIEnv *env, LoomSession *h, int32_t st, char *out) {
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

#define PAGES_OPEN() \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { throwLoom(env); return nullptr; } \
  const char *n = env->GetStringUTFChars(ns, nullptr); \
  const char *pw = env->GetStringUTFChars(pageWorkspaceId, nullptr)

#define PAGES_RELEASE_NS() \
  env->ReleaseStringUTFChars(ns, n); \
  env->ReleaseStringUTFChars(pageWorkspaceId, pw)

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSpacesCreateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring spaceId, jstring title, jstring expectedRoot, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *space = env->GetStringUTFChars(spaceId, nullptr);
  const char *ttl = env->GetStringUTFChars(title, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_spaces_create_json(h, n, pw, space, ttl, root, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(spaceId, space);
  env->ReleaseStringUTFChars(title, ttl);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSpacesListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  char *out = nullptr;
  st = loom_spaces_list_json(h, n, pw, &out);
  PAGES_RELEASE_NS();
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSpacesGetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring spaceId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *space = env->GetStringUTFChars(spaceId, nullptr);
  char *out = nullptr;
  st = loom_spaces_get_json(h, n, pw, space, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(spaceId, space);
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativePagesCreateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring pageId, jstring spaceId, jstring parentPageId, jstring title, jstring expectedRoot,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *page = env->GetStringUTFChars(pageId, nullptr);
  const char *space = env->GetStringUTFChars(spaceId, nullptr);
  const char *parent = env->GetStringUTFChars(parentPageId, nullptr);
  const char *ttl = env->GetStringUTFChars(title, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_pages_create_json(h, n, pw, page, space, parent, ttl, root, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(pageId, page);
  env->ReleaseStringUTFChars(spaceId, space);
  env->ReleaseStringUTFChars(parentPageId, parent);
  env->ReleaseStringUTFChars(title, ttl);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativePagesUpdateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring pageId, jstring bodyText, jstring expectedRoot, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *page = env->GetStringUTFChars(pageId, nullptr);
  const char *body = env->GetStringUTFChars(bodyText, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_pages_update_json(h, n, pw, page, body, root, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(pageId, page);
  env->ReleaseStringUTFChars(bodyText, body);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativePagesPublishJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring pageId, jstring expectedRoot, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *page = env->GetStringUTFChars(pageId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_pages_publish_json(h, n, pw, page, root, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(pageId, page);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishPagesString(env, h, st, out);
}

#define PAGE_READ(java_name, c_name, arg_name) \
extern "C" JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##java_name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId, \
    jstring arg_name, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, \
    jbyteArray authPassphrase) { \
  (void)thiz; \
  PAGES_OPEN(); \
  const char *arg = env->GetStringUTFChars(arg_name, nullptr); \
  char *out = nullptr; \
  st = c_name(h, n, pw, arg, &out); \
  PAGES_RELEASE_NS(); \
  env->ReleaseStringUTFChars(arg_name, arg); \
  return finishPagesString(env, h, st, out); \
}

PAGE_READ(nativePagesGetJson, loom_pages_get_json, pageId)
PAGE_READ(nativePagesHistoryJson, loom_pages_history_json, pageId)
PAGE_READ(nativeStructuresGetJson, loom_structures_get_json, structureId)

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativePagesListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  char *out = nullptr;
  st = loom_pages_list_json(h, n, pw, &out);
  PAGES_RELEASE_NS();
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStructuresCreateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring structureId, jstring spaceId, jstring kind, jstring title, jstring expectedRoot,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *structure = env->GetStringUTFChars(structureId, nullptr);
  const char *space = env->GetStringUTFChars(spaceId, nullptr);
  const char *k = env->GetStringUTFChars(kind, nullptr);
  const char *ttl = env->GetStringUTFChars(title, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_structures_create_json(h, n, pw, structure, space, k, ttl, root, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(structureId, structure);
  env->ReleaseStringUTFChars(spaceId, space);
  env->ReleaseStringUTFChars(kind, k);
  env->ReleaseStringUTFChars(title, ttl);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishPagesString(env, h, st, out);
}

#define STRUCTURE_NODE(java_name, c_name) \
extern "C" JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_rn_UldrenLoomNative_##java_name( \
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId, \
    jstring structureId, jstring nodeId, jstring kind, jstring label, jstring bodyDigest, \
    jstring entityRef, jstring expectedRoot, jbyteArray passphrase, jbyteArray kek, \
    jstring authPrincipal, jbyteArray authPassphrase) { \
  (void)thiz; \
  PAGES_OPEN(); \
  const char *structure = env->GetStringUTFChars(structureId, nullptr); \
  const char *node = env->GetStringUTFChars(nodeId, nullptr); \
  const char *k = env->GetStringUTFChars(kind, nullptr); \
  const char *lbl = env->GetStringUTFChars(label, nullptr); \
  const char *digest = env->GetStringUTFChars(bodyDigest, nullptr); \
  const char *entity = env->GetStringUTFChars(entityRef, nullptr); \
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr); \
  char *out = nullptr; \
  st = c_name(h, n, pw, structure, node, k, lbl, digest, entity, root, &out); \
  PAGES_RELEASE_NS(); \
  env->ReleaseStringUTFChars(structureId, structure); \
  env->ReleaseStringUTFChars(nodeId, node); \
  env->ReleaseStringUTFChars(kind, k); \
  env->ReleaseStringUTFChars(label, lbl); \
  env->ReleaseStringUTFChars(bodyDigest, digest); \
  env->ReleaseStringUTFChars(entityRef, entity); \
  env->ReleaseStringUTFChars(expectedRoot, root); \
  return finishPagesString(env, h, st, out); \
}

STRUCTURE_NODE(nativeStructuresAddNodeJson, loom_structures_add_node_json)
STRUCTURE_NODE(nativeStructuresUpdateNodeJson, loom_structures_update_node_json)

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStructuresBindJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring structureId, jstring nodeId, jstring entityRef, jstring expectedRoot,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *structure = env->GetStringUTFChars(structureId, nullptr);
  const char *node = env->GetStringUTFChars(nodeId, nullptr);
  const char *entity = env->GetStringUTFChars(entityRef, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_structures_bind_json(h, n, pw, structure, node, entity, root, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(structureId, structure);
  env->ReleaseStringUTFChars(nodeId, node);
  env->ReleaseStringUTFChars(entityRef, entity);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStructuresMoveNodeJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring structureId, jstring nodeId, jstring parentNodeId, jstring label,
    jstring expectedRoot, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *structure = env->GetStringUTFChars(structureId, nullptr);
  const char *node = env->GetStringUTFChars(nodeId, nullptr);
  const char *parent = env->GetStringUTFChars(parentNodeId, nullptr);
  const char *lbl = env->GetStringUTFChars(label, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_structures_move_node_json(h, n, pw, structure, node, parent, lbl, root, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(structureId, structure);
  env->ReleaseStringUTFChars(nodeId, node);
  env->ReleaseStringUTFChars(parentNodeId, parent);
  env->ReleaseStringUTFChars(label, lbl);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStructuresLinkNodeJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring structureId, jstring edgeId, jstring srcNodeId, jstring dstNodeId, jstring label,
    jstring targetRef, jstring expectedRoot, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *structure = env->GetStringUTFChars(structureId, nullptr);
  const char *edge = env->GetStringUTFChars(edgeId, nullptr);
  const char *src = env->GetStringUTFChars(srcNodeId, nullptr);
  const char *dst = env->GetStringUTFChars(dstNodeId, nullptr);
  const char *lbl = env->GetStringUTFChars(label, nullptr);
  const char *target = env->GetStringUTFChars(targetRef, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_structures_link_node_json(h, n, pw, structure, edge, src, dst, lbl, target, root, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(structureId, structure);
  env->ReleaseStringUTFChars(edgeId, edge);
  env->ReleaseStringUTFChars(srcNodeId, src);
  env->ReleaseStringUTFChars(dstNodeId, dst);
  env->ReleaseStringUTFChars(label, lbl);
  env->ReleaseStringUTFChars(targetRef, target);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStructuresDecomposeToTicketsJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jstring structureId, jstring itemsJson, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  const char *structure = env->GetStringUTFChars(structureId, nullptr);
  const char *items = env->GetStringUTFChars(itemsJson, nullptr);
  char *out = nullptr;
  st = loom_structures_decompose_to_tickets_json(h, n, pw, structure, items, &out);
  PAGES_RELEASE_NS();
  env->ReleaseStringUTFChars(structureId, structure);
  env->ReleaseStringUTFChars(itemsJson, items);
  return finishPagesString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeStructuresListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring pageWorkspaceId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  PAGES_OPEN();
  char *out = nullptr;
  st = loom_structures_list_json(h, n, pw, &out);
  PAGES_RELEASE_NS();
  return finishPagesString(env, h, st, out);
}

#undef PAGES_OPEN
#undef PAGES_RELEASE_NS
#undef PAGE_READ
#undef STRUCTURE_NODE
