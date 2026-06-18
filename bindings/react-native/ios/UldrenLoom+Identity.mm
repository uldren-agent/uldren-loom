#import "UldrenLoom+Internal.h"

static BOOL loomAclScopeKind(NSString *kind, int32_t *out) {
  if ([kind isEqualToString:@"ref"]) {
    *out = 0;
  } else if ([kind isEqualToString:@"collection"]) {
    *out = 1;
  } else if ([kind isEqualToString:@"path"]) {
    *out = 2;
  } else if ([kind isEqualToString:@"key"]) {
    *out = 3;
  } else if ([kind isEqualToString:@"table"]) {
    *out = 4;
  } else if ([kind isEqualToString:@"exec"]) {
    *out = 5;
  } else {
    return NO;
  }
  return YES;
}

static NSError *loomNSError(NSInteger code, NSString *reason) {
  return [NSError errorWithDomain:@"LoomError"
                             code:code
                         userInfo:@{NSLocalizedDescriptionKey : reason}];
}

static BOOL loomAclScopeArrays(NSArray *scopes,
                               int32_t **outKinds,
                               const unsigned char ***outPrefixes,
                               uintptr_t **outLens,
                               NSMutableArray **outData,
                               NSError **outError) {
  NSUInteger count = scopes.count;
  *outKinds = NULL;
  *outPrefixes = NULL;
  *outLens = NULL;
  *outData = [NSMutableArray arrayWithCapacity:count];
  if (count == 0) {
    return YES;
  }
  int32_t *kinds = (int32_t *)calloc(count, sizeof(int32_t));
  const unsigned char **prefixes = (const unsigned char **)calloc(count, sizeof(unsigned char *));
  uintptr_t *lens = (uintptr_t *)calloc(count, sizeof(uintptr_t));
  if (kinds == NULL || prefixes == NULL || lens == NULL) {
    free(kinds);
    free(prefixes);
    free(lens);
    *outError = loomNSError(12, @"ACL scope allocation failed");
    return NO;
  }
  for (NSUInteger i = 0; i < count; i++) {
    if (![scopes[i] isKindOfClass:[NSString class]]) {
      free(kinds);
      free(prefixes);
      free(lens);
      *outError = loomNSError(22, @"ACL scope must be a string");
      return NO;
    }
    NSString *value = scopes[i];
    NSRange split = [value rangeOfString:@":"];
    if (split.location == NSNotFound || split.location == 0) {
      free(kinds);
      free(prefixes);
      free(lens);
      *outError = loomNSError(22, @"ACL scope must be kind:prefix");
      return NO;
    }
    NSString *kind = [value substringToIndex:split.location];
    if (!loomAclScopeKind(kind, &kinds[i])) {
      free(kinds);
      free(prefixes);
      free(lens);
      *outError = loomNSError(22, @"unknown ACL scope kind");
      return NO;
    }
    NSString *prefix = [value substringFromIndex:split.location + 1];
    NSData *data = [prefix dataUsingEncoding:NSUTF8StringEncoding];
    [*outData addObject:data];
    prefixes[i] = (const unsigned char *)data.bytes;
    lens[i] = (uintptr_t)data.length;
  }
  *outKinds = kinds;
  *outPrefixes = prefixes;
  *outLens = lens;
  return YES;
}

@implementation UldrenLoom (Identity)

- (void)authenticatePassphrase:(NSString *)loomPath
                     principal:(NSString *)principal
            principalPassphrase:(NSString *)principalPassphrase
                    passphrase:(NSString *)passphrase
                           kek:(NSArray *)kek
                       resolve:(RCTPromiseResolveBlock)resolve
                        reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek];
    if (h == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSData *pd = [principalPassphrase dataUsingEncoding:NSUTF8StringEncoding];
    int32_t st = loom_authenticate_passphrase(h, principal.UTF8String,
                                              (const unsigned char *)pd.bytes, (uintptr_t)pd.length);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)identityListJson:(NSString *)loomPath
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
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_identity_list_json(h, &out);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"{}";
    if (out) {
      loom_string_free(out);
    }
    resolve(result);
  });
}

