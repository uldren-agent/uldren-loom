#import "UldrenLoom.h"

#import "loom.h"

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

// Bridges to the codegen'd C++ TurboModule.
- (std::shared_ptr<facebook::react::TurboModule>)getTurboModule:
    (const facebook::react::ObjCTurboModule::InitParams &)params {
  return std::make_shared<facebook::react::NativeUldrenLoomSpecJSI>(params);
}

@end
