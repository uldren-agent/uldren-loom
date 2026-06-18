#import "UldrenLoom+Internal.h"
#include <errno.h>
#include <stdlib.h>

unsigned char *loomBytesFromArray(NSArray *arr, NSUInteger *outLen) {
  NSUInteger n = arr.count;
  *outLen = n;
  unsigned char *buf = (unsigned char *)malloc(n ? n : 1);
  for (NSUInteger i = 0; i < n; i++) {
    buf[i] = (unsigned char)([arr[i] integerValue] & 0xFF);
  }
  return buf;
}

// Build an NSArray of 0-255 numbers from an owned (ptr, len) buffer and free the native buffer.

NSArray *loomArrayFromOwnedBytes(unsigned char *ptr, uintptr_t len) {
  NSMutableArray *out = [NSMutableArray arrayWithCapacity:(NSUInteger)len];
  for (uintptr_t i = 0; i < len; i++) {
    [out addObject:@(ptr[i])];
  }
  loom_bytes_free(ptr, len);
  return out;
}

NSString *loomStringFromU64(uint64_t value) {
  return [NSString stringWithFormat:@"%llu", (unsigned long long)value];
}

BOOL loomParseU64(NSString *value, uint64_t *out) {
  if (![value isKindOfClass:[NSString class]] || value.length == 0) {
    return NO;
  }
  const char *chars = value.UTF8String;
  if (chars == NULL || chars[0] == '+' || chars[0] == '-') {
    return NO;
  }
  for (const char *p = chars; *p != '\0'; p++) {
    if (*p < '0' || *p > '9') {
      return NO;
    }
  }
  errno = 0;
  char *end = NULL;
  unsigned long long parsed = strtoull(chars, &end, 10);
  if (errno == ERANGE || end == NULL || *end != '\0') {
    return NO;
  }
  *out = (uint64_t)parsed;
  return YES;
}

BOOL loomResolveU64(NSString *value, uint64_t *out, RCTPromiseRejectBlock reject) {
  if (loomParseU64(value, out)) {
    return YES;
  }
  NSString *reason = @"queue sequence must be an unsigned 64-bit decimal string";
  NSError *err = [NSError errorWithDomain:@"LoomError"
                                     code:22
                                 userInfo:@{NSLocalizedDescriptionKey : reason}];
  reject(@"22", reason, err);
  return NO;
}

@implementation UldrenLoom

RCT_EXPORT_MODULE()

- (NSString *)version {
  char *v = loom_version();
  NSString *out = v ? [NSString stringWithUTF8String:v] : @"";
  if (v) {
    loom_string_free(v);
  }
  return out;
}

- (NSString *)blobDigest:(NSArray *)bytes {
  NSUInteger len = bytes.count;
  unsigned char *buf = (unsigned char *)malloc(len ? len : 1);
  for (NSUInteger i = 0; i < len; i++) {
    buf[i] = (unsigned char)([bytes[i] integerValue] & 0xFF);
  }
  char *d = loom_blob_digest(buf, (size_t)len);
  free(buf);
  NSString *out = d ? [NSString stringWithUTF8String:d] : @"";
  if (d) {
    loom_string_free(d);
  }
  return out;
}

// Build an NSError from the engine's last error. Call only after a non-zero status, on the same
// thread that produced it (the C ABI's last-error slot is thread-local).

- (NSError *)loomError {
  int32_t code = 0;
  char *msg = NULL;
  uintptr_t len = 0;
  loom_last_error(&code, &msg, &len);
  NSString *reason = msg ? [NSString stringWithUTF8String:msg] : @"loom error";
  if (msg) {
    loom_string_free(msg);
  }
  return [NSError errorWithDomain:@"LoomError"
                             code:code
                         userInfo:@{NSLocalizedDescriptionKey : reason}];
}

// A dedicated background queue so SQL never runs on the JS thread: the engine has no worker pool of
// its own, so off-thread execution is the binding's job.

- (dispatch_queue_t)workQueue {
  static dispatch_queue_t q;
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    q = dispatch_queue_create("ai.uldren.loom.rn", DISPATCH_QUEUE_CONCURRENT);
  });
  return q;
}

