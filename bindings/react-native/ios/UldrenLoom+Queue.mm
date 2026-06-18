#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Queue)

- (void)queueAppend:(NSString *)loomPath
          workspace:(NSString *)ns
             stream:(NSString *)stream
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
    uint64_t seq = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_queue_append(h, ns.UTF8String, stream.UTF8String, ebuf, (uintptr_t)elen, &seq);
    }
    free(ebuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomStringFromU64(seq));
  });
}

- (void)queueGet:(NSString *)loomPath
       workspace:(NSString *)ns
          stream:(NSString *)stream
             seq:(NSString *)seqText
      passphrase:(NSString *)passphrase
             kek:(NSArray *)kek
   authPrincipal:(NSString *)authPrincipal
  authPassphrase:(NSString *)authPassphrase
         resolve:(RCTPromiseResolveBlock)resolve
          reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    uint64_t seq = 0;
    if (!loomResolveU64(seqText, &seq, reject)) {
      return;
    }
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
      st = loom_queue_get(h, ns.UTF8String, stream.UTF8String, seq, &ptr, &len, &found);
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

- (void)queueRange:(NSString *)loomPath
         workspace:(NSString *)ns
            stream:(NSString *)stream
                lo:(NSString *)loText
                hi:(NSString *)hiText
        passphrase:(NSString *)passphrase
               kek:(NSArray *)kek
    authPrincipal:(NSString *)authPrincipal
   authPassphrase:(NSString *)authPassphrase
           resolve:(RCTPromiseResolveBlock)resolve
           reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    uint64_t lo = 0;
    uint64_t hi = 0;
    if (!loomResolveU64(loText, &lo, reject) || !loomResolveU64(hiText, &hi, reject)) {
      return;
    }
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
      st = loom_queue_range(h, ns.UTF8String, stream.UTF8String, lo, hi, &ptr, &len);
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

- (void)queueLen:(NSString *)loomPath
       workspace:(NSString *)ns
          stream:(NSString *)stream
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
    uint64_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_queue_len(h, ns.UTF8String, stream.UTF8String, &len);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomStringFromU64(len));
  });
}

- (void)queueConsumerPosition:(NSString *)loomPath
                    workspace:(NSString *)ns
                       stream:(NSString *)stream
                   consumerId:(NSString *)consumerId
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
    uint64_t seq = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_queue_consumer_position(h, ns.UTF8String, stream.UTF8String,
                                        consumerId.UTF8String, &seq);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomStringFromU64(seq));
  });
}

- (void)queueConsumerRead:(NSString *)loomPath
                workspace:(NSString *)ns
                   stream:(NSString *)stream
               consumerId:(NSString *)consumerId
                      max:(double)max
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
      st = loom_queue_consumer_read(h, ns.UTF8String, stream.UTF8String,
                                    consumerId.UTF8String, (uint32_t)max, &ptr, &len);
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

- (void)queueConsumerAdvance:(NSString *)loomPath
                   workspace:(NSString *)ns
                      stream:(NSString *)stream
                  consumerId:(NSString *)consumerId
                     nextSeq:(NSString *)nextSeqText
                  passphrase:(NSString *)passphrase
                         kek:(NSArray *)kek
              authPrincipal:(NSString *)authPrincipal
             authPassphrase:(NSString *)authPassphrase
                     resolve:(RCTPromiseResolveBlock)resolve
                      reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    uint64_t nextSeq = 0;
    if (!loomResolveU64(nextSeqText, &nextSeq, reject)) {
      return;
    }
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek];
    if (h == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_queue_consumer_advance(h, ns.UTF8String, stream.UTF8String,
                                       consumerId.UTF8String, nextSeq);
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

- (void)queueConsumerReset:(NSString *)loomPath
                 workspace:(NSString *)ns
                    stream:(NSString *)stream
                consumerId:(NSString *)consumerId
                   nextSeq:(NSString *)nextSeqText
                passphrase:(NSString *)passphrase
                       kek:(NSArray *)kek
            authPrincipal:(NSString *)authPrincipal
           authPassphrase:(NSString *)authPassphrase
                   resolve:(RCTPromiseResolveBlock)resolve
                    reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    uint64_t nextSeq = 0;
    if (!loomResolveU64(nextSeqText, &nextSeq, reject)) {
      return;
    }
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek];
    if (h == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_queue_consumer_reset(h, ns.UTF8String, stream.UTF8String,
                                     consumerId.UTF8String, nextSeq);
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

@end
