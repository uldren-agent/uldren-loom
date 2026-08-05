#import "UldrenLoom+Internal.h"
#include <math.h>

@implementation UldrenLoom (Operational)

- (void)studioReindexJson:(NSString *)loomPath
                workspace:(NSString *)workspace
                  profile:(NSString *)profile
               passphrase:(NSString *)passphrase
                      kek:(NSArray *)kek
            authPrincipal:(NSString *)authPrincipal
           authPassphrase:(NSString *)authPassphrase
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
      st = loom_studio_reindex_json(h, workspace.UTF8String, profile.UTF8String, &out);
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

- (void)studioRevisionsRebuildJson:(NSString *)loomPath
                          workspace:(NSString *)workspace
                            profile:(NSString *)profile
                             dryRun:(BOOL)dryRun
                         passphrase:(NSString *)passphrase
                                kek:(NSArray *)kek
                      authPrincipal:(NSString *)authPrincipal
                     authPassphrase:(NSString *)authPassphrase
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
      st = loom_studio_revisions_rebuild_json(
          h, workspace.UTF8String, profile.UTF8String, dryRun ? 1 : 0, &out);
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

- (void)storeBundleImport:(NSString *)loomPath
                   bundle:(NSArray *)bundle
                   dryRun:(BOOL)dryRun
               passphrase:(NSString *)passphrase
                      kek:(NSArray *)kek
            authPrincipal:(NSString *)authPrincipal
           authPassphrase:(NSString *)authPassphrase
                  resolve:(RCTPromiseResolveBlock)resolve
                   reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek];
    if (h == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSUInteger bundleLen = 0;
    unsigned char *bundleBytes = loomBytesFromArray(bundle, &bundleLen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_store_bundle_import(h, bundleBytes, (uintptr_t)bundleLen, dryRun ? 1 : 0, &ptr, &len);
    }
    free(bundleBytes);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)auditCompact:(NSString *)loomPath
          throughSeq:(double)throughSeq
          passphrase:(NSString *)passphrase
                 kek:(NSArray *)kek
       authPrincipal:(NSString *)authPrincipal
      authPassphrase:(NSString *)authPassphrase
             resolve:(RCTPromiseResolveBlock)resolve
              reject:(RCTPromiseRejectBlock)reject {
  if (throughSeq < 0 || throughSeq > 9007199254740991.0 || throughSeq != floor(throughSeq)) {
    reject(@"22", @"throughSeq must be a non-negative safe integer", nil);
    return;
  }
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
      st = loom_audit_compact(h, (uint64_t)throughSeq, &ptr, &len);
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

#define STORE_MAINTENANCE_METHOD(objcName, inputName, cName) \
- (void)objcName:(NSString *)loomPath \
       inputName:(NSArray *)inputName \
      passphrase:(NSString *)passphrase \
             kek:(NSArray *)kek \
   authPrincipal:(NSString *)authPrincipal \
  authPassphrase:(NSString *)authPassphrase \
         resolve:(RCTPromiseResolveBlock)resolve \
          reject:(RCTPromiseRejectBlock)reject { \
  dispatch_async([self workQueue], ^{ \
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek]; \
    if (h == NULL) { \
      NSError *err = [self loomError]; \
      reject([@(err.code) stringValue], err.localizedDescription, err); \
      return; \
    } \
    NSUInteger inputLen = 0; \
    unsigned char *input = loomBytesFromArray(inputName, &inputLen); \
    unsigned char *ptr = NULL; \
    uintptr_t len = 0; \
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase]; \
    if (st == 0) { \
      st = cName(h, input, (uintptr_t)inputLen, &ptr, &len); \
    } \
    free(input); \
    loom_close(h); \
    if (st != 0) { \
      NSError *err = [self loomError]; \
      reject([@(err.code) stringValue], err.localizedDescription, err); \
      return; \
    } \
    resolve(loomArrayFromOwnedBytes(ptr, len)); \
  }); \
}

STORE_MAINTENANCE_METHOD(storeMaintenanceStatus, request, loom_store_maintenance_status)
STORE_MAINTENANCE_METHOD(storeMaintenancePolicySet, update, loom_store_maintenance_policy_set)
STORE_MAINTENANCE_METHOD(storeMaintenanceRun, request, loom_store_maintenance_run)

