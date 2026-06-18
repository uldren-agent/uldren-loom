#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Cas)

- (void)casPut:(NSString *)loomPath
     workspace:(NSString *)ns
       content:(NSArray *)content
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
    NSUInteger clen = 0;
    unsigned char *cbuf = loomBytesFromArray(content, &clen);
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_cas_put(h, ns.UTF8String, cbuf, (uintptr_t)clen, &out);
    }
    free(cbuf);
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

- (void)casGet:(NSString *)loomPath
     workspace:(NSString *)ns
        digest:(NSString *)digest
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
      st = loom_cas_get(h, ns.UTF8String, digest.UTF8String, &ptr, &len, &found);
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

- (void)casHas:(NSString *)loomPath
     workspace:(NSString *)ns
        digest:(NSString *)digest
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
      st = loom_cas_has(h, ns.UTF8String, digest.UTF8String, &found);
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

- (void)casListJson:(NSString *)loomPath
          workspace:(NSString *)ns
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
      st = loom_cas_list_json(h, ns.UTF8String, &out);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"[]";
    if (out) {
      loom_string_free(out);
    }
    resolve(result);
  });
}

- (void)casDelete:(NSString *)loomPath
        workspace:(NSString *)ns
           digest:(NSString *)digest
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
      st = loom_cas_delete(h, ns.UTF8String, digest.UTF8String, &found);
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

@end
