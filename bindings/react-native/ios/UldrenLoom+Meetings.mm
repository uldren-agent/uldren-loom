#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Meetings)

- (void)meetingsImportSnapshot:(NSString *)loomPath
                     workspace:(NSString *)workspace
                  inputProfile:(NSString *)inputProfile
                      snapshot:(NSArray *)snapshot
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
    NSUInteger len = 0;
    unsigned char *buf = loomBytesFromArray(snapshot, &len);
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_meetings_import_snapshot(
          h, workspace.UTF8String, inputProfile.UTF8String, buf, (uintptr_t)len,
          dryRun ? 1 : 0, &out);
    }
    free(buf);
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

- (void)meetingsSourceRead:(NSString *)loomPath
                 workspace:(NSString *)workspace
                  sourceId:(NSString *)sourceId
                      leaf:(NSString *)leaf
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
      st = loom_meetings_source_read(
          h, workspace.UTF8String, sourceId.UTF8String, leaf.UTF8String, &ptr, &len);
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