// Open a session, choosing the opener from the supplied key: a non-empty `kek`
// (32 bytes) -> KEK unlock; else a non-empty `passphrase` -> passphrase unlock; else
// the plain open. Returns NULL on failure (the C ABI's thread-local last-error is set; caller rejects).

- (LoomSqlSession *)openSession:(NSString *)loomPath
                             ns:(NSString *)ns
                             db:(NSString *)db
                     passphrase:(NSString *)passphrase
                            kek:(NSArray *)kek {
  return [self openSession:loomPath ns:ns db:db passphrase:passphrase kek:kek authPrincipal:@"" authPassphrase:@""];
}

- (LoomSqlSession *)openSession:(NSString *)loomPath
                             ns:(NSString *)ns
                             db:(NSString *)db
                     passphrase:(NSString *)passphrase
                            kek:(NSArray *)kek
                  authPrincipal:(NSString *)authPrincipal
                 authPassphrase:(NSString *)authPassphrase {
  LoomSqlSession *s = NULL;
  int32_t st;
  NSData *ad = [authPassphrase dataUsingEncoding:NSUTF8StringEncoding];
  BOOL authed = authPrincipal.length > 0 && ad.length > 0;
  if (kek.count > 0) {
    NSUInteger klen = kek.count;
    unsigned char *kbuf = (unsigned char *)malloc(klen);
    for (NSUInteger i = 0; i < klen; i++) {
      kbuf[i] = (unsigned char)([kek[i] integerValue] & 0xFF);
    }
    if (authed) {
      st = loom_sql_open_with_kek_authenticated(
          loomPath.UTF8String, ns.UTF8String, db.UTF8String, kbuf, (uintptr_t)klen,
          authPrincipal.UTF8String, (const unsigned char *)ad.bytes, (uintptr_t)ad.length, &s);
    } else {
      st = loom_sql_open_with_kek(loomPath.UTF8String, ns.UTF8String, db.UTF8String, kbuf,
                                  (uintptr_t)klen, &s);
    }
    free(kbuf);
  } else if (passphrase.length) {
    NSData *pd = [passphrase dataUsingEncoding:NSUTF8StringEncoding];
    if (authed) {
      st = loom_sql_open_keyed_authenticated(
          loomPath.UTF8String, ns.UTF8String, db.UTF8String, (const unsigned char *)pd.bytes,
          (uintptr_t)pd.length, authPrincipal.UTF8String, (const unsigned char *)ad.bytes,
          (uintptr_t)ad.length, &s);
    } else {
      st = loom_sql_open_keyed(loomPath.UTF8String, ns.UTF8String, db.UTF8String,
                               (const unsigned char *)pd.bytes, (uintptr_t)pd.length, &s);
    }
  } else if (authed) {
    st = loom_sql_open_authenticated(loomPath.UTF8String, ns.UTF8String, db.UTF8String,
                                     authPrincipal.UTF8String, (const unsigned char *)ad.bytes,
                                     (uintptr_t)ad.length, &s);
  } else {
    st = loom_sql_open(loomPath.UTF8String, ns.UTF8String, db.UTF8String, &s);
  }
  return (st == 0) ? s : NULL;
}

- (LoomSession *)openStore:(NSString *)loomPath passphrase:(NSString *)passphrase kek:(NSArray *)kek {
  LoomSession *h = NULL;
  int32_t st;
  if (kek.count > 0) {
    NSUInteger klen = kek.count;
    unsigned char *kbuf = (unsigned char *)malloc(klen);
    for (NSUInteger i = 0; i < klen; i++) {
      kbuf[i] = (unsigned char)([kek[i] integerValue] & 0xFF);
    }
    st = loom_open_with_kek(loomPath.UTF8String, kbuf, (uintptr_t)klen, &h);
    free(kbuf);
  } else if (passphrase.length) {
    NSData *pd = [passphrase dataUsingEncoding:NSUTF8StringEncoding];
    st = loom_open_keyed(loomPath.UTF8String, (const unsigned char *)pd.bytes, (uintptr_t)pd.length,
                         &h);
  } else {
    st = loom_open(loomPath.UTF8String, &h);
  }
  return (st == 0) ? h : NULL;
}

