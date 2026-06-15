// JNI bridge for the React Native Android module, calling the Uldren Loom C ABI.
// Licensed under BUSL-1.1. (c) Uldren Technologies LLC.
#include <jni.h>

#include <cstddef>

extern "C" {
#include "loom.h"
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomModule_nativeVersion(JNIEnv *env, jobject thiz) {
  (void)thiz;
  char *v = loom_version();
  jstring out = env->NewStringUTF(v ? v : "");
  if (v) {
    loom_string_free(v);
  }
  return out;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomModule_nativeBlobDigest(JNIEnv *env, jobject thiz,
                                                         jbyteArray data) {
  (void)thiz;
  jsize len = env->GetArrayLength(data);
  jbyte *buf = env->GetByteArrayElements(data, nullptr);
  char *d = loom_blob_digest(reinterpret_cast<const unsigned char *>(buf), static_cast<size_t>(len));
  env->ReleaseByteArrayElements(data, buf, JNI_ABORT);
  jstring out = env->NewStringUTF(d ? d : "");
  if (d) {
    loom_string_free(d);
  }
  return out;
}
