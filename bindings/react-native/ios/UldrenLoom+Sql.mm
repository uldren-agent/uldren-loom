#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Sql)

- (void)sqlExecTyped:(NSString *)loomPath
                  ns:(NSString *)ns
                  db:(NSString *)db
                 sql:(NSString *)sql
          passphrase:(NSString *)passphrase
                 kek:(NSArray *)kek
       authPrincipal:(NSString *)authPrincipal
      authPassphrase:(NSString *)authPassphrase
             resolve:(RCTPromiseResolveBlock)resolve
              reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSqlSession *s = [self openSession:loomPath ns:ns db:db passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase];
    if (s == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    if (loom_sql_exec(s, sql.UTF8String, &ptr, &len) != 0) {
      NSError *err = [self loomError];
      loom_sql_close(s);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    char *json = NULL;
    int32_t rst = loom_result_to_bridge_json(ptr, len, &json);
    loom_bytes_free(ptr, len);
    if (rst != 0) {
      NSError *err = [self loomError];
      loom_sql_close(s);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = json ? [NSString stringWithUTF8String:json] : @"";
    if (json) {
      loom_string_free(json);
    }
    loom_sql_close(s);
    resolve(result);
  });
}

// Atomic transaction/batch in one native round-trip: open a held-open batch, run every
// statement in order (incl. BEGIN/COMMIT/ROLLBACK), commit with one atomic save on success, abort and
// discard on any error. The writer lock stays entirely inside native code, off the JS thread. Resolves
// the lossless bridge JSON of the final statement's result.

- (void)sqlBatch:(NSString *)loomPath
              ns:(NSString *)ns
              db:(NSString *)db
      statements:(NSArray *)statements
      passphrase:(NSString *)passphrase
             kek:(NSArray *)kek
   authPrincipal:(NSString *)authPrincipal
  authPassphrase:(NSString *)authPassphrase
         resolve:(RCTPromiseResolveBlock)resolve
          reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSqlBatch *b = [self beginBatch:loomPath ns:ns db:db passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase];
    if (b == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    char *json = NULL;  // bridge JSON of the most recent statement's result
    BOOL ok = YES;
    for (id stmt in statements) {
      NSString *s = (NSString *)stmt;
      unsigned char *ptr = NULL;
      uintptr_t len = 0;
      if (loom_sql_batch_exec(b, s.UTF8String, &ptr, &len) != 0) {
        ok = NO;
        break;
      }
      if (json) {
        loom_string_free(json);
        json = NULL;
      }
      int32_t rst = loom_result_to_bridge_json(ptr, len, &json);
      loom_bytes_free(ptr, len);
      if (rst != 0) {
        ok = NO;
        break;
      }
    }
    if (ok && loom_sql_batch_commit(b) != 0) {
      ok = NO;
    }
    if (!ok) {
      NSError *err = [self loomError];
      if (json) {
        loom_string_free(json);
      }
      loom_sql_batch_abort(b);
      loom_sql_batch_close(b);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = json ? [NSString stringWithUTF8String:json] : @"[]";
    if (json) {
      loom_string_free(json);
    }
    loom_sql_batch_close(b);
    resolve(result);
  });
}

// Resolves the JSON debug form (rendered from the canonical-CBOR result via loom_result_to_json).

- (void)sqlExecJson:(NSString *)loomPath
                 ns:(NSString *)ns
                 db:(NSString *)db
                sql:(NSString *)sql
         passphrase:(NSString *)passphrase
                kek:(NSArray *)kek
      authPrincipal:(NSString *)authPrincipal
     authPassphrase:(NSString *)authPassphrase
            resolve:(RCTPromiseResolveBlock)resolve
             reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSqlSession *s = [self openSession:loomPath ns:ns db:db passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase];
    if (s == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    if (loom_sql_exec(s, sql.UTF8String, &ptr, &len) != 0) {
      NSError *err = [self loomError];
      loom_sql_close(s);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    char *json = NULL;
    int32_t rst = loom_result_to_json(ptr, len, &json);
    loom_bytes_free(ptr, len);
    if (rst != 0) {
      NSError *err = [self loomError];
      loom_sql_close(s);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = json ? [NSString stringWithUTF8String:json] : @"";
    if (json) {
      loom_string_free(json);
    }
    loom_sql_close(s);
    resolve(result);
  });
}

// As `sqlExec`, but resolves the result payloads as canonical-CBOR bytes - a
// 0-255 number array over the bridge (the type-faithful form).

- (void)sqlExecBytes:(NSString *)loomPath
                  ns:(NSString *)ns
                  db:(NSString *)db
                 sql:(NSString *)sql
          passphrase:(NSString *)passphrase
                 kek:(NSArray *)kek
       authPrincipal:(NSString *)authPrincipal
      authPassphrase:(NSString *)authPassphrase
             resolve:(RCTPromiseResolveBlock)resolve
              reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSqlSession *s = [self openSession:loomPath ns:ns db:db passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase];
    if (s == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    if (loom_sql_exec(s, sql.UTF8String, &ptr, &len) != 0) {
      NSError *err = [self loomError];
      loom_sql_close(s);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSMutableArray *out = [NSMutableArray arrayWithCapacity:(NSUInteger)len];
    for (uintptr_t i = 0; i < len; i++) {
      [out addObject:@(ptr[i])];
    }
    loom_bytes_free(ptr, len);
    loom_sql_close(s);
    resolve(out);
  });
}

