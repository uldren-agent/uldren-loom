// Shared JNI helpers and the C ABI include for the React Native Android module.
// Licensed under BUSL-1.1. (c) Uldren Technologies LLC.
#ifndef ULDREN_LOOM_RN_JNI_H
#define ULDREN_LOOM_RN_JNI_H

#include <jni.h>
#include <cerrno>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <string>
#include <vector>

extern "C" {
#include "loom.h"
}

void throwLoom(JNIEnv *env);
void throwIllegalArgument(JNIEnv *env, const char *message);
bool parseU64String(JNIEnv *env, jstring value, uint64_t *out);
jstring u64String(JNIEnv *env, uint64_t value);
int32_t openSessionKeyed(JNIEnv *env, const char *p, const char *n, const char *d, jbyteArray passphrase, jbyteArray kek, LoomSqlSession **out);
int32_t openAuthenticatedSessionKeyed(JNIEnv *env, const char *p, const char *n, const char *d,
                                      jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
                                      jbyteArray authPassphrase, LoomSqlSession **out);
int32_t openStoreKeyed(JNIEnv *env, const char *p, jbyteArray passphrase, jbyteArray kek, LoomSession **out);
int32_t authenticateStore(JNIEnv *env, LoomSession *h, jstring principal, jbyteArray passphrase);
int32_t openAuthenticatedStoreKeyed(JNIEnv *env, const char *p, jbyteArray passphrase, jbyteArray kek,
                                    jstring authPrincipal, jbyteArray authPassphrase, LoomSession **out);
jbyteArray ownedBytes(JNIEnv *env, unsigned char *ptr, uintptr_t len);
jstring ownedJsonString(JNIEnv *env, unsigned char *ptr, uintptr_t len);
int32_t beginBatchKeyed(JNIEnv *env, const char *p, const char *n, const char *d, jbyteArray passphrase, jbyteArray kek, LoomSqlBatch **out);
int32_t beginAuthenticatedBatchKeyed(JNIEnv *env, const char *p, const char *n, const char *d,
                                     jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
                                     jbyteArray authPassphrase, LoomSqlBatch **out);

#endif  // ULDREN_LOOM_RN_JNI_H