- (void)identityAddPrincipal:(NSString *)loomPath
              principalHandle:(NSString *)principalHandle
                        name:(NSString *)name
                        kind:(NSString *)kind
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
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_identity_add_principal(h, principalHandle.UTF8String, name.UTF8String, kind.UTF8String, &out);
    }
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

- (void)identityRenamePrincipalHandle:(NSString *)loomPath
                             principal:(NSString *)principal
                       principalHandle:(NSString *)principalHandle
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
      st = loom_identity_rename_principal_handle(h, principal.UTF8String, principalHandle.UTF8String);
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

- (void)identitySetPassphrase:(NSString *)loomPath
                    principal:(NSString *)principal
           principalPassphrase:(NSString *)principalPassphrase
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
    NSData *pd = [principalPassphrase dataUsingEncoding:NSUTF8StringEncoding];
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_identity_set_passphrase(h, principal.UTF8String,
                                        (const unsigned char *)pd.bytes, (uintptr_t)pd.length);
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

- (void)identityRemovePrincipal:(NSString *)loomPath
                      principal:(NSString *)principal
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
      st = loom_identity_remove_principal(h, principal.UTF8String);
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

- (void)identityAssignRole:(NSString *)loomPath
                 principal:(NSString *)principal
                      role:(NSString *)role
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
      st = loom_identity_assign_role(h, principal.UTF8String, role.UTF8String);
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

- (void)identityRevokeRole:(NSString *)loomPath
                 principal:(NSString *)principal
                      role:(NSString *)role
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
    int32_t removed = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_identity_revoke_role(h, principal.UTF8String, role.UTF8String, &removed);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(removed != 0));
  });
}

- (void)identityCreateExternalCredential:(NSString *)loomPath
                               principal:(NSString *)principal
                                    kind:(NSString *)kind
                                   label:(NSString *)label
                                  issuer:(NSString *)issuer
                                 subject:(NSString *)subject
                          materialDigest:(NSString *)materialDigest
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
    char *out = NULL;
    const char *digest = materialDigest.length > 0 ? materialDigest.UTF8String : NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_identity_create_external_credential(
          h, principal.UTF8String, kind.UTF8String, label.UTF8String, issuer.UTF8String,
          subject.UTF8String, digest, &out);
    }
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

- (void)identityRevokeExternalCredential:(NSString *)loomPath
                              credential:(NSString *)credential
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
      st = loom_identity_revoke_external_credential(h, credential.UTF8String);
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

- (void)identityAddPublicKey:(NSString *)loomPath
                   principal:(NSString *)principal
                       label:(NSString *)label
                   algorithm:(NSString *)algorithm
                publicKeyHex:(NSString *)publicKeyHex
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
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_identity_add_public_key(
          h, principal.UTF8String, label.UTF8String, algorithm.UTF8String,
          publicKeyHex.UTF8String, &out);
    }
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

- (void)identityRevokePublicKey:(NSString *)loomPath
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
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_identity_revoke_public_key(h, key.UTF8String);
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

- (void)aclListJson:(NSString *)loomPath
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
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_acl_list_json(h, &out);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"[]";
    if (out) {
      loom_string_free(out);
    }
    resolve(result);
  });
}

- (void)aclGrant:(NSString *)loomPath
          effect:(double)effect
         subject:(NSString *)subject
       workspace:(NSString *)ns
           domain:(NSString *)domain
      rightsMask:(double)rightsMask
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
      st = loom_acl_grant(h, (int32_t)effect, subject.UTF8String,
                          ns.length ? ns.UTF8String : NULL,
                          domain.length ? domain.UTF8String : NULL,
                          (uint32_t)rightsMask);
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

