#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Kv)

- (void)kvPut:(NSString *)loomPath
    workspace:(NSString *)ns
         collection:(NSString *)collection
          key:(NSArray *)key
        value:(NSArray *)value
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
    NSUInteger klen = 0;
    unsigned char *kbuf = loomBytesFromArray(key, &klen);
    NSUInteger vlen = 0;
    unsigned char *vbuf = loomBytesFromArray(value, &vlen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_kv_put(h, ns.UTF8String, collection.UTF8String, kbuf, (uintptr_t)klen, vbuf,
                       (uintptr_t)vlen);
    }
    free(kbuf);
    free(vbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)kvGet:(NSString *)loomPath
    workspace:(NSString *)ns
         collection:(NSString *)collection
          key:(NSArray *)key
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
    NSUInteger klen = 0;
    unsigned char *kbuf = loomBytesFromArray(key, &klen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_kv_get(h, ns.UTF8String, collection.UTF8String, kbuf, (uintptr_t)klen, &ptr, &len,
                       &found);
    }
    free(kbuf);
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

- (void)kvDelete:(NSString *)loomPath
       workspace:(NSString *)ns
            collection:(NSString *)collection
             key:(NSArray *)key
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
    NSUInteger klen = 0;
    unsigned char *kbuf = loomBytesFromArray(key, &klen);
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_kv_delete(h, ns.UTF8String, collection.UTF8String, kbuf, (uintptr_t)klen, &found);
    }
    free(kbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(found != 0));
  });
}

- (void)kvList:(NSString *)loomPath
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
      st = loom_kv_list_cbor(h, ns.UTF8String, collection.UTF8String, &ptr, &len);
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

- (void)kvRange:(NSString *)loomPath
      workspace:(NSString *)ns
           collection:(NSString *)collection
             lo:(NSArray *)lo
             hi:(NSArray *)hi
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
    NSUInteger lolen = 0;
    unsigned char *lobuf = loomBytesFromArray(lo, &lolen);
    NSUInteger hilen = 0;
    unsigned char *hibuf = loomBytesFromArray(hi, &hilen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_kv_range_cbor(h, ns.UTF8String, collection.UTF8String, lobuf, (uintptr_t)lolen,
                              hibuf, (uintptr_t)hilen, &ptr, &len);
    }
    free(lobuf);
    free(hibuf);
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