- (int32_t)authenticateStore:(LoomSession *)h principal:(NSString *)principal passphrase:(NSString *)passphrase {
  if (principal.length == 0 || passphrase.length == 0) {
    return 0;
  }
  NSData *pd = [passphrase dataUsingEncoding:NSUTF8StringEncoding];
  return loom_authenticate_passphrase(h, principal.UTF8String,
                                      (const unsigned char *)pd.bytes, (uintptr_t)pd.length);
}

- (void)execCbor:(NSString *)loomPath
         request:(NSArray *)request
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
    if ([self authenticateStore:h principal:authPrincipal passphrase:authPassphrase] != 0) {
      NSError *err = [self loomError];
      loom_close(h);
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSUInteger reqLen = 0;
    unsigned char *req = loomBytesFromArray(request, &reqLen);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_exec_cbor(h, req, (uintptr_t)reqLen, &ptr, &len);
    free(req);
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    resolve(loomArrayFromOwnedBytes(ptr, len));
  });
}

- (LoomSqlBatch *)beginBatch:(NSString *)loomPath
                          ns:(NSString *)ns
                          db:(NSString *)db
                  passphrase:(NSString *)passphrase
                         kek:(NSArray *)kek {
  return [self beginBatch:loomPath ns:ns db:db passphrase:passphrase kek:kek authPrincipal:@"" authPassphrase:@""];
}

- (LoomSqlBatch *)beginBatch:(NSString *)loomPath
                          ns:(NSString *)ns
                          db:(NSString *)db
                  passphrase:(NSString *)passphrase
                         kek:(NSArray *)kek
               authPrincipal:(NSString *)authPrincipal
              authPassphrase:(NSString *)authPassphrase {
  LoomSqlBatch *b = NULL;
  int32_t st;
  NSData *ad = [authPassphrase dataUsingEncoding:NSUTF8StringEncoding];
  BOOL authed = authPrincipal.length > 0 && ad.length > 0;
  if (kek.count > 0) {
    NSUInteger klen = kek.count;
    unsigned char *kbuf = (unsigned char *)malloc(klen);
    for (NSUInteger i = 0; i < klen; i++) {
      kbuf[i] = (unsigned char)([kek[i] integerValue] & 0xFF);
    }
    if (authed) {
      st = loom_sql_batch_begin_with_kek_authenticated(
          loomPath.UTF8String, ns.UTF8String, db.UTF8String, kbuf, (uintptr_t)klen,
          authPrincipal.UTF8String, (const unsigned char *)ad.bytes, (uintptr_t)ad.length, &b);
    } else {
      st = loom_sql_batch_begin_with_kek(loomPath.UTF8String, ns.UTF8String, db.UTF8String, kbuf,
                                         (uintptr_t)klen, &b);
    }
    free(kbuf);
  } else if (passphrase.length) {
    NSData *pd = [passphrase dataUsingEncoding:NSUTF8StringEncoding];
    if (authed) {
      st = loom_sql_batch_begin_keyed_authenticated(
          loomPath.UTF8String, ns.UTF8String, db.UTF8String, (const unsigned char *)pd.bytes,
          (uintptr_t)pd.length, authPrincipal.UTF8String, (const unsigned char *)ad.bytes,
          (uintptr_t)ad.length, &b);
    } else {
      st = loom_sql_batch_begin_keyed(loomPath.UTF8String, ns.UTF8String, db.UTF8String,
                                      (const unsigned char *)pd.bytes, (uintptr_t)pd.length, &b);
    }
  } else if (authed) {
    st = loom_sql_batch_begin_authenticated(loomPath.UTF8String, ns.UTF8String, db.UTF8String,
                                            authPrincipal.UTF8String,
                                            (const unsigned char *)ad.bytes,
                                            (uintptr_t)ad.length, &b);
  } else {
    st = loom_sql_batch_begin(loomPath.UTF8String, ns.UTF8String, db.UTF8String, &b);
  }
  return (st == 0) ? b : NULL;
}

// Create a fresh `.loom` under an identity profile, optionally encrypted under a passphrase.
// An empty `suite`/`passphrase` means profile-default / unencrypted. Resolves nil.

- (std::shared_ptr<facebook::react::TurboModule>)getTurboModule:
    (const facebook::react::ObjCTurboModule::InitParams &)params {
  return std::make_shared<facebook::react::NativeUldrenLoomSpecJSI>(params);
}

@end
