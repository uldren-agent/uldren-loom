#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Lanes)

- (void)lanesBytes:(NSString *)loomPath passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase call:(int32_t (^)(LoomSession *, unsigned char **, uintptr_t *))call resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
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
      st = call(h, &ptr, &len);
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

- (void)lanesCreate:(NSString *)loomPath workspace:(NSString *)workspace lane:(NSArray *)lane passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self lanesBytes:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, unsigned char **ptr, uintptr_t *len) {
    NSUInteger inLen = 0;
    unsigned char *bytes = loomBytesFromArray(lane, &inLen);
    int32_t st = loom_lanes_create_cbor(h, workspace.UTF8String, bytes, (uintptr_t)inLen, ptr, len);
    free(bytes);
    return st;
  } resolve:resolve reject:reject];
}

- (void)lanesGet:(NSString *)loomPath workspace:(NSString *)workspace laneId:(NSString *)laneId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
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
      st = loom_lanes_get_cbor(h, workspace.UTF8String, laneId.UTF8String, &ptr, &len, &found);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(found ? loomArrayFromOwnedBytes(ptr, len) : (id)kCFNull);
  });
}

- (void)lanesList:(NSString *)loomPath workspace:(NSString *)workspace passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self lanesBytes:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, unsigned char **ptr, uintptr_t *len) {
    return loom_lanes_list_cbor(h, workspace.UTF8String, ptr, len);
  } resolve:resolve reject:reject];
}

#define LANES_STRING(objc_name, label_name, c_name) \
- (void)objc_name:(NSString *)loomPath workspace:(NSString *)workspace laneId:(NSString *)laneId label_name:(NSString *)value updatedBy:(NSString *)updatedBy passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject { \
  [self lanesBytes:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, unsigned char **ptr, uintptr_t *len) { \
    return c_name(h, workspace.UTF8String, laneId.UTF8String, value.UTF8String, updatedBy.UTF8String, ptr, len); \
  } resolve:resolve reject:reject]; \
}

LANES_STRING(lanesTicketRemove, ticketId, loom_lanes_ticket_remove_cbor)

- (void)lanesUpdate:(NSString *)loomPath workspace:(NSString *)workspace laneId:(NSString *)laneId title:(NSString *)title description:(NSString *)description laneStatus:(NSString *)laneStatus statusReport:(NSString *)statusReport reviewerFeedback:(NSString *)reviewerFeedback updatedBy:(NSString *)updatedBy passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self lanesBytes:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, unsigned char **ptr, uintptr_t *len) {
    return loom_lanes_update_cbor(h, workspace.UTF8String, laneId.UTF8String,
                                  title ? title.UTF8String : NULL,
                                  description ? description.UTF8String : NULL,
                                  laneStatus ? laneStatus.UTF8String : NULL,
                                  statusReport ? statusReport.UTF8String : NULL,
                                  reviewerFeedback ? reviewerFeedback.UTF8String : NULL,
                                  updatedBy.UTF8String, ptr, len);
  } resolve:resolve reject:reject];
}

- (void)lanesTicketAdd:(NSString *)loomPath workspace:(NSString *)workspace laneId:(NSString *)laneId ticketId:(NSString *)ticketId updatedBy:(NSString *)updatedBy placement:(NSString *)placement anchor:(NSString *)anchor passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self lanesBytes:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, unsigned char **ptr, uintptr_t *len) {
    return loom_lanes_ticket_add_cbor(h, workspace.UTF8String, laneId.UTF8String, ticketId.UTF8String, updatedBy.UTF8String, placement.UTF8String, anchor.UTF8String, ptr, len);
  } resolve:resolve reject:reject];
}

@end
