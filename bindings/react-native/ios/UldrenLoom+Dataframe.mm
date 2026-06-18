#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Dataframe)

- (void)dataframeCreate:(NSString *)loomPath
              workspace:(NSString *)ns
                   name:(NSString *)name
                   plan:(NSArray *)plan
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
    NSUInteger plen = 0;
    unsigned char *pbuf = loomBytesFromArray(plan, &plen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_dataframe_create(h, ns.UTF8String, name.UTF8String, pbuf, (uintptr_t)plen);
    }
    free(pbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)dataframeCollect:(NSString *)loomPath
               workspace:(NSString *)ns
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
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_dataframe_collect_cbor(h, ns.UTF8String, name.UTF8String, &ptr, &len);
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

- (void)dataframePreview:(NSString *)loomPath
               workspace:(NSString *)ns
                    name:(NSString *)name
                    rows:(double)rows
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
      st = loom_dataframe_preview_cbor(h, ns.UTF8String, name.UTF8String, (uint64_t)rows, &ptr, &len);
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

- (void)dataframeMaterialize:(NSString *)loomPath
                   workspace:(NSString *)ns
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
    int32_t hasDigest = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_dataframe_materialize(h, ns.UTF8String, name.UTF8String, &out, &hasDigest);
    }
    loom_close(h);
    if (st != 0) {
      if (out != NULL) {
        loom_string_free(out);
      }
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    if (hasDigest == 0) {
      resolve([NSNull null]);
      return;
    }
    NSString *digest = out != NULL ? [NSString stringWithUTF8String:out] : @"";
    if (out != NULL) {
      loom_string_free(out);
    }
    resolve(digest);
  });
}

- (void)dataframePlanDigest:(NSString *)loomPath
                  workspace:(NSString *)ns
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
      st = loom_dataframe_plan_digest(h, ns.UTF8String, name.UTF8String, &out);
    }
    loom_close(h);
    if (st != 0) {
      if (out != NULL) {
        loom_string_free(out);
      }
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *digest = out != NULL ? [NSString stringWithUTF8String:out] : @"";
    if (out != NULL) {
      loom_string_free(out);
    }
    resolve(digest);
  });
}

- (void)dataframeSourceDigests:(NSString *)loomPath
                     workspace:(NSString *)ns
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
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_dataframe_source_digests_cbor(h, ns.UTF8String, name.UTF8String, &ptr, &len);
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

@end