- (void)aclGrantScoped:(NSString *)loomPath
                effect:(double)effect
               subject:(NSString *)subject
             workspace:(NSString *)ns
                 domain:(NSString *)domain
            rightsMask:(double)rightsMask
               refGlob:(NSString *)refGlob
                scopes:(NSArray *)scopes
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
    int32_t *scopeKinds = NULL;
    const unsigned char **scopePrefixes = NULL;
    uintptr_t *scopeLens = NULL;
    NSMutableArray *scopeData = nil;
    NSError *scopeError = nil;
    if (!loomAclScopeArrays(scopes, &scopeKinds, &scopePrefixes, &scopeLens, &scopeData, &scopeError)) {
      loom_close(h);
      reject([@(scopeError.code) stringValue], scopeError.localizedDescription, scopeError);
      return;
    }
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_acl_grant_scoped(h, (int32_t)effect, subject.UTF8String,
                                 ns.length ? ns.UTF8String : NULL,
                                 domain.length ? domain.UTF8String : NULL,
                                 (uint32_t)rightsMask,
                                 refGlob.length ? refGlob.UTF8String : NULL,
                                 (uintptr_t)scopes.count, scopeKinds, scopePrefixes, scopeLens);
    }
    free(scopeKinds);
    free(scopePrefixes);
    free(scopeLens);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)aclGrantScopedPredicate:(NSString *)loomPath
                         effect:(double)effect
                        subject:(NSString *)subject
                      workspace:(NSString *)ns
                          domain:(NSString *)domain
                     rightsMask:(double)rightsMask
                        refGlob:(NSString *)refGlob
                         scopes:(NSArray *)scopes
                   predicateCel:(NSString *)predicateCel
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
    int32_t *scopeKinds = NULL;
    const unsigned char **scopePrefixes = NULL;
    uintptr_t *scopeLens = NULL;
    NSMutableArray *scopeData = nil;
    NSError *scopeError = nil;
    if (!loomAclScopeArrays(scopes, &scopeKinds, &scopePrefixes, &scopeLens, &scopeData, &scopeError)) {
      loom_close(h);
      reject([@(scopeError.code) stringValue], scopeError.localizedDescription, scopeError);
      return;
    }
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_acl_grant_scoped_predicate(h, (int32_t)effect, subject.UTF8String,
                                           ns.length ? ns.UTF8String : NULL,
                                           domain.length ? domain.UTF8String : NULL,
                                           (uint32_t)rightsMask,
                                           refGlob.length ? refGlob.UTF8String : NULL,
                                           (uintptr_t)scopes.count, scopeKinds, scopePrefixes, scopeLens,
                                           predicateCel.length ? "cel" : NULL,
                                           predicateCel.length ? predicateCel.UTF8String : NULL);
    }
    free(scopeKinds);
    free(scopePrefixes);
    free(scopeLens);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)aclRevoke:(NSString *)loomPath
           effect:(double)effect
          subject:(NSString *)subject
        workspace:(NSString *)ns
            domain:(NSString *)domain
       rightsMask:(double)rightsMask
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
    int32_t removed = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_acl_revoke(h, (int32_t)effect, subject.UTF8String,
                           ns.length ? ns.UTF8String : NULL,
                           domain.length ? domain.UTF8String : NULL,
                           (uint32_t)rightsMask, &removed);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(removed != 0));
  });
}

- (void)aclRevokeScoped:(NSString *)loomPath
                 effect:(double)effect
                subject:(NSString *)subject
              workspace:(NSString *)ns
                  domain:(NSString *)domain
             rightsMask:(double)rightsMask
                refGlob:(NSString *)refGlob
                 scopes:(NSArray *)scopes
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
    int32_t *scopeKinds = NULL;
    const unsigned char **scopePrefixes = NULL;
    uintptr_t *scopeLens = NULL;
    NSMutableArray *scopeData = nil;
    NSError *scopeError = nil;
    if (!loomAclScopeArrays(scopes, &scopeKinds, &scopePrefixes, &scopeLens, &scopeData, &scopeError)) {
      loom_close(h);
      reject([@(scopeError.code) stringValue], scopeError.localizedDescription, scopeError);
      return;
    }
    int32_t removed = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_acl_revoke_scoped(h, (int32_t)effect, subject.UTF8String,
                                  ns.length ? ns.UTF8String : NULL,
                                  domain.length ? domain.UTF8String : NULL,
                                  (uint32_t)rightsMask,
                                  refGlob.length ? refGlob.UTF8String : NULL,
                                  (uintptr_t)scopes.count, scopeKinds, scopePrefixes, scopeLens,
                                  &removed);
    }
    free(scopeKinds);
    free(scopePrefixes);
    free(scopeLens);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(removed != 0));
  });
}

