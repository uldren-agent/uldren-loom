#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Graph)

- (void)graphUpsertNode:(NSString *)loomPath
              workspace:(NSString *)ns
                   name:(NSString *)name
                     id:(NSString *)nodeId
                  props:(NSArray *)props
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
    NSUInteger plen = 0;
    unsigned char *pbuf = loomBytesFromArray(props, &plen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_graph_upsert_node(h, ns.UTF8String, name.UTF8String, nodeId.UTF8String, pbuf,
                                  (uintptr_t)plen);
    }
    free(pbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)graphGetNode:(NSString *)loomPath
           workspace:(NSString *)ns
                name:(NSString *)name
                  id:(NSString *)nodeId
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
      st = loom_graph_get_node(h, ns.UTF8String, name.UTF8String, nodeId.UTF8String, &ptr, &len,
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

- (void)graphRemoveNode:(NSString *)loomPath
              workspace:(NSString *)ns
                   name:(NSString *)name
                     id:(NSString *)nodeId
                cascade:(BOOL)cascade
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
      st = loom_graph_remove_node(h, ns.UTF8String, name.UTF8String, nodeId.UTF8String,
                                  cascade ? 1 : 0);
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

- (void)graphUpsertEdge:(NSString *)loomPath
              workspace:(NSString *)ns
                   name:(NSString *)name
                     id:(NSString *)edgeId
                    src:(NSString *)src
                    dst:(NSString *)dst
                  label:(NSString *)label
                  props:(NSArray *)props
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
    NSUInteger plen = 0;
    unsigned char *pbuf = loomBytesFromArray(props, &plen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_graph_upsert_edge(h, ns.UTF8String, name.UTF8String, edgeId.UTF8String,
                                  src.UTF8String, dst.UTF8String, label.UTF8String, pbuf,
                                  (uintptr_t)plen);
    }
    free(pbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)graphGetEdge:(NSString *)loomPath
           workspace:(NSString *)ns
                name:(NSString *)name
                  id:(NSString *)edgeId
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
      st = loom_graph_get_edge(h, ns.UTF8String, name.UTF8String, edgeId.UTF8String, &ptr, &len,
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

- (void)graphRemoveEdge:(NSString *)loomPath
              workspace:(NSString *)ns
                   name:(NSString *)name
                     id:(NSString *)edgeId
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
      st = loom_graph_remove_edge(h, ns.UTF8String, name.UTF8String, edgeId.UTF8String, &found);
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

- (void)graphNeighbors:(NSString *)loomPath
             workspace:(NSString *)ns
                  name:(NSString *)name
                    id:(NSString *)nodeId
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
      st = loom_graph_neighbors_cbor(h, ns.UTF8String, name.UTF8String, nodeId.UTF8String, &ptr,
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

- (void)graphOutEdges:(NSString *)loomPath
            workspace:(NSString *)ns
                 name:(NSString *)name
                   id:(NSString *)nodeId
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
      st = loom_graph_out_edges_cbor(h, ns.UTF8String, name.UTF8String, nodeId.UTF8String, &ptr,
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

- (void)graphInEdges:(NSString *)loomPath
           workspace:(NSString *)ns
                name:(NSString *)name
                  id:(NSString *)nodeId
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
      st = loom_graph_in_edges_cbor(h, ns.UTF8String, name.UTF8String, nodeId.UTF8String, &ptr,
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

- (void)graphReachable:(NSString *)loomPath
             workspace:(NSString *)ns
                  name:(NSString *)name
                 start:(NSString *)start
              maxDepth:(double)maxDepth
              viaLabel:(NSString *)viaLabel
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
    const char *via = (viaLabel != nil) ? viaLabel.UTF8String : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_graph_reachable_cbor(h, ns.UTF8String, name.UTF8String, start.UTF8String,
                                     (int64_t)maxDepth, via, &ptr, &len);
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

- (void)graphShortestPath:(NSString *)loomPath
                workspace:(NSString *)ns
                     name:(NSString *)name
                     from:(NSString *)from
                       to:(NSString *)to
                 viaLabel:(NSString *)viaLabel
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
    const char *via = (viaLabel != nil) ? viaLabel.UTF8String : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_graph_shortest_path_cbor(h, ns.UTF8String, name.UTF8String, from.UTF8String,
                                         to.UTF8String, via, &ptr, &len, &found);
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

@end
