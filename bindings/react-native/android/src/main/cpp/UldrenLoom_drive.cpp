#include "UldrenLoom_jni.h"

static jstring finishString(JNIEnv *env, LoomSession *h, int32_t st, char *out) {
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

static jbyteArray finishBytes(JNIEnv *env, LoomSession *h, int32_t st, unsigned char *ptr,
                              uintptr_t len) {
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

static bool parseOptionalU64(JNIEnv *env, jstring value, uint64_t *out, bool *has) {
  const char *chars = env->GetStringUTFChars(value, nullptr);
  if (chars == nullptr) {
    return false;
  }
  bool present = chars[0] != '\0';
  env->ReleaseStringUTFChars(value, chars);
  if (!present) {
    *out = 0;
    *has = false;
    return true;
  }
  *has = true;
  return parseU64String(env, value, out);
}

#define DRIVE_OPEN() \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { throwLoom(env); return nullptr; } \
  const char *n = env->GetStringUTFChars(ns, nullptr); \
  const char *dw = env->GetStringUTFChars(driveWorkspaceId, nullptr)

#define DRIVE_RELEASE_NS() \
  env->ReleaseStringUTFChars(ns, n); \
  env->ReleaseStringUTFChars(driveWorkspaceId, dw)

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring folderId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *folder = env->GetStringUTFChars(folderId, nullptr);
  char *out = nullptr;
  st = loom_drive_list_json(h, n, dw, folder, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(folderId, folder);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveStatJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring folderId, jstring name, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *folder = env->GetStringUTFChars(folderId, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  char *out = nullptr;
  st = loom_drive_stat_json(h, n, dw, folder, nm, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(folderId, folder);
  env->ReleaseStringUTFChars(name, nm);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveReadFile(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring fileId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *file = env->GetStringUTFChars(fileId, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_drive_read(h, n, dw, file, &ptr, &len);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(fileId, file);
  return finishBytes(env, h, st, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveListVersionsJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring fileId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *file = env->GetStringUTFChars(fileId, nullptr);
  char *out = nullptr;
  st = loom_drive_list_versions_json(h, n, dw, file, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(fileId, file);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveListConflictsJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  char *out = nullptr;
  st = loom_drive_list_conflicts_json(h, n, dw, &out);
  DRIVE_RELEASE_NS();
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveListSharesJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  char *out = nullptr;
  st = loom_drive_list_shares_json(h, n, dw, &out);
  DRIVE_RELEASE_NS();
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveListRetentionJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  char *out = nullptr;
  st = loom_drive_list_retention_json(h, n, dw, &out);
  DRIVE_RELEASE_NS();
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveCreateFolderJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring parentFolderId, jstring folderId, jstring name, jstring expectedRoot,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *parent = env->GetStringUTFChars(parentFolderId, nullptr);
  const char *folder = env->GetStringUTFChars(folderId, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_drive_create_folder_json(h, n, dw, parent, folder, nm, root, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(parentFolderId, parent);
  env->ReleaseStringUTFChars(folderId, folder);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveCreateUploadJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring uploadId, jstring parentFolderId, jstring name, jstring fileId, jstring expectedRoot,
    jstring createdAtMs, jboolean replaceFile, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t created = 0;
  if (!parseU64String(env, createdAtMs, &created)) return nullptr;
  DRIVE_OPEN();
  const char *upload = env->GetStringUTFChars(uploadId, nullptr);
  const char *parent = env->GetStringUTFChars(parentFolderId, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *file = env->GetStringUTFChars(fileId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_drive_create_upload_json(
      h, n, dw, upload, parent, nm, file, root, created, replaceFile ? 1 : 0, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(uploadId, upload);
  env->ReleaseStringUTFChars(parentFolderId, parent);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(fileId, file);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveUploadChunkJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring uploadId, jbyteArray chunk, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *upload = env->GetStringUTFChars(uploadId, nullptr);
  jsize chunkLen = (chunk != nullptr) ? env->GetArrayLength(chunk) : 0;
  jbyte *chunkBytes = (chunk != nullptr) ? env->GetByteArrayElements(chunk, nullptr) : nullptr;
  char *out = nullptr;
  st = loom_drive_upload_chunk_json(
      h, n, dw, upload, reinterpret_cast<const unsigned char *>(chunkBytes),
      static_cast<uintptr_t>(chunkLen), &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(uploadId, upload);
  if (chunkBytes) env->ReleaseByteArrayElements(chunk, chunkBytes, JNI_ABORT);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveCommitUploadJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring uploadId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *upload = env->GetStringUTFChars(uploadId, nullptr);
  char *out = nullptr;
  st = loom_drive_commit_upload_json(h, n, dw, upload, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(uploadId, upload);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveRenameJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring folderId, jstring nodeId, jstring newName, jstring expectedRoot,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *folder = env->GetStringUTFChars(folderId, nullptr);
  const char *node = env->GetStringUTFChars(nodeId, nullptr);
  const char *nm = env->GetStringUTFChars(newName, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_drive_rename_json(h, n, dw, folder, node, nm, root, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(folderId, folder);
  env->ReleaseStringUTFChars(nodeId, node);
  env->ReleaseStringUTFChars(newName, nm);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveMoveJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring sourceFolderId, jstring targetFolderId, jstring nodeId, jstring expectedRoot,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *source = env->GetStringUTFChars(sourceFolderId, nullptr);
  const char *target = env->GetStringUTFChars(targetFolderId, nullptr);
  const char *node = env->GetStringUTFChars(nodeId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_drive_move_json(h, n, dw, source, target, node, root, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(sourceFolderId, source);
  env->ReleaseStringUTFChars(targetFolderId, target);
  env->ReleaseStringUTFChars(nodeId, node);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveDeleteJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring folderId, jstring nodeId, jstring expectedRoot, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *folder = env->GetStringUTFChars(folderId, nullptr);
  const char *node = env->GetStringUTFChars(nodeId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_drive_delete_json(h, n, dw, folder, node, root, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(folderId, folder);
  env->ReleaseStringUTFChars(nodeId, node);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveResolveConflictJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring conflictId, jstring resolution, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *conflict = env->GetStringUTFChars(conflictId, nullptr);
  const char *res = env->GetStringUTFChars(resolution, nullptr);
  char *out = nullptr;
  st = loom_drive_resolve_conflict_json(h, n, dw, conflict, res, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(conflictId, conflict);
  env->ReleaseStringUTFChars(resolution, res);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveGrantShareJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring grantId, jstring targetKind, jstring targetId, jstring principal, jstring role,
    jstring grantedAtMs, jstring expiresAtMs, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t granted = 0;
  uint64_t expires = 0;
  bool hasExpires = false;
  if (!parseU64String(env, grantedAtMs, &granted)) return nullptr;
  if (!parseOptionalU64(env, expiresAtMs, &expires, &hasExpires)) return nullptr;
  DRIVE_OPEN();
  const char *grant = env->GetStringUTFChars(grantId, nullptr);
  const char *kind = env->GetStringUTFChars(targetKind, nullptr);
  const char *target = env->GetStringUTFChars(targetId, nullptr);
  const char *pr = env->GetStringUTFChars(principal, nullptr);
  const char *rl = env->GetStringUTFChars(role, nullptr);
  char *out = nullptr;
  st = loom_drive_grant_share_json(
      h, n, dw, grant, kind, target, pr, rl, granted, expires, hasExpires ? 1 : 0, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(grantId, grant);
  env->ReleaseStringUTFChars(targetKind, kind);
  env->ReleaseStringUTFChars(targetId, target);
  env->ReleaseStringUTFChars(principal, pr);
  env->ReleaseStringUTFChars(role, rl);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveRevokeShareJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring grantId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *grant = env->GetStringUTFChars(grantId, nullptr);
  char *out = nullptr;
  st = loom_drive_revoke_share_json(h, n, dw, grant, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(grantId, grant);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveApplyShareExpiryJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring nowMs, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t now = 0;
  if (!parseU64String(env, nowMs, &now)) return nullptr;
  DRIVE_OPEN();
  char *out = nullptr;
  st = loom_drive_apply_share_expiry_json(h, n, dw, now, &out);
  DRIVE_RELEASE_NS();
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDrivePinRetentionJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring pinId, jstring kind, jstring root, jstring targetEntityId, jstring addedAtMs,
    jstring expiresAtMs, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t added = 0;
  uint64_t expires = 0;
  bool hasExpires = false;
  if (!parseU64String(env, addedAtMs, &added)) return nullptr;
  if (!parseOptionalU64(env, expiresAtMs, &expires, &hasExpires)) return nullptr;
  DRIVE_OPEN();
  const char *pin = env->GetStringUTFChars(pinId, nullptr);
  const char *kd = env->GetStringUTFChars(kind, nullptr);
  const char *rt = env->GetStringUTFChars(root, nullptr);
  const char *target = env->GetStringUTFChars(targetEntityId, nullptr);
  const char *targetPtr = target[0] == '\0' ? nullptr : target;
  char *out = nullptr;
  st = loom_drive_pin_retention_json(
      h, n, dw, pin, kd, rt, targetPtr, added, expires, hasExpires ? 1 : 0, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(pinId, pin);
  env->ReleaseStringUTFChars(kind, kd);
  env->ReleaseStringUTFChars(root, rt);
  env->ReleaseStringUTFChars(targetEntityId, target);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveUnpinRetentionJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring pinId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  DRIVE_OPEN();
  const char *pin = env->GetStringUTFChars(pinId, nullptr);
  char *out = nullptr;
  st = loom_drive_unpin_retention_json(h, n, dw, pin, &out);
  DRIVE_RELEASE_NS();
  env->ReleaseStringUTFChars(pinId, pin);
  return finishString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeDriveApplyRetentionJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring driveWorkspaceId,
    jstring nowMs, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  uint64_t now = 0;
  if (!parseU64String(env, nowMs, &now)) return nullptr;
  DRIVE_OPEN();
  char *out = nullptr;
  st = loom_drive_apply_retention_json(h, n, dw, now, &out);
  DRIVE_RELEASE_NS();
  return finishString(env, h, st, out);
}

#undef DRIVE_OPEN
#undef DRIVE_RELEASE_NS
