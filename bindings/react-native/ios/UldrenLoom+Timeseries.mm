#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Timeseries)

- (void)tsPut:(NSString *)loomPath
    workspace:(NSString *)ns
         collection:(NSString *)collection
           ts:(NSString *)ts
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
    NSUInteger vlen = 0;
    unsigned char *vbuf = loomBytesFromArray(value, &vlen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_ts_put(h, ns.UTF8String, collection.UTF8String, (int64_t)ts.longLongValue, vbuf,
                       (uintptr_t)vlen);
    }
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

- (void)tsGet:(NSString *)loomPath
    workspace:(NSString *)ns
         collection:(NSString *)collection
           ts:(NSString *)ts
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
      st = loom_ts_get(h, ns.UTF8String, collection.UTF8String, (int64_t)ts.longLongValue, &ptr,
                       &len, &found);
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

- (void)tsRange:(NSString *)loomPath
      workspace:(NSString *)ns
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
      st = loom_ts_range_cbor(h, ns.UTF8String, collection.UTF8String, (int64_t)from.longLongValue,
                              (int64_t)to.longLongValue, &ptr, &len);
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

- (void)tsLatest:(NSString *)loomPath
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
    int64_t ts = 0;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_ts_latest(h, ns.UTF8String, collection.UTF8String, &ts, &ptr, &len, &found);
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
    resolve(@{
      @"ts" : [NSString stringWithFormat:@"%lld", (long long)ts],
      @"value" : loomArrayFromOwnedBytes(ptr, len)
    });
  });
}

@end
