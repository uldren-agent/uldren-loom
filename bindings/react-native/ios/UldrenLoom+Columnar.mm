#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Columnar)

- (void)columnarCreate:(NSString *)loomPath
             workspace:(NSString *)ns
                  name:(NSString *)name
               columns:(NSArray *)columns
     targetSegmentRows:(double)targetSegmentRows
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
    unsigned char *cbuf = loomBytesFromArray(columns, &clen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_columnar_create(h, ns.UTF8String, name.UTF8String, cbuf, (uintptr_t)clen,
                                (uintptr_t)targetSegmentRows);
    }
    free(cbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)columnarAppend:(NSString *)loomPath
             workspace:(NSString *)ns
                  name:(NSString *)name
                   row:(NSArray *)row
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
    NSUInteger rlen = 0;
    unsigned char *rbuf = loomBytesFromArray(row, &rlen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_columnar_append(h, ns.UTF8String, name.UTF8String, rbuf, (uintptr_t)rlen);
    }
    free(rbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)columnarScan:(NSString *)loomPath
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
      st = loom_columnar_scan_cbor(h, ns.UTF8String, name.UTF8String, &ptr, &len);
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

- (void)columnarColumns:(NSString *)loomPath
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
      st = loom_columnar_columns_cbor(h, ns.UTF8String, name.UTF8String, &ptr, &len);
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

- (void)columnarRows:(NSString *)loomPath
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
    uint64_t count = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_columnar_rows(h, ns.UTF8String, name.UTF8String, &count);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@((double)count));
  });
}

- (void)columnarCompact:(NSString *)loomPath
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
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_columnar_compact(h, ns.UTF8String, name.UTF8String);
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

- (void)columnarInspect:(NSString *)loomPath
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
      st = loom_columnar_inspect_cbor(h, ns.UTF8String, name.UTF8String, &ptr, &len);
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

- (void)columnarSourceDigest:(NSString *)loomPath
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
      st = loom_columnar_source_digest_cbor(h, ns.UTF8String, name.UTF8String, &ptr, &len);
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

- (void)columnarSelect:(NSString *)loomPath
             workspace:(NSString *)ns
                  name:(NSString *)name
               columns:(NSArray *)columns
                filter:(NSArray *)filter
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
    unsigned char *cbuf = loomBytesFromArray(columns, &clen);
    NSUInteger flen = 0;
    unsigned char *fbuf = loomBytesFromArray(filter, &flen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_columnar_select_cbor(h, ns.UTF8String, name.UTF8String, cbuf, (uintptr_t)clen,
                                     fbuf, (uintptr_t)flen, &ptr, &len);
    }
    free(cbuf);
    free(fbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)columnarAggregate:(NSString *)loomPath
                workspace:(NSString *)ns
                     name:(NSString *)name
               aggregates:(NSArray *)aggregates
                   filter:(NSArray *)filter
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
    NSUInteger alen = 0;
    unsigned char *abuf = loomBytesFromArray(aggregates, &alen);
    NSUInteger flen = 0;
    unsigned char *fbuf = loomBytesFromArray(filter, &flen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_columnar_aggregate_cbor(h, ns.UTF8String, name.UTF8String, abuf, (uintptr_t)alen,
                                        fbuf, (uintptr_t)flen, &ptr, &len);
    }
    free(abuf);
    free(fbuf);
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
