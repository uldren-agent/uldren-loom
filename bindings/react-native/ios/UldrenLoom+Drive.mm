#import "UldrenLoom+Internal.h"

static void rejectDriveU64(RCTPromiseRejectBlock reject) {
  NSString *reason = @"drive timestamp must be an unsigned 64-bit decimal string";
  NSError *err = [NSError errorWithDomain:@"LoomError"
                                     code:22
                                 userInfo:@{NSLocalizedDescriptionKey : reason}];
  reject(@"22", reason, err);
}

static BOOL parseDriveU64(NSString *value, uint64_t *out, RCTPromiseRejectBlock reject) {
  if (loomParseU64(value, out)) {
    return YES;
  }
  rejectDriveU64(reject);
  return NO;
}

static BOOL parseOptionalDriveU64(NSString *value, uint64_t *out, int32_t *has,
                                  RCTPromiseRejectBlock reject) {
  if (value == nil || value.length == 0) {
    *out = 0;
    *has = 0;
    return YES;
  }
  *has = 1;
  return parseDriveU64(value, out, reject);
}

@implementation UldrenLoom (Drive)

- (void)driveString:(NSString *)loomPath
         passphrase:(NSString *)passphrase
                kek:(NSArray *)kek
      authPrincipal:(NSString *)authPrincipal
     authPassphrase:(NSString *)authPassphrase
               call:(int32_t (^)(LoomSession *, char **))call
            resolve:(RCTPromiseResolveBlock)resolve
             reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek];
    if (h == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = call(h, &out);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"";
    if (out) {
      loom_string_free(out);
    }
    resolve(result);
  });
}

- (void)driveBytes:(NSString *)loomPath
        passphrase:(NSString *)passphrase
               kek:(NSArray *)kek
     authPrincipal:(NSString *)authPrincipal
    authPassphrase:(NSString *)authPassphrase
              call:(int32_t (^)(LoomSession *, unsigned char **, uintptr_t *))call
           resolve:(RCTPromiseResolveBlock)resolve
            reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek];
    if (h == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = call(h, &ptr, &len);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)driveListJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId folderId:(NSString *)folderId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_list_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, folderId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveStatJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId folderId:(NSString *)folderId name:(NSString *)name passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_stat_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, folderId.UTF8String, name.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveReadFile:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId fileId:(NSString *)fileId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveBytes:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, unsigned char **ptr, uintptr_t *len) {
    return loom_drive_read(h, workspace.UTF8String, driveWorkspaceId.UTF8String, fileId.UTF8String, ptr, len);
  } resolve:resolve reject:reject];
}

- (void)driveListVersionsJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId fileId:(NSString *)fileId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_list_versions_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, fileId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveListConflictsJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_list_conflicts_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveListSharesJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_list_shares_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveListRetentionJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_list_retention_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveCreateFolderJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId parentFolderId:(NSString *)parentFolderId folderId:(NSString *)folderId name:(NSString *)name expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_create_folder_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, parentFolderId.UTF8String, folderId.UTF8String, name.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveCreateUploadJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId uploadId:(NSString *)uploadId parentFolderId:(NSString *)parentFolderId name:(NSString *)name fileId:(NSString *)fileId expectedRoot:(NSString *)expectedRoot createdAtMs:(NSString *)createdAtMs replaceFile:(BOOL)replaceFile passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  uint64_t created = 0;
  if (!parseDriveU64(createdAtMs, &created, reject)) return;
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_create_upload_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, uploadId.UTF8String, parentFolderId.UTF8String, name.UTF8String, fileId.UTF8String, expectedRoot.UTF8String, created, replaceFile ? 1 : 0, out);
  } resolve:resolve reject:reject];
}

- (void)driveUploadChunkJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId uploadId:(NSString *)uploadId chunk:(NSArray *)chunk passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    NSUInteger len = 0;
    unsigned char *buf = loomBytesFromArray(chunk, &len);
    int32_t st = loom_drive_upload_chunk_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, uploadId.UTF8String, buf, (uintptr_t)len, out);
    free(buf);
    return st;
  } resolve:resolve reject:reject];
}

- (void)driveCommitUploadJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId uploadId:(NSString *)uploadId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_commit_upload_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, uploadId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveRenameJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId folderId:(NSString *)folderId nodeId:(NSString *)nodeId newName:(NSString *)newName expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_rename_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, folderId.UTF8String, nodeId.UTF8String, newName.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveMoveJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId sourceFolderId:(NSString *)sourceFolderId targetFolderId:(NSString *)targetFolderId nodeId:(NSString *)nodeId expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_move_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, sourceFolderId.UTF8String, targetFolderId.UTF8String, nodeId.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveDeleteJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId folderId:(NSString *)folderId nodeId:(NSString *)nodeId expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_delete_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, folderId.UTF8String, nodeId.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveResolveConflictJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId conflictId:(NSString *)conflictId resolution:(NSString *)resolution passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_resolve_conflict_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, conflictId.UTF8String, resolution.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveGrantShareJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId grantId:(NSString *)grantId targetKind:(NSString *)targetKind targetId:(NSString *)targetId principal:(NSString *)principal role:(NSString *)role grantedAtMs:(NSString *)grantedAtMs expiresAtMs:(NSString *)expiresAtMs passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  uint64_t granted = 0;
  uint64_t expires = 0;
  int32_t hasExpires = 0;
  if (!parseDriveU64(grantedAtMs, &granted, reject)) return;
  if (!parseOptionalDriveU64(expiresAtMs, &expires, &hasExpires, reject)) return;
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_grant_share_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, grantId.UTF8String, targetKind.UTF8String, targetId.UTF8String, principal.UTF8String, role.UTF8String, granted, expires, hasExpires, out);
  } resolve:resolve reject:reject];
}

- (void)driveRevokeShareJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId grantId:(NSString *)grantId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_revoke_share_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, grantId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveApplyShareExpiryJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId nowMs:(NSString *)nowMs passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  uint64_t now = 0;
  if (!parseDriveU64(nowMs, &now, reject)) return;
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_apply_share_expiry_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, now, out);
  } resolve:resolve reject:reject];
}

- (void)drivePinRetentionJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId pinId:(NSString *)pinId kind:(NSString *)kind root:(NSString *)root targetEntityId:(NSString *)targetEntityId addedAtMs:(NSString *)addedAtMs expiresAtMs:(NSString *)expiresAtMs passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  uint64_t added = 0;
  uint64_t expires = 0;
  int32_t hasExpires = 0;
  if (!parseDriveU64(addedAtMs, &added, reject)) return;
  if (!parseOptionalDriveU64(expiresAtMs, &expires, &hasExpires, reject)) return;
  const char *target = targetEntityId.length ? targetEntityId.UTF8String : NULL;
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_pin_retention_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, pinId.UTF8String, kind.UTF8String, root.UTF8String, target, added, expires, hasExpires, out);
  } resolve:resolve reject:reject];
}

- (void)driveUnpinRetentionJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId pinId:(NSString *)pinId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_unpin_retention_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, pinId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)driveApplyRetentionJson:(NSString *)loomPath workspace:(NSString *)workspace driveWorkspaceId:(NSString *)driveWorkspaceId nowMs:(NSString *)nowMs passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  uint64_t now = 0;
  if (!parseDriveU64(nowMs, &now, reject)) return;
  [self driveString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_drive_apply_retention_json(h, workspace.UTF8String, driveWorkspaceId.UTF8String, now, out);
  } resolve:resolve reject:reject];
}

@end
