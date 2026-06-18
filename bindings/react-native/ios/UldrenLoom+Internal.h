// Internal declarations shared across the category translation units. Not public API.
// Licensed under BUSL-1.1. (c) Uldren Technologies LLC.
#import "UldrenLoom.h"
#import "loom.h"

// File-scope C helpers shared across the category translation units.
unsigned char *loomBytesFromArray(NSArray *arr, NSUInteger *outLen);
NSArray *loomArrayFromOwnedBytes(unsigned char *ptr, uintptr_t len);
NSString *loomStringFromU64(uint64_t value);
BOOL loomParseU64(NSString *value, uint64_t *out);
BOOL loomResolveU64(NSString *value, uint64_t *out, RCTPromiseRejectBlock reject);

@interface UldrenLoom (Internal)
- (NSError *)loomError;
- (dispatch_queue_t)workQueue;
- (LoomSqlSession *)openSession:(NSString *)loomPath ns:(NSString *)ns db:(NSString *)db passphrase:(NSString *)passphrase kek:(NSArray *)kek;
- (LoomSqlSession *)openSession:(NSString *)loomPath ns:(NSString *)ns db:(NSString *)db passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase;
- (LoomSession *)openStore:(NSString *)loomPath passphrase:(NSString *)passphrase kek:(NSArray *)kek;
- (int32_t)authenticateStore:(LoomSession *)h principal:(NSString *)principal passphrase:(NSString *)passphrase;
- (LoomSqlBatch *)beginBatch:(NSString *)loomPath ns:(NSString *)ns db:(NSString *)db passphrase:(NSString *)passphrase kek:(NSArray *)kek;
- (LoomSqlBatch *)beginBatch:(NSString *)loomPath ns:(NSString *)ns db:(NSString *)db passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase;
@end
