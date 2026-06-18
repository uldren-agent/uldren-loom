#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Mail)

- (void)mailCreateMailbox:(NSString *)loomPath
                workspace:(NSString *)ns
                principal:(NSString *)principal
                  mailbox:(NSString *)mailbox
              displayName:(NSString *)displayName
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
      st = loom_mail_create_mailbox(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                          displayName.UTF8String);
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

- (void)mailDeleteMailbox:(NSString *)loomPath
                workspace:(NSString *)ns
                principal:(NSString *)principal
                  mailbox:(NSString *)mailbox
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
      st = loom_mail_delete_mailbox(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                          &found);
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

- (void)mailListMailboxes:(NSString *)loomPath
                workspace:(NSString *)ns
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
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_mail_list_mailboxes(h, ns.UTF8String, principal.UTF8String, &ptr, &len);
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

- (void)mailIngestMessage:(NSString *)loomPath
                workspace:(NSString *)ns
                principal:(NSString *)principal
                  mailbox:(NSString *)mailbox
                      uid:(NSString *)uid
                      raw:(NSArray *)raw
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
    unsigned char *rbuf = loomBytesFromArray(raw, &rlen);
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_mail_ingest_message(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                          uid.UTF8String, rbuf, (uintptr_t)rlen, &out);
    }
    free(rbuf);
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

- (void)mailGetMessage:(NSString *)loomPath
             workspace:(NSString *)ns
             principal:(NSString *)principal
               mailbox:(NSString *)mailbox
                   uid:(NSString *)uid
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
      st = loom_mail_get_message(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                       uid.UTF8String, &ptr, &len, &found);
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

- (void)mailToEml:(NSString *)loomPath
          workspace:(NSString *)ns
          principal:(NSString *)principal
            mailbox:(NSString *)mailbox
                uid:(NSString *)uid
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
      st = loom_mail_to_eml(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                    uid.UTF8String, &ptr, &len, &found);
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

- (void)mailDeleteMessage:(NSString *)loomPath
                workspace:(NSString *)ns
                principal:(NSString *)principal
                  mailbox:(NSString *)mailbox
                      uid:(NSString *)uid
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
      st = loom_mail_delete_message(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                          uid.UTF8String, &found);
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

- (void)mailListMessages:(NSString *)loomPath
               workspace:(NSString *)ns
               principal:(NSString *)principal
                 mailbox:(NSString *)mailbox
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
      st = loom_mail_list_messages(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
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

- (void)mailGetFlags:(NSString *)loomPath
           workspace:(NSString *)ns
           principal:(NSString *)principal
             mailbox:(NSString *)mailbox
                 uid:(NSString *)uid
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
      st = loom_mail_get_flags(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                     uid.UTF8String, &ptr, &len);
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

- (void)mailSetFlags:(NSString *)loomPath
           workspace:(NSString *)ns
           principal:(NSString *)principal
             mailbox:(NSString *)mailbox
                 uid:(NSString *)uid
               flags:(NSArray *)flags
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
    NSUInteger flen = 0;
    unsigned char *fbuf = loomBytesFromArray(flags, &flen);
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = loom_mail_set_flags(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                     uid.UTF8String, fbuf, (uintptr_t)flen);
    }
    free(fbuf);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(nil);
  });
}

- (void)mailSearch:(NSString *)loomPath
         workspace:(NSString *)ns
         principal:(NSString *)principal
           mailbox:(NSString *)mailbox
              text:(NSString *)text
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
      st = loom_mail_search(h, ns.UTF8String, principal.UTF8String, mailbox.UTF8String,
                                  text.UTF8String, &ptr, &len);
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

// Bridges to the codegen'd C++ TurboModule.

@end