#undef STORE_MAINTENANCE_METHOD

- (void)importTableCsv:(NSString *)loomPath
             workspace:(NSString *)workspace
           sourceScope:(NSString *)sourceScope
            csvPayload:(NSArray *)csvPayload
              database:(NSString *)database
                 table:(NSString *)table
                schema:(NSString *)schema
            primaryKey:(NSString *)primaryKey
                  mode:(NSString *)mode
                commit:(BOOL)commit
                author:(NSString *)author
               message:(NSString *)message
                dryRun:(BOOL)dryRun
            passphrase:(NSString *)passphrase
                   kek:(NSArray *)kek
         authPrincipal:(NSString *)authPrincipal
        authPassphrase:(NSString *)authPassphrase
               resolve:(RCTPromiseResolveBlock)resolve
                reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek];
    if (h == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSUInteger payloadLen = 0;
    unsigned char *payload = loomBytesFromArray(csvPayload, &payloadLen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    const char *authorArg = author != nil ? author.UTF8String : NULL;
    const char *messageArg = message != nil ? message.UTF8String : NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_import_table_csv(
          h, workspace.UTF8String, sourceScope.UTF8String, payload, (uintptr_t)payloadLen,
          database.UTF8String, table.UTF8String, schema.UTF8String, primaryKey.UTF8String,
          mode.UTF8String, commit ? 1 : 0, authorArg, messageArg, dryRun ? 1 : 0, &ptr, &len);
    }
    free(payload);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

#define INTERCHANGE_TICKET_METHOD(objc_name, c_name) \
- (void)objc_name:(NSString *)loomPath \
        workspace:(NSString *)workspace \
          profile:(NSString *)profile \
      sourceScope:(NSString *)sourceScope \
  snapshotPayload:(NSArray *)snapshotPayload \
      fieldPolicy:(NSString *)fieldPolicy \
           dryRun:(BOOL)dryRun \
       passphrase:(NSString *)passphrase \
              kek:(NSArray *)kek \
    authPrincipal:(NSString *)authPrincipal \
   authPassphrase:(NSString *)authPassphrase \
          resolve:(RCTPromiseResolveBlock)resolve \
           reject:(RCTPromiseRejectBlock)reject { \
  dispatch_async([self workQueue], ^{ \
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek]; \
    if (h == NULL) { \
      NSError *err = [self loomError]; \
      reject([@(err.code) stringValue], err.localizedDescription, err); \
      return; \
    } \
    NSUInteger payloadLen = 0; \
    unsigned char *payload = loomBytesFromArray(snapshotPayload, &payloadLen); \
    unsigned char *ptr = NULL; \
    uintptr_t len = 0; \
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase]; \
    if (st == 0) { \
      st = c_name(h, workspace.UTF8String, profile.UTF8String, sourceScope.UTF8String, payload, \
                  (uintptr_t)payloadLen, fieldPolicy.UTF8String, dryRun ? 1 : 0, &ptr, &len); \
    } \
    free(payload); \
    loom_close(h); \
    if (st != 0) { \
      NSError *err = [self loomError]; \
      reject([@(err.code) stringValue], err.localizedDescription, err); \
      return; \
    } \
    resolve(loomArrayFromOwnedBytes(ptr, len)); \
  }); \
}

INTERCHANGE_TICKET_METHOD(importRedmine, loom_import_redmine)
INTERCHANGE_TICKET_METHOD(importAsana, loom_import_asana)
INTERCHANGE_TICKET_METHOD(importJira, loom_import_jira)

#undef INTERCHANGE_TICKET_METHOD

