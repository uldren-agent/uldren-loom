#import "UldrenLoom+Internal.h"

static NSString *loomStringFromOwnedBytes(unsigned char *ptr, uintptr_t len) {
  if (ptr == NULL) {
    return @"";
  }
  NSData *data = [NSData dataWithBytes:ptr length:(NSUInteger)len];
  loom_bytes_free(ptr, len);
  NSString *value = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
  return value ?: @"";
}

static NSString *loomStringFromOwnedString(char *ptr) {
  if (ptr == NULL) {
    return @"";
  }
  NSString *value = [NSString stringWithUTF8String:ptr] ?: @"";
  loom_string_free(ptr);
  return value;
}

@implementation UldrenLoom (Document)

- (void)docPutText:(NSString *)loomPath
         workspace:(NSString *)ns
        collection:(NSString *)collection
                id:(NSString *)docId
              text:(NSString *)text
    expectedEntityTag:(NSString *)expectedEntityTag
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
    char *digest = NULL;
    char *entityTag = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      const char *guard = expectedEntityTag.length == 0 ? NULL : expectedEntityTag.UTF8String;
      st = loom_doc_put_text(h, ns.UTF8String, collection.UTF8String, docId.UTF8String,
                             text.UTF8String, guard, &digest, &entityTag);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *digestString = loomStringFromOwnedString(digest);
    NSString *entityTagString = loomStringFromOwnedString(entityTag);
    resolve(@{@"digest" : digestString, @"entity_tag" : entityTagString});
  });
}

- (void)docGetText:(NSString *)loomPath
         workspace:(NSString *)ns
        collection:(NSString *)collection
                id:(NSString *)docId
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
    char *text = NULL;
    char *digest = NULL;
    char *entityTag = NULL;
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_doc_get_text(h, ns.UTF8String, collection.UTF8String, docId.UTF8String, &text,
                             &digest, &entityTag, &found);
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
    NSString *textString = loomStringFromOwnedString(text);
    NSString *digestString = loomStringFromOwnedString(digest);
    NSString *entityTagString = loomStringFromOwnedString(entityTag);
    resolve(@{@"text" : textString, @"digest" : digestString, @"entity_tag" : entityTagString});
  });
}

- (void)docPutBinary:(NSString *)loomPath
           workspace:(NSString *)ns
          collection:(NSString *)collection
                  id:(NSString *)docId
               bytes:(NSArray *)bytes
      expectedEntityTag:(NSString *)expectedEntityTag
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
    NSUInteger dlen = 0;
    unsigned char *dbuf = loomBytesFromArray(bytes, &dlen);
    char *digest = NULL;
    char *entityTag = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      const char *guard = expectedEntityTag.length == 0 ? NULL : expectedEntityTag.UTF8String;
      st = loom_doc_put_binary(h, ns.UTF8String, collection.UTF8String, docId.UTF8String, dbuf,
                               (uintptr_t)dlen, guard, &digest, &entityTag);
    }
    free(dbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *digestString = loomStringFromOwnedString(digest);
    NSString *entityTagString = loomStringFromOwnedString(entityTag);
    resolve(@{@"digest" : digestString, @"entity_tag" : entityTagString});
  });
}

- (void)docGetBinary:(NSString *)loomPath
           workspace:(NSString *)ns
          collection:(NSString *)collection
                  id:(NSString *)docId
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
    char *digest = NULL;
    char *entityTag = NULL;
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_doc_get_binary(h, ns.UTF8String, collection.UTF8String, docId.UTF8String, &ptr,
                               &len, &digest, &entityTag, &found);
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
    NSArray *value = loomArrayFromOwnedBytes(ptr, len);
    NSString *digestString = loomStringFromOwnedString(digest);
    NSString *entityTagString = loomStringFromOwnedString(entityTag);
    resolve(@{@"bytes" : value, @"digest" : digestString, @"entity_tag" : entityTagString});
  });
}

- (void)docDelete:(NSString *)loomPath
       workspace:(NSString *)ns
            collection:(NSString *)collection
              id:(NSString *)docId
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
      st = loom_doc_delete(h, ns.UTF8String, collection.UTF8String, docId.UTF8String, &found);
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

- (void)docListBinary:(NSString *)loomPath
            workspace:(NSString *)ns
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
      st = loom_doc_list_binary_cbor(h, ns.UTF8String, collection.UTF8String, &ptr, &len);
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

- (void)docIndexCreate:(NSString *)loomPath
             workspace:(NSString *)ns
            collection:(NSString *)collection
                  name:(NSString *)name
             fieldPath:(NSString *)fieldPath
                unique:(BOOL)unique
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
      st = loom_doc_index_create(h, ns.UTF8String, collection.UTF8String, name.UTF8String,
                                 fieldPath.UTF8String, unique ? 1 : 0);
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

- (void)docIndexCreateJson:(NSString *)loomPath
                 workspace:(NSString *)ns
                collection:(NSString *)collection
           declarationJson:(NSArray *)declarationJson
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
    NSUInteger len = 0;
    unsigned char *bytes = loomBytesFromArray(declarationJson, &len);
    if (st == 0) {
      st = loom_doc_index_create_json(h, ns.UTF8String, collection.UTF8String, bytes, (uintptr_t)len);
    }
    free(bytes);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)docIndexDrop:(NSString *)loomPath
           workspace:(NSString *)ns
          collection:(NSString *)collection
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
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_doc_index_drop(h, ns.UTF8String, collection.UTF8String, name.UTF8String, &found);
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

- (void)docIndexRebuild:(NSString *)loomPath
              workspace:(NSString *)ns
             collection:(NSString *)collection
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
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_doc_index_rebuild(h, ns.UTF8String, collection.UTF8String, name.UTF8String);
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

- (void)docIndexListJson:(NSString *)loomPath
               workspace:(NSString *)ns
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
      st = loom_doc_index_list_json(h, ns.UTF8String, collection.UTF8String, &ptr, &len);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomStringFromOwnedBytes(ptr, len));
  });
}

- (void)docIndexStatusJson:(NSString *)loomPath
                 workspace:(NSString *)ns
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
      st = loom_doc_index_status_json(h, ns.UTF8String, collection.UTF8String, &ptr, &len);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomStringFromOwnedBytes(ptr, len));
  });
}

- (void)docFindJson:(NSString *)loomPath
          workspace:(NSString *)ns
         collection:(NSString *)collection
              index:(NSString *)index
          valueJson:(NSString *)valueJson
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
    NSData *value = [valueJson dataUsingEncoding:NSUTF8StringEncoding] ?: [NSData data];
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_doc_find_json(h, ns.UTF8String, collection.UTF8String, index.UTF8String,
                              (const unsigned char *)value.bytes, (uintptr_t)value.length, &ptr,
                              &len);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomStringFromOwnedBytes(ptr, len));
  });
}

- (void)docQueryJson:(NSString *)loomPath
           workspace:(NSString *)ns
          collection:(NSString *)collection
           queryJson:(NSString *)queryJson
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
    NSData *query = [queryJson dataUsingEncoding:NSUTF8StringEncoding] ?: [NSData data];
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_doc_query_json(h, ns.UTF8String, collection.UTF8String,
                               (const unsigned char *)query.bytes, (uintptr_t)query.length, &ptr,
                               &len);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomStringFromOwnedBytes(ptr, len));
  });
}

@end
