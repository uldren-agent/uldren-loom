#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Vector)

- (void)vectorCreate:(NSString *)loomPath
           workspace:(NSString *)ns
                name:(NSString *)name
                 dim:(double)dim
              metric:(double)metric
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
      st = loom_vector_create(h, ns.UTF8String, name.UTF8String, (uintptr_t)dim,
                              (int32_t)metric);
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

- (void)vectorUpsert:(NSString *)loomPath
           workspace:(NSString *)ns
                name:(NSString *)name
                  id:(NSString *)vecId
              vector:(NSArray *)vector
            metadata:(NSArray *)metadata
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
    unsigned char *vbuf = loomBytesFromArray(vector, &vlen);
    NSUInteger mlen = 0;
    unsigned char *mbuf = loomBytesFromArray(metadata, &mlen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_vector_upsert(h, ns.UTF8String, name.UTF8String, vecId.UTF8String, vbuf,
                              (uintptr_t)vlen, mbuf, (uintptr_t)mlen);
    }
    free(vbuf);
    free(mbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)vectorUpsertSource:(NSString *)loomPath
                 workspace:(NSString *)ns
                      name:(NSString *)name
                        id:(NSString *)vecId
                    vector:(NSArray *)vector
                  metadata:(NSArray *)metadata
                sourceText:(NSArray *)sourceText
                   modelId:(NSString *)modelId
             weightsDigest:(NSString *)weightsDigest
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
    unsigned char *vbuf = loomBytesFromArray(vector, &vlen);
    NSUInteger mlen = 0;
    unsigned char *mbuf = loomBytesFromArray(metadata, &mlen);
    NSUInteger slen = 0;
    unsigned char *sbuf = loomBytesFromArray(sourceText, &slen);
    const char *model = modelId == nil ? NULL : modelId.UTF8String;
    const char *weights = weightsDigest == nil ? NULL : weightsDigest.UTF8String;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_vector_upsert_source(
          h, ns.UTF8String, name.UTF8String, vecId.UTF8String, vbuf, (uintptr_t)vlen, mbuf,
          (uintptr_t)mlen, sbuf, (uintptr_t)slen, model, modelId == nil ? 0 : 1, weights,
          weightsDigest == nil ? 0 : 1);
    }
    free(vbuf);
    free(mbuf);
    free(sbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)vectorGet:(NSString *)loomPath
        workspace:(NSString *)ns
             name:(NSString *)name
               id:(NSString *)vecId
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
      st = loom_vector_get(h, ns.UTF8String, name.UTF8String, vecId.UTF8String, &ptr, &len,
                           &found);
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

- (void)vectorSourceText:(NSString *)loomPath
               workspace:(NSString *)ns
                    name:(NSString *)name
                      id:(NSString *)vecId
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
      st = loom_vector_source_text(h, ns.UTF8String, name.UTF8String, vecId.UTF8String,
                                   &ptr, &len, &found);
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

- (void)vectorEmbeddingModel:(NSString *)loomPath
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
      st = loom_vector_embedding_model_cbor(h, ns.UTF8String, name.UTF8String, &ptr, &len,
                                            &found);
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

- (void)vectorIds:(NSString *)loomPath
        workspace:(NSString *)ns
             name:(NSString *)name
           prefix:(NSString *)prefix
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
    const char *p = prefix == nil ? NULL : prefix.UTF8String;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_vector_ids_cbor(h, ns.UTF8String, name.UTF8String, p, prefix == nil ? 0 : 1,
                                &ptr, &len);
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

- (void)vectorMetadataIndexKeys:(NSString *)loomPath
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
      st = loom_vector_metadata_index_keys_cbor(h, ns.UTF8String, name.UTF8String, &ptr,
                                                &len);
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

- (void)vectorCreateMetadataIndex:(NSString *)loomPath
                        workspace:(NSString *)ns
                             name:(NSString *)name
                              key:(NSString *)key
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
    int32_t changed = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_vector_create_metadata_index(h, ns.UTF8String, name.UTF8String,
                                             key.UTF8String, &changed);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(changed != 0));
  });
}

- (void)vectorDropMetadataIndex:(NSString *)loomPath
                      workspace:(NSString *)ns
                           name:(NSString *)name
                            key:(NSString *)key
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
    int32_t changed = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_vector_drop_metadata_index(h, ns.UTF8String, name.UTF8String, key.UTF8String,
                                           &changed);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(changed != 0));
  });
}

- (void)vectorDelete:(NSString *)loomPath
           workspace:(NSString *)ns
                name:(NSString *)name
                  id:(NSString *)vecId
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
      st = loom_vector_delete(h, ns.UTF8String, name.UTF8String, vecId.UTF8String, &found);
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

- (void)vectorSearch:(NSString *)loomPath
           workspace:(NSString *)ns
                name:(NSString *)name
               query:(NSArray *)query
                   k:(double)k
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
    NSUInteger qlen = 0;
    unsigned char *qbuf = loomBytesFromArray(query, &qlen);
    NSUInteger flen = 0;
    unsigned char *fbuf = loomBytesFromArray(filter, &flen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_vector_search_cbor(h, ns.UTF8String, name.UTF8String, qbuf, (uintptr_t)qlen,
                                   (uintptr_t)k, fbuf, (uintptr_t)flen, &ptr, &len);
    }
    free(qbuf);
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

- (void)vectorSearchPolicy:(NSString *)loomPath
                 workspace:(NSString *)ns
                      name:(NSString *)name
                     query:(NSArray *)query
                         k:(double)k
                    filter:(NSArray *)filter
                    policy:(double)policy
                 threshold:(double)threshold
                        ef:(double)ef
                       pqM:(double)pqM
                       pqK:(double)pqK
                   pqIters:(double)pqIters
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
    NSUInteger qlen = 0;
    unsigned char *qbuf = loomBytesFromArray(query, &qlen);
    NSUInteger flen = 0;
    unsigned char *fbuf = loomBytesFromArray(filter, &flen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_vector_search_policy_cbor(
          h, ns.UTF8String, name.UTF8String, qbuf, (uintptr_t)qlen, (uintptr_t)k, fbuf,
          (uintptr_t)flen, (int32_t)policy, (uintptr_t)threshold, (uintptr_t)ef, (uintptr_t)pqM,
          (uintptr_t)pqK, (uintptr_t)pqIters, &ptr, &len);
    }
    free(qbuf);
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