#define INTERCHANGE_STRING_METHOD(objc_name, c_name, payload_label, payload_name, text_label, text_name) \
- (void)objc_name:(NSString *)loomPath \
        workspace:(NSString *)workspace \
          profile:(NSString *)profile \
      sourceScope:(NSString *)sourceScope \
    payload_label:(NSArray *)payload_name \
       text_label:(NSString *)text_name \
           dryRun:(BOOL)dryRun \
       passphrase:(NSString *)passphrase \
              kek:(NSArray *)kek \
    authPrincipal:(NSString *)authPrincipal \
   authPassphrase:(NSString *)authPassphrase \
          resolve:(RCTPromiseResolveBlock)resolve \
           reject:(RCTPromiseRejectBlock)reject { \
  dispatch_async([self workQueue], ^{ \
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek]; \
    if (h == NULL) { \
      NSError *err = [self loomError]; \
      reject([@(err.code) stringValue], err.localizedDescription, err); \
      return; \
    } \
    NSUInteger payloadLen = 0; \
    unsigned char *payload = loomBytesFromArray(payload_name, &payloadLen); \
    unsigned char *ptr = NULL; \
    uintptr_t len = 0; \
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase]; \
    if (st == 0) { \
      st = c_name(h, workspace.UTF8String, profile.UTF8String, sourceScope.UTF8String, payload, \
                  (uintptr_t)payloadLen, text_name.UTF8String, dryRun ? 1 : 0, &ptr, &len); \
    } \
    free(payload); \
    loom_close(h); \
    if (st != 0) { \
      NSError *err = [self loomError]; \
      reject([@(err.code) stringValue], err.localizedDescription, err); \
      return; \
    } \
    resolve(loomArrayFromOwnedBytes(ptr, len)); \
  }); \
}

INTERCHANGE_STRING_METHOD(importConfluence, loom_import_confluence, snapshotPayload, snapshotPayload, defaultSpace, defaultSpace)
INTERCHANGE_STRING_METHOD(importMarkdown, loom_import_markdown, archivePayload, archivePayload, space, space)
INTERCHANGE_STRING_METHOD(importNotion, loom_import_notion, snapshotPayload, snapshotPayload, defaultSpace, defaultSpace)

#undef INTERCHANGE_STRING_METHOD

#define INTERCHANGE_SIMPLE_METHOD(objc_name, c_name, payload_label, payload_name) \
- (void)objc_name:(NSString *)loomPath \
        workspace:(NSString *)workspace \
          profile:(NSString *)profile \
      sourceScope:(NSString *)sourceScope \
    payload_label:(NSArray *)payload_name \
           dryRun:(BOOL)dryRun \
       passphrase:(NSString *)passphrase \
              kek:(NSArray *)kek \
    authPrincipal:(NSString *)authPrincipal \
   authPassphrase:(NSString *)authPassphrase \
          resolve:(RCTPromiseResolveBlock)resolve \
           reject:(RCTPromiseRejectBlock)reject { \
  dispatch_async([self workQueue], ^{ \
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek]; \
    if (h == NULL) { \
      NSError *err = [self loomError]; \
      reject([@(err.code) stringValue], err.localizedDescription, err); \
      return; \
    } \
    NSUInteger payloadLen = 0; \
    unsigned char *payload = loomBytesFromArray(payload_name, &payloadLen); \
    unsigned char *ptr = NULL; \
    uintptr_t len = 0; \
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase]; \
    if (st == 0) { \
      st = c_name(h, workspace.UTF8String, profile.UTF8String, sourceScope.UTF8String, payload, \
                  (uintptr_t)payloadLen, dryRun ? 1 : 0, &ptr, &len); \
    } \
    free(payload); \
    loom_close(h); \
    if (st != 0) { \
      NSError *err = [self loomError]; \
      reject([@(err.code) stringValue], err.localizedDescription, err); \
      return; \
    } \
    resolve(loomArrayFromOwnedBytes(ptr, len)); \
  }); \
}

INTERCHANGE_SIMPLE_METHOD(importSlack, loom_import_slack, snapshotPayload, snapshotPayload)
INTERCHANGE_SIMPLE_METHOD(importDrive, loom_import_drive, archivePayload, archivePayload)

#undef INTERCHANGE_SIMPLE_METHOD

- (void)inferenceInstanceCreateJson:(NSString *)loomPath
                          workspace:(NSString *)workspace
                               name:(NSString *)name
                              model:(NSString *)model
                               kind:(NSString *)kind
                            runtime:(NSString *)runtime
                             preset:(NSString *)preset
                       settingsJson:(NSString *)settingsJson
                         passphrase:(NSString *)passphrase
                                kek:(NSArray *)kek
                      authPrincipal:(NSString *)authPrincipal
                     authPassphrase:(NSString *)authPassphrase
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
    const char *presetArg = preset != nil ? preset.UTF8String : NULL;
    const char *settingsArg = settingsJson != nil ? settingsJson.UTF8String : NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_inference_instance_create_json(
          h, workspace.UTF8String, name.UTF8String, model.UTF8String, kind.UTF8String,
          runtime.UTF8String, presetArg, settingsArg, &out);
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

