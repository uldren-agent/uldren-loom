/* JNI shim bridging `ai.uldren.loom.Loom` to the Uldren Loom C ABI (include/loom.h).
 * The same shim serves the Android and the desktop-JVM targets (identical class name).
 * Licensed under BUSL-1.1. (c) Uldren Technologies LLC. */
#include <jni.h>
#include <stddef.h>

#include "loom.h"

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_Loom_version(JNIEnv *env, jobject thiz) {
    (void)thiz;
    char *v = loom_version();
    jstring out = (*env)->NewStringUTF(env, v ? v : "");
    if (v) loom_string_free(v);
    return out;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_Loom_blobDigest(JNIEnv *env, jobject thiz, jbyteArray data) {
    (void)thiz;
    jsize len = (*env)->GetArrayLength(env, data);
    jbyte *buf = (*env)->GetByteArrayElements(env, data, NULL);
    char *d = loom_blob_digest((const unsigned char *)buf, (size_t)len);
    (*env)->ReleaseByteArrayElements(env, data, buf, JNI_ABORT);
    jstring out = (*env)->NewStringUTF(env, d ? d : "");
    if (d) loom_string_free(d);
    return out;
}
