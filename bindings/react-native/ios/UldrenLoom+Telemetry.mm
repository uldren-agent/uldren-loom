#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Telemetry)

- (void)metricsPutDescriptor:(NSString *)loomPath
                   workspace:(NSString *)ns
                  descriptor:(NSArray *)descriptor
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
    unsigned char *buf = loomBytesFromArray(descriptor, &len);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_metrics_put_descriptor(h, ns.UTF8String, buf, (uintptr_t)len);
    }
    free(buf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)metricsGetDescriptor:(NSString *)loomPath
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
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_metrics_get_descriptor(h, ns.UTF8String, name.UTF8String, &ptr, &len, &found);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(found == 0 ? nil : loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)metricsPutObservation:(NSString *)loomPath
                    workspace:(NSString *)ns
               descriptorName:(NSString *)descriptorName
                  observation:(NSArray *)observation
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
    unsigned char *buf = loomBytesFromArray(observation, &len);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_metrics_put_observation(h, ns.UTF8String, descriptorName.UTF8String, buf, (uintptr_t)len);
    }
    free(buf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)metricsQuery:(NSString *)loomPath
           workspace:(NSString *)ns
      descriptorName:(NSString *)descriptorName
     fromTimestampMs:(NSString *)fromTimestampMs
       toTimestampMs:(NSString *)toTimestampMs
           maxSeries:(double)maxSeries
           maxGroups:(double)maxGroups
          maxSamples:(double)maxSamples
      maxOutputBytes:(NSString *)maxOutputBytes
      nowTimestampMs:(NSString *)nowTimestampMs
          passphrase:(NSString *)passphrase
                 kek:(NSArray *)kek
       authPrincipal:(NSString *)authPrincipal
      authPassphrase:(NSString *)authPassphrase
             resolve:(RCTPromiseResolveBlock)resolve
              reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    uint64_t from = 0, to = 0, maxBytes = 0, now = 0;
    if (!loomResolveU64(fromTimestampMs, &from, reject) || !loomResolveU64(toTimestampMs, &to, reject) ||
        !loomResolveU64(maxOutputBytes, &maxBytes, reject) || !loomResolveU64(nowTimestampMs, &now, reject)) {
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
      st = loom_metrics_query_cbor(h, ns.UTF8String, descriptorName.UTF8String, from, to,
                                   (uint32_t)maxSeries, (uint32_t)maxGroups, (uint32_t)maxSamples,
                                   maxBytes, now, &ptr, &len);
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

- (void)logsPutRecord:(NSString *)loomPath
            workspace:(NSString *)ns
               record:(NSArray *)record
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
    NSUInteger inLen = 0;
    unsigned char *buf = loomBytesFromArray(record, &inLen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_logs_put_record(h, ns.UTF8String, buf, (uintptr_t)inLen, &ptr, &len);
    }
    free(buf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSData *data = [NSData dataWithBytes:ptr length:(NSUInteger)len];
    loom_bytes_free(ptr, len);
    resolve([[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] ?: @"");
  });
}

- (void)logsGetRecord:(NSString *)loomPath
            workspace:(NSString *)ns
             recordId:(NSString *)recordId
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
      st = loom_logs_get_record(h, ns.UTF8String, recordId.UTF8String, &ptr, &len, &found);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(found == 0 ? nil : loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)logsQuery:(NSString *)loomPath
        workspace:(NSString *)ns
 fromTimeUnixNano:(NSString *)fromTimeUnixNano
   toTimeUnixNano:(NSString *)toTimeUnixNano
       maxRecords:(double)maxRecords
   maxOutputBytes:(NSString *)maxOutputBytes
       passphrase:(NSString *)passphrase
              kek:(NSArray *)kek
    authPrincipal:(NSString *)authPrincipal
   authPassphrase:(NSString *)authPassphrase
          resolve:(RCTPromiseResolveBlock)resolve
           reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    uint64_t from = 0, to = 0, maxBytes = 0;
    if (!loomResolveU64(fromTimeUnixNano, &from, reject) || !loomResolveU64(toTimeUnixNano, &to, reject) ||
        !loomResolveU64(maxOutputBytes, &maxBytes, reject)) {
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
      st = loom_logs_query_cbor(h, ns.UTF8String, from, to, (uint32_t)maxRecords, maxBytes, &ptr, &len);
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

- (void)tracesPutSpan:(NSString *)loomPath
            workspace:(NSString *)ns
                 span:(NSArray *)span
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
    unsigned char *buf = loomBytesFromArray(span, &len);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_traces_put_span(h, ns.UTF8String, buf, (uintptr_t)len);
    }
    free(buf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)tracesGetSpan:(NSString *)loomPath
            workspace:(NSString *)ns
              traceId:(NSString *)traceId
               spanId:(NSString *)spanId
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
      st = loom_traces_get_span(h, ns.UTF8String, traceId.UTF8String, spanId.UTF8String, &ptr, &len, &found);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(found == 0 ? nil : loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)tracesTraceSpans:(NSString *)loomPath
               workspace:(NSString *)ns
                 traceId:(NSString *)traceId
                maxSpans:(double)maxSpans
          maxOutputBytes:(NSString *)maxOutputBytes
              passphrase:(NSString *)passphrase
                     kek:(NSArray *)kek
           authPrincipal:(NSString *)authPrincipal
          authPassphrase:(NSString *)authPassphrase
                 resolve:(RCTPromiseResolveBlock)resolve
                  reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    uint64_t maxBytes = 0;
    if (!loomResolveU64(maxOutputBytes, &maxBytes, reject)) {
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
      st = loom_traces_trace_spans_cbor(h, ns.UTF8String, traceId.UTF8String, (uint32_t)maxSpans, maxBytes, &ptr, &len);
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

- (void)tracesQuery:(NSString *)loomPath
          workspace:(NSString *)ns
    fromStartTimeNs:(NSString *)fromStartTimeNs
      toStartTimeNs:(NSString *)toStartTimeNs
           maxSpans:(double)maxSpans
     maxOutputBytes:(NSString *)maxOutputBytes
         passphrase:(NSString *)passphrase
                kek:(NSArray *)kek
      authPrincipal:(NSString *)authPrincipal
     authPassphrase:(NSString *)authPassphrase
            resolve:(RCTPromiseResolveBlock)resolve
             reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    uint64_t from = 0, to = 0, maxBytes = 0;
    if (!loomResolveU64(fromStartTimeNs, &from, reject) || !loomResolveU64(toStartTimeNs, &to, reject) ||
        !loomResolveU64(maxOutputBytes, &maxBytes, reject)) {
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
      st = loom_traces_query_cbor(h, ns.UTF8String, from, to, (uint32_t)maxSpans, maxBytes, &ptr, &len);
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