- (void)sqlQueryBytes:(NSString *)loomPath
                   ns:(NSString *)ns
                   db:(NSString *)db
                  sql:(NSString *)sql
           passphrase:(NSString *)passphrase
                  kek:(NSArray *)kek
        authPrincipal:(NSString *)authPrincipal
       authPassphrase:(NSString *)authPassphrase
              resolve:(RCTPromiseResolveBlock)resolve
               reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSqlSession *s = [self openSession:loomPath ns:ns db:db passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase];
    if (s == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    LoomIter *it = NULL;
    if (loom_sql_query(s, sql.UTF8String, &it) != 0) {
      NSError *err = [self loomError];
      loom_sql_close(s);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSMutableArray *rows = [NSMutableArray array];
    for (;;) {
      unsigned char *ptr = NULL;
      uintptr_t len = 0;
      int32_t done = 0;
      if (loom_iter_next(it, &ptr, &len, &done) != 0) {
        NSError *err = [self loomError];
        loom_iter_free(it);
        loom_sql_close(s);
        reject([@(err.code) stringValue], err.localizedDescription, err);
        return;
      }
      if (done != 0) {
        break;
      }
      [rows addObject:loomArrayFromOwnedBytes(ptr, len)];
    }
    loom_iter_free(it);
    loom_sql_close(s);
    resolve(rows);
  });
}

- (void)sqlCommit:(NSString *)loomPath
               ns:(NSString *)ns
               db:(NSString *)db
          message:(NSString *)message
           author:(NSString *)author
       passphrase:(NSString *)passphrase
              kek:(NSArray *)kek
    authPrincipal:(NSString *)authPrincipal
   authPassphrase:(NSString *)authPassphrase
          resolve:(RCTPromiseResolveBlock)resolve
           reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSqlSession *s = [self openSession:loomPath ns:ns db:db passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase];
    if (s == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    char *out = NULL;
    if (loom_sql_commit(s, message.UTF8String, author.UTF8String, &out) != 0) {
      NSError *err = [self loomError];
      loom_sql_close(s);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"";
    if (out) {
      loom_string_free(out);
    }
    loom_sql_close(s);
    resolve(result);
  });
}

// Copy a 0-255 NSArray into a freshly malloc'd byte buffer of `*outLen` bytes (caller frees).

- (void)sqlReadTable:(NSString *)loomPath
        workspace:(NSString *)ns
            table:(NSString *)table
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
      st = loom_sql_read_table(h, ns.UTF8String, table.UTF8String, &ptr, &len);
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

- (void)sqlReadTableAt:(NSString *)loomPath
        workspace:(NSString *)ns
            table:(NSString *)table
           commit:(NSString *)commit
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
      st = loom_sql_read_table_at(h, ns.UTF8String, table.UTF8String,
                                        commit.UTF8String, &ptr, &len);
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

- (void)sqlIndexScan:(NSString *)loomPath
        workspace:(NSString *)ns
            table:(NSString *)table
            index:(NSString *)index
           prefix:(NSArray *)prefix
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
    unsigned char *pbuf = loomBytesFromArray(prefix, &plen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_sql_index_scan(h, ns.UTF8String, table.UTF8String,
                                 index.UTF8String, pbuf, (uintptr_t)plen, &ptr, &len);
    }
    free(pbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)sqlIndexScanAt:(NSString *)loomPath
        workspace:(NSString *)ns
            table:(NSString *)table
            index:(NSString *)index
           prefix:(NSArray *)prefix
           commit:(NSString *)commit
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
    unsigned char *pbuf = loomBytesFromArray(prefix, &plen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_sql_index_scan_at(h, ns.UTF8String, table.UTF8String,
                                        index.UTF8String, pbuf, (uintptr_t)plen,
                                        commit.UTF8String, &ptr, &len);
    }
    free(pbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (void)sqlBlame:(NSString *)loomPath
         workspace:(NSString *)ns
            branch:(NSString *)branch
             table:(NSString *)table
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
      st = loom_sql_blame(h, ns.UTF8String, branch.UTF8String,
                                  table.UTF8String, &ptr, &len);
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

- (void)sqlDiff:(NSString *)loomPath
        workspace:(NSString *)ns
            table:(NSString *)table
       fromCommit:(NSString *)fromCommit
         toCommit:(NSString *)toCommit
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
      st = loom_sql_diff(h, ns.UTF8String, table.UTF8String,
                                 fromCommit.UTF8String, toCommit.UTF8String, &ptr, &len);
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

- (void)sqlTableDiff:(NSString *)loomPath
        workspace:(NSString *)ns
            table:(NSString *)table
       fromCommit:(NSString *)fromCommit
         toCommit:(NSString *)toCommit
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
      st = loom_sql_table_diff(h, ns.UTF8String, table.UTF8String,
                                     fromCommit.UTF8String, toCommit.UTF8String, &ptr, &len);
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
