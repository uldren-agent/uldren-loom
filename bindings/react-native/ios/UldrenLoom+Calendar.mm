#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Calendar)

- (void)calCreateCollection:(NSString *)loomPath
                  workspace:(NSString *)ns
                  principal:(NSString *)principal
                 collection:(NSString *)collection
                displayName:(NSString *)displayName
                 components:(NSString *)components
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
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_create_collection(h, ns.UTF8String, principal.UTF8String,
                                            collection.UTF8String, displayName.UTF8String,
                                            components.UTF8String);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)calDeleteCollection:(NSString *)loomPath
                  workspace:(NSString *)ns
                  principal:(NSString *)principal
                 collection:(NSString *)collection
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
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_delete_collection(h, ns.UTF8String, principal.UTF8String,
                                            collection.UTF8String, &found);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(found != 0));
  });
}

- (void)calListCollections:(NSString *)loomPath
                 workspace:(NSString *)ns
                 principal:(NSString *)principal
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
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_list_collections(h, ns.UTF8String, principal.UTF8String, &ptr, &len);
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

- (void)calPutEntry:(NSString *)loomPath
          workspace:(NSString *)ns
          principal:(NSString *)principal
         collection:(NSString *)collection
              entry:(NSArray *)entry
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
    NSUInteger elen = 0;
    unsigned char *ebuf = loomBytesFromArray(entry, &elen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_put_entry(h, ns.UTF8String, principal.UTF8String, collection.UTF8String,
                                    ebuf, (uintptr_t)elen);
    }
    free(ebuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)calGetEntry:(NSString *)loomPath
          workspace:(NSString *)ns
          principal:(NSString *)principal
         collection:(NSString *)collection
                uid:(NSString *)uid
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
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_get_entry(h, ns.UTF8String, principal.UTF8String, collection.UTF8String,
                                    uid.UTF8String, &ptr, &len, &found);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    if (found == 0) {
      resolve(nil);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)calDeleteEntry:(NSString *)loomPath
             workspace:(NSString *)ns
             principal:(NSString *)principal
            collection:(NSString *)collection
                   uid:(NSString *)uid
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
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_delete_entry(h, ns.UTF8String, principal.UTF8String, collection.UTF8String,
                                       uid.UTF8String, &found);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(found != 0));
  });
}

- (void)calListEntries:(NSString *)loomPath
             workspace:(NSString *)ns
             principal:(NSString *)principal
            collection:(NSString *)collection
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
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_list_entries(h, ns.UTF8String, principal.UTF8String, collection.UTF8String,
                                       &ptr, &len);
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

- (void)calRange:(NSString *)loomPath
       workspace:(NSString *)ns
       principal:(NSString *)principal
      collection:(NSString *)collection
            from:(NSString *)from
              to:(NSString *)to
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
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_range(h, ns.UTF8String, principal.UTF8String, collection.UTF8String,
                                from.UTF8String, to.UTF8String, &ptr, &len);
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

- (void)calSearch:(NSString *)loomPath
        workspace:(NSString *)ns
        principal:(NSString *)principal
       collection:(NSString *)collection
        component:(NSString *)component
             text:(NSString *)text
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
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_search(h, ns.UTF8String, principal.UTF8String, collection.UTF8String,
                                 component.UTF8String, text.UTF8String, &ptr, &len);
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

- (void)calEntryIcs:(NSString *)loomPath
          workspace:(NSString *)ns
          principal:(NSString *)principal
         collection:(NSString *)collection
                uid:(NSString *)uid
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
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cal_entry_ics(h, ns.UTF8String, principal.UTF8String, collection.UTF8String,
                                    uid.UTF8String, &out, &found);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    if (found == 0) {
      resolve(nil);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"";
    if (out) {
      loom_string_free(out);
    }
    resolve(result);
  });
}

- (void)calPutIcs:(NSString *)loomPath
        workspace:(NSString *)ns
        principal:(NSString *)principal
       collection:(NSString *)collection
              ics:(NSString *)ics
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
      st = loom_cal_put_ics(h, ns.UTF8String, principal.UTF8String, collection.UTF8String,
                                  ics.UTF8String, &out);
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

// --- Contacts facade (CardDAV address books + contacts). Each call opens the loom for the op and closes. ---

@end
