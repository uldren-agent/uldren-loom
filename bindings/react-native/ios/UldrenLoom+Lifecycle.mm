#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Lifecycle)

- (void)create:(NSString *)loomPath
       profile:(NSString *)profile
         suite:(NSString *)suite
    passphrase:(NSString *)passphrase
       resolve:(RCTPromiseResolveBlock)resolve
        reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    const char *su = suite.length ? suite.UTF8String : NULL;
    NSData *pd = passphrase.length ? [passphrase dataUsingEncoding:NSUTF8StringEncoding] : nil;
    int32_t st = loom_create(loomPath.UTF8String, profile.UTF8String, su,
                             pd ? (const unsigned char *)pd.bytes : NULL,
                             pd ? (uintptr_t)pd.length : 0);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

// As `create`, but wraps the DEK under a host-supplied 256-bit KEK. `kek` is a
// 0-255 number array (32 bytes). Resolves nil.

- (void)createWithKek:(NSString *)loomPath
              profile:(NSString *)profile
                suite:(NSString *)suite
                  kek:(NSArray *)kek
              resolve:(RCTPromiseResolveBlock)resolve
               reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    const char *su = suite.length ? suite.UTF8String : NULL;
    NSUInteger klen = kek.count;
    unsigned char *kbuf = (unsigned char *)malloc(klen ? klen : 1);
    for (NSUInteger i = 0; i < klen; i++) {
      kbuf[i] = (unsigned char)([kek[i] integerValue] & 0xFF);
    }
    int32_t st =
        loom_create_with_kek(loomPath.UTF8String, profile.UTF8String, su, kbuf, (uintptr_t)klen);
    free(kbuf);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)capabilities:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_capabilities(&ptr, &len);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)runtimeProfile:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_runtime_profile(&ptr, &len);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)studioSurfaceCatalogJson:(NSString *)workspace
                             set:(NSString *)set
                         resolve:(RCTPromiseResolveBlock)resolve
                          reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    char *out = NULL;
    int32_t st = loom_studio_surface_catalog_json(workspace.UTF8String, set.UTF8String, &out);
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