- (void)inferenceInstanceUpdateJson:(NSString *)loomPath
                          workspace:(NSString *)workspace
                               name:(NSString *)name
                             preset:(NSString *)preset
                       settingsJson:(NSString *)settingsJson
                         passphrase:(NSString *)passphrase
                                kek:(NSArray *)kek
                      authPrincipal:(NSString *)authPrincipal
                     authPassphrase:(NSString *)authPassphrase
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
    const char *presetArg = preset != nil ? preset.UTF8String : NULL;
    const char *settingsArg = settingsJson != nil ? settingsJson.UTF8String : NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_inference_instance_update_json(
          h, workspace.UTF8String, name.UTF8String, presetArg, settingsArg, &out);
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

- (void)inferenceInstanceDeleteJson:(NSString *)loomPath
                          workspace:(NSString *)workspace
                               name:(NSString *)name
                         passphrase:(NSString *)passphrase
                                kek:(NSArray *)kek
                      authPrincipal:(NSString *)authPrincipal
                     authPassphrase:(NSString *)authPassphrase
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
      st = loom_inference_instance_delete_json(h, workspace.UTF8String, name.UTF8String, &out);
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

- (void)serveListenerConfigureJson:(NSString *)loomPath
                        requestJson:(NSString *)requestJson
                         passphrase:(NSString *)passphrase
                                kek:(NSArray *)kek
                      authPrincipal:(NSString *)authPrincipal
                     authPassphrase:(NSString *)authPassphrase
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
      st = loom_serve_listener_configure_json(h, requestJson.UTF8String, &out);
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

- (void)serveListenerListJson:(NSString *)loomPath
                   passphrase:(NSString *)passphrase
                          kek:(NSArray *)kek
                authPrincipal:(NSString *)authPrincipal
               authPassphrase:(NSString *)authPassphrase
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
      st = loom_serve_listener_list_json(h, &out);
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

- (void)serveListenerSetEnabledJson:(NSString *)loomPath
                         listenerId:(NSString *)listenerId
                            enabled:(BOOL)enabled
                         passphrase:(NSString *)passphrase
                                kek:(NSArray *)kek
                      authPrincipal:(NSString *)authPrincipal
                     authPassphrase:(NSString *)authPassphrase
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
      st = loom_serve_listener_set_enabled_json(h, listenerId.UTF8String, enabled ? 1 : 0, &out);
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

- (void)serveListenerRemoveJson:(NSString *)loomPath
                     listenerId:(NSString *)listenerId
                     passphrase:(NSString *)passphrase
                            kek:(NSArray *)kek
                  authPrincipal:(NSString *)authPrincipal
                 authPassphrase:(NSString *)authPassphrase
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
      st = loom_serve_listener_remove_json(h, listenerId.UTF8String, &out);
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

- (void)serveWebRouteListJson:(NSString *)loomPath
                   listenerId:(NSString *)listenerId
                   passphrase:(NSString *)passphrase
                          kek:(NSArray *)kek
                authPrincipal:(NSString *)authPrincipal
               authPassphrase:(NSString *)authPassphrase
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
      st = loom_serve_web_route_list_json(h, listenerId.UTF8String, &out);
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

- (void)serveWebRouteSetJson:(NSString *)loomPath
                 requestJson:(NSString *)requestJson
                  passphrase:(NSString *)passphrase
                         kek:(NSArray *)kek
               authPrincipal:(NSString *)authPrincipal
              authPassphrase:(NSString *)authPassphrase
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
      st = loom_serve_web_route_set_json(h, requestJson.UTF8String, &out);
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

- (void)serveWebRouteRemoveJson:(NSString *)loomPath
                     listenerId:(NSString *)listenerId
                        routeId:(NSString *)routeId
                     passphrase:(NSString *)passphrase
                            kek:(NSArray *)kek
                  authPrincipal:(NSString *)authPrincipal
                 authPassphrase:(NSString *)authPassphrase
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
      st = loom_serve_web_route_remove_json(h, listenerId.UTF8String, routeId.UTF8String, &out);
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

@end