- (void)aclRevokeScopedPredicate:(NSString *)loomPath
                          effect:(double)effect
                         subject:(NSString *)subject
                       workspace:(NSString *)ns
                           domain:(NSString *)domain
                      rightsMask:(double)rightsMask
                         refGlob:(NSString *)refGlob
                          scopes:(NSArray *)scopes
                    predicateCel:(NSString *)predicateCel
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
    int32_t *scopeKinds = NULL;
    const unsigned char **scopePrefixes = NULL;
    uintptr_t *scopeLens = NULL;
    NSMutableArray *scopeData = nil;
    NSError *scopeError = nil;
    if (!loomAclScopeArrays(scopes, &scopeKinds, &scopePrefixes, &scopeLens, &scopeData, &scopeError)) {
      loom_close(h);
      reject([@(scopeError.code) stringValue], scopeError.localizedDescription, scopeError);
      return;
    }
    int32_t removed = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_acl_revoke_scoped_predicate(h, (int32_t)effect, subject.UTF8String,
                                            ns.length ? ns.UTF8String : NULL,
                                            domain.length ? domain.UTF8String : NULL,
                                            (uint32_t)rightsMask,
                                            refGlob.length ? refGlob.UTF8String : NULL,
                                            (uintptr_t)scopes.count, scopeKinds, scopePrefixes, scopeLens,
                                            predicateCel.length ? "cel" : NULL,
                                            predicateCel.length ? predicateCel.UTF8String : NULL,
                                            &removed);
    }
    free(scopeKinds);
    free(scopePrefixes);
    free(scopeLens);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(removed != 0));
  });
}

- (void)protectedRefListJson:(NSString *)loomPath
                   workspace:(NSString *)ns
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
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_protected_ref_list_json(h, ns.UTF8String, &out);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"[]";
    if (out) {
      loom_string_free(out);
    }
    resolve(result);
  });
}

- (void)protectedRefGetJson:(NSString *)loomPath
                  workspace:(NSString *)ns
                    refName:(NSString *)refName
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
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_protected_ref_get_json(h, ns.UTF8String, refName.UTF8String, &out);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"null";
    if (out) {
      loom_string_free(out);
    }
    resolve(result);
  });
}

- (void)protectedRefSet:(NSString *)loomPath
              workspace:(NSString *)ns
                refName:(NSString *)refName
        fastForwardOnly:(BOOL)fastForwardOnly
  signedCommitsRequired:(BOOL)signedCommitsRequired
signedRefAdvanceRequired:(BOOL)signedRefAdvanceRequired
    requiredReviewCount:(double)requiredReviewCount
          retentionLock:(BOOL)retentionLock
         governanceLock:(BOOL)governanceLock
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
      st = loom_protected_ref_set(h, ns.UTF8String, refName.UTF8String,
                                  fastForwardOnly, signedCommitsRequired,
                                  signedRefAdvanceRequired, (uint32_t)requiredReviewCount,
                                  retentionLock, governanceLock);
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

- (void)protectedRefRemove:(NSString *)loomPath
                 workspace:(NSString *)ns
                   refName:(NSString *)refName
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
    int32_t removed = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_protected_ref_remove(h, ns.UTF8String, refName.UTF8String, &removed);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(@(removed != 0));
  });
}

@end
