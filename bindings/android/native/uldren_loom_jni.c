/* JNI shim bridging `ai.uldren.loom.Loom` to the Uldren Loom C ABI (include/loom.h).
 * The same shim serves the Android and the desktop-JVM targets (identical class name).
 * Licensed under BUSL-1.1. (c) Uldren Technologies LLC. */
#include <jni.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "loom.h"

/* Throw a Java RuntimeException carrying the engine's last error. Call only after a non-zero status. */
static void throw_loom(JNIEnv *env) {
    int32_t code = 0;
    char *msg = NULL;
    uintptr_t len = 0;
    loom_last_error(&code, &msg, &len);
    jclass ex = (*env)->FindClass(env, "java/lang/RuntimeException");
    (*env)->ThrowNew(env, ex, msg ? msg : "loom error");
    if (msg) loom_string_free(msg);
}

static jstring owned_json_string(JNIEnv *env, unsigned char *ptr, uintptr_t len) {
    char *buf = (char *)malloc((size_t)len + 1);
    if (!buf) {
        if (ptr) loom_bytes_free(ptr, len);
        jclass ex = (*env)->FindClass(env, "java/lang/OutOfMemoryError");
        (*env)->ThrowNew(env, ex, "json result allocation failed");
        return NULL;
    }
    if (len > 0 && ptr) {
        memcpy(buf, ptr, (size_t)len);
    }
    buf[len] = '\0';
    if (ptr) loom_bytes_free(ptr, len);
    jstring out = (*env)->NewStringUTF(env, buf);
    free(buf);
    return out;
}

typedef struct {
    jsize count;
    jint *kinds;
    jbyteArray *arrays;
    jbyte **bytes;
    const unsigned char **prefixes;
    uintptr_t *lengths;
} JniAclScopes;

static int acl_scopes_acquire(JNIEnv *env, jintArray scope_kinds, jobjectArray scope_prefixes,
                              JniAclScopes *out) {
    out->count = scope_kinds ? (*env)->GetArrayLength(env, scope_kinds) : 0;
    jsize prefix_count = scope_prefixes ? (*env)->GetArrayLength(env, scope_prefixes) : 0;
    if (out->count != prefix_count) {
        jclass ex = (*env)->FindClass(env, "java/lang/IllegalArgumentException");
        (*env)->ThrowNew(env, ex, "scope kind and prefix counts differ");
        return -1;
    }
    out->kinds = NULL;
    out->arrays = NULL;
    out->bytes = NULL;
    out->prefixes = NULL;
    out->lengths = NULL;
    if (out->count == 0) {
        return 0;
    }
    out->kinds = (*env)->GetIntArrayElements(env, scope_kinds, NULL);
    out->arrays = calloc((size_t)out->count, sizeof(jbyteArray));
    out->bytes = calloc((size_t)out->count, sizeof(jbyte *));
    out->prefixes = calloc((size_t)out->count, sizeof(unsigned char *));
    out->lengths = calloc((size_t)out->count, sizeof(uintptr_t));
    if (!out->kinds || !out->arrays || !out->bytes || !out->prefixes || !out->lengths) {
        jclass ex = (*env)->FindClass(env, "java/lang/OutOfMemoryError");
        (*env)->ThrowNew(env, ex, "acl scope allocation failed");
        return -1;
    }
    for (jsize i = 0; i < out->count; i++) {
        out->arrays[i] = (jbyteArray)(*env)->GetObjectArrayElement(env, scope_prefixes, i);
        if (!out->arrays[i]) {
            jclass ex = (*env)->FindClass(env, "java/lang/IllegalArgumentException");
            (*env)->ThrowNew(env, ex, "scope prefix is null");
            return -1;
        }
        out->lengths[i] = (uintptr_t)(*env)->GetArrayLength(env, out->arrays[i]);
        out->bytes[i] = (*env)->GetByteArrayElements(env, out->arrays[i], NULL);
        out->prefixes[i] = (const unsigned char *)out->bytes[i];
        if (!out->bytes[i]) {
            return -1;
        }
    }
    return 0;
}

static void acl_scopes_release(JNIEnv *env, jintArray scope_kinds, JniAclScopes *scopes) {
    if (scopes->arrays) {
        for (jsize i = 0; i < scopes->count; i++) {
            if (scopes->arrays[i]) {
                if (scopes->bytes && scopes->bytes[i]) {
                    (*env)->ReleaseByteArrayElements(env, scopes->arrays[i], scopes->bytes[i], JNI_ABORT);
                }
                (*env)->DeleteLocalRef(env, scopes->arrays[i]);
            }
        }
    }
    if (scopes->kinds) {
        (*env)->ReleaseIntArrayElements(env, scope_kinds, scopes->kinds, JNI_ABORT);
    }
    free(scopes->arrays);
    free(scopes->bytes);
    free(scopes->prefixes);
    free(scopes->lengths);
}

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

/* Create a fresh `.loom` under an identity profile, optionally encrypted under a passphrase.
 * `suite`/`passphrase` may be null (null/empty passphrase -> unencrypted). */
JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeCreate(JNIEnv *env, jobject thiz, jstring path, jstring profile,
                                      jstring suite, jbyteArray passphrase) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *prof = (*env)->GetStringUTFChars(env, profile, NULL);
    const char *su = suite ? (*env)->GetStringUTFChars(env, suite, NULL) : NULL;
    jbyte *pass = NULL;
    jsize plen = 0;
    if (passphrase != NULL) {
        plen = (*env)->GetArrayLength(env, passphrase);
        pass = (*env)->GetByteArrayElements(env, passphrase, NULL);
    }
    int32_t st = loom_create(p, prof, su, (const unsigned char *)pass, (uintptr_t)plen);
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, profile, prof);
    if (su) (*env)->ReleaseStringUTFChars(env, suite, su);
    if (pass) (*env)->ReleaseByteArrayElements(env, passphrase, pass, JNI_ABORT);
    if (st != 0) {
        throw_loom(env);
    }
}

/* Like nativeCreate, but wraps the DEK under a host-supplied 256-bit KEK. `kek`
 * must be 32 bytes; null/empty creates an unencrypted store. */
JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeCreateWithKek(JNIEnv *env, jobject thiz, jstring path, jstring profile,
                                             jstring suite, jbyteArray kek) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *prof = (*env)->GetStringUTFChars(env, profile, NULL);
    const char *su = suite ? (*env)->GetStringUTFChars(env, suite, NULL) : NULL;
    jbyte *k = NULL;
    jsize klen = 0;
    if (kek != NULL) {
        klen = (*env)->GetArrayLength(env, kek);
        k = (*env)->GetByteArrayElements(env, kek, NULL);
    }
    int32_t st = loom_create_with_kek(p, prof, su, (const unsigned char *)k, (uintptr_t)klen);
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, profile, prof);
    if (su) (*env)->ReleaseStringUTFChars(env, suite, su);
    if (k) (*env)->ReleaseByteArrayElements(env, kek, k, JNI_ABORT);
    if (st != 0) {
        throw_loom(env);
    }
}

static LoomSession *open_store_handle(JNIEnv *env, jstring path, jbyteArray passphrase, jbyteArray kek) {
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    jbyte *pass = NULL;
    jsize plen = 0;
    if (passphrase != NULL) {
        plen = (*env)->GetArrayLength(env, passphrase);
        pass = (*env)->GetByteArrayElements(env, passphrase, NULL);
    }
    jbyte *k = NULL;
    jsize klen = 0;
    if (kek != NULL) {
        klen = (*env)->GetArrayLength(env, kek);
        k = (*env)->GetByteArrayElements(env, kek, NULL);
    }
    LoomSession *h = NULL;
    int32_t st;
    if (klen > 0) {
        st = loom_open_with_kek(p, (const unsigned char *)k, (uintptr_t)klen, &h);
    } else if (plen > 0) {
        st = loom_open_keyed(p, (const unsigned char *)pass, (uintptr_t)plen, &h);
    } else {
        st = loom_open(p, &h);
    }
    (*env)->ReleaseStringUTFChars(env, path, p);
    if (pass) (*env)->ReleaseByteArrayElements(env, passphrase, pass, JNI_ABORT);
    if (k) (*env)->ReleaseByteArrayElements(env, kek, k, JNI_ABORT);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return h;
}

static int32_t authenticate_handle(JNIEnv *env, LoomSession *h, jstring principal, jbyteArray passphrase);
static LoomSession *open_authenticated_store_handle(JNIEnv *env, jstring path, jbyteArray passphrase,
                                                   jbyteArray kek, jstring auth_principal,
                                                   jbyteArray auth_passphrase);

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeWorkspaceCreate(JNIEnv *env, jobject thiz, jstring path,
                                               jstring name, jstring facet, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal,
                                               jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = name ? (*env)->GetStringUTFChars(env, name, NULL) : NULL;
    const char *f = facet ? (*env)->GetStringUTFChars(env, facet, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_workspace_create(h, n, f, &out);
    if (n) (*env)->ReleaseStringUTFChars(env, name, n);
    if (f) (*env)->ReleaseStringUTFChars(env, facet, f);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeWorkspaceListJson(JNIEnv *env, jobject thiz, jstring path,
                                                 jbyteArray passphrase, jbyteArray kek,
                                                 jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    char *out = NULL;
    int32_t st = loom_workspace_list_json(h, &out);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "[]");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeWorkspaceRename(JNIEnv *env, jobject thiz, jstring path,
                                               jstring ns, jstring newName, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal,
                                               jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *nn = (*env)->GetStringUTFChars(env, newName, NULL);
    int32_t st = loom_workspace_rename(h, n, nn);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, newName, nn);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeWorkspaceDelete(JNIEnv *env, jobject thiz, jstring path,
                                               jstring ns, jbyteArray passphrase, jbyteArray kek,
                                               jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    int32_t st = loom_workspace_delete(h, n);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeAuthenticatePassphrase(JNIEnv *env, jobject thiz, jstring path,
                                                      jstring principal, jbyteArray principal_passphrase,
                                                      jbyteArray passphrase, jbyteArray kek) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    jbyte *pp = (*env)->GetByteArrayElements(env, principal_passphrase, NULL);
    jsize pplen = (*env)->GetArrayLength(env, principal_passphrase);
    int32_t st = loom_authenticate_passphrase(h, p, (const unsigned char *)pp, (uintptr_t)pplen);
    (*env)->ReleaseStringUTFChars(env, principal, p);
    (*env)->ReleaseByteArrayElements(env, principal_passphrase, pp, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeExecCbor(JNIEnv *env, jobject thiz, jstring path,
                                        jbyteArray request, jbyteArray passphrase, jbyteArray kek,
                                        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h =
        open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    jbyte *req = (*env)->GetByteArrayElements(env, request, NULL);
    jsize req_len = (*env)->GetArrayLength(env, request);
    unsigned char *out = NULL;
    uintptr_t out_len = 0;
    int32_t st = loom_exec_cbor(h, (const unsigned char *)req, (uintptr_t)req_len, &out, &out_len);
    (*env)->ReleaseByteArrayElements(env, request, req, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jbyteArray r = (*env)->NewByteArray(env, (jsize)out_len);
    if (r && out_len > 0) {
        (*env)->SetByteArrayRegion(env, r, 0, (jsize)out_len, (const jbyte *)out);
    }
    loom_bytes_free(out, out_len);
    return r;
}

static int32_t authenticate_handle(JNIEnv *env, LoomSession *h, jstring principal, jbyteArray passphrase) {
    if (principal == NULL || passphrase == NULL) {
        return 0;
    }
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    jbyte *pp = (*env)->GetByteArrayElements(env, passphrase, NULL);
    jsize pplen = (*env)->GetArrayLength(env, passphrase);
    int32_t st = loom_authenticate_passphrase(h, p, (const unsigned char *)pp, (uintptr_t)pplen);
    (*env)->ReleaseStringUTFChars(env, principal, p);
    (*env)->ReleaseByteArrayElements(env, passphrase, pp, JNI_ABORT);
    return st;
}

static LoomSession *open_authenticated_store_handle(JNIEnv *env, jstring path, jbyteArray passphrase,
                                                   jbyteArray kek, jstring auth_principal,
                                                   jbyteArray auth_passphrase) {
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) {
        return NULL;
    }
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st != 0) {
        loom_close(h);
        throw_loom(env);
        return NULL;
    }
    return h;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityListJson(JNIEnv *env, jobject thiz, jstring path,
                                                jbyteArray passphrase, jbyteArray kek,
                                                jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return NULL;
    char *out = NULL;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_list_json(h, &out);
    }
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "{}");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityAddPrincipal(JNIEnv *env, jobject thiz, jstring path,
                                                    jstring principal_handle, jstring name, jstring kind, jbyteArray passphrase,
                                                    jbyteArray kek, jstring auth_principal,
                                                    jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return NULL;
    const char *hnd = (*env)->GetStringUTFChars(env, principal_handle, NULL);
    const char *n = (*env)->GetStringUTFChars(env, name, NULL);
    const char *k = (*env)->GetStringUTFChars(env, kind, NULL);
    char *out = NULL;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_add_principal(h, hnd, n, k, &out);
    }
    (*env)->ReleaseStringUTFChars(env, principal_handle, hnd);
    (*env)->ReleaseStringUTFChars(env, name, n);
    (*env)->ReleaseStringUTFChars(env, kind, k);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityRenamePrincipalHandle(JNIEnv *env, jobject thiz,
                                                                  jstring path, jstring principal,
                                                                  jstring principal_handle,
                                                                  jbyteArray passphrase, jbyteArray kek,
                                                                  jstring auth_principal,
                                                                  jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *hnd = (*env)->GetStringUTFChars(env, principal_handle, NULL);
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_rename_principal_handle(h, p, hnd);
    }
    (*env)->ReleaseStringUTFChars(env, principal, p);
    (*env)->ReleaseStringUTFChars(env, principal_handle, hnd);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentitySetPassphrase(JNIEnv *env, jobject thiz, jstring path,
                                                     jstring principal, jbyteArray principal_passphrase,
                                                     jbyteArray passphrase, jbyteArray kek,
                                                     jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    jbyte *pp = (*env)->GetByteArrayElements(env, principal_passphrase, NULL);
    jsize pplen = (*env)->GetArrayLength(env, principal_passphrase);
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_set_passphrase(h, p, (const unsigned char *)pp, (uintptr_t)pplen);
    }
    (*env)->ReleaseStringUTFChars(env, principal, p);
    (*env)->ReleaseByteArrayElements(env, principal_passphrase, pp, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityRemovePrincipal(JNIEnv *env, jobject thiz, jstring path,
                                                       jstring principal, jbyteArray passphrase,
                                                       jbyteArray kek, jstring auth_principal,
                                                       jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_remove_principal(h, p);
    }
    (*env)->ReleaseStringUTFChars(env, principal, p);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityAssignRole(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring principal, jstring role,
                                                   jbyteArray passphrase, jbyteArray kek,
                                                   jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *r = (*env)->GetStringUTFChars(env, role, NULL);
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_assign_role(h, p, r);
    }
    (*env)->ReleaseStringUTFChars(env, principal, p);
    (*env)->ReleaseStringUTFChars(env, role, r);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityRevokeRole(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring principal, jstring role,
                                                   jbyteArray passphrase, jbyteArray kek,
                                                   jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return JNI_FALSE;
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *r = (*env)->GetStringUTFChars(env, role, NULL);
    int32_t removed = 0;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_revoke_role(h, p, r, &removed);
    }
    (*env)->ReleaseStringUTFChars(env, principal, p);
    (*env)->ReleaseStringUTFChars(env, role, r);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return removed ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityCreateExternalCredential(JNIEnv *env, jobject thiz,
                                                                 jstring path, jstring principal,
                                                                 jstring kind, jstring label,
                                                                 jstring issuer, jstring subject,
                                                                 jstring material_digest,
                                                                 jbyteArray passphrase,
                                                                 jbyteArray kek,
                                                                 jstring auth_principal,
                                                                 jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return NULL;
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *k = (*env)->GetStringUTFChars(env, kind, NULL);
    const char *l = (*env)->GetStringUTFChars(env, label, NULL);
    const char *i = (*env)->GetStringUTFChars(env, issuer, NULL);
    const char *s = (*env)->GetStringUTFChars(env, subject, NULL);
    const char *m = material_digest ? (*env)->GetStringUTFChars(env, material_digest, NULL) : NULL;
    char *out = NULL;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_create_external_credential(h, p, k, l, i, s, m, &out);
    }
    (*env)->ReleaseStringUTFChars(env, principal, p);
    (*env)->ReleaseStringUTFChars(env, kind, k);
    (*env)->ReleaseStringUTFChars(env, label, l);
    (*env)->ReleaseStringUTFChars(env, issuer, i);
    (*env)->ReleaseStringUTFChars(env, subject, s);
    if (m) (*env)->ReleaseStringUTFChars(env, material_digest, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityRevokeExternalCredential(JNIEnv *env, jobject thiz,
                                                                 jstring path, jstring credential,
                                                                 jbyteArray passphrase,
                                                                 jbyteArray kek,
                                                                 jstring auth_principal,
                                                                 jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *c = (*env)->GetStringUTFChars(env, credential, NULL);
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_revoke_external_credential(h, c);
    }
    (*env)->ReleaseStringUTFChars(env, credential, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityAddPublicKey(JNIEnv *env, jobject thiz,
                                                     jstring path, jstring principal,
                                                     jstring label, jstring algorithm,
                                                     jstring public_key_hex,
                                                     jbyteArray passphrase,
                                                     jbyteArray kek,
                                                     jstring auth_principal,
                                                     jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return NULL;
    const char *p = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *l = (*env)->GetStringUTFChars(env, label, NULL);
    const char *a = (*env)->GetStringUTFChars(env, algorithm, NULL);
    const char *k = (*env)->GetStringUTFChars(env, public_key_hex, NULL);
    char *out = NULL;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_add_public_key(h, p, l, a, k, &out);
    }
    (*env)->ReleaseStringUTFChars(env, principal, p);
    (*env)->ReleaseStringUTFChars(env, label, l);
    (*env)->ReleaseStringUTFChars(env, algorithm, a);
    (*env)->ReleaseStringUTFChars(env, public_key_hex, k);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeIdentityRevokePublicKey(JNIEnv *env, jobject thiz,
                                                        jstring path, jstring key,
                                                        jbyteArray passphrase,
                                                        jbyteArray kek,
                                                        jstring auth_principal,
                                                        jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *k = (*env)->GetStringUTFChars(env, key, NULL);
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_identity_revoke_public_key(h, k);
    }
    (*env)->ReleaseStringUTFChars(env, key, k);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeAclListJson(JNIEnv *env, jobject thiz, jstring path,
                                           jbyteArray passphrase, jbyteArray kek,
                                           jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return NULL;
    char *out = NULL;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_acl_list_json(h, &out);
    }
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "[]");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeAclGrant(JNIEnv *env, jobject thiz, jstring path, jint effect,
                                        jstring subject, jstring workspace_, jstring facet,
                                        jint rights_mask, jbyteArray passphrase, jbyteArray kek,
                                        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *s = (*env)->GetStringUTFChars(env, subject, NULL);
    const char *n = workspace_ ? (*env)->GetStringUTFChars(env, workspace_, NULL) : NULL;
    const char *f = facet ? (*env)->GetStringUTFChars(env, facet, NULL) : NULL;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_acl_grant(h, (int32_t)effect, s, n, f, (uint32_t)rights_mask);
    }
    (*env)->ReleaseStringUTFChars(env, subject, s);
    if (n) (*env)->ReleaseStringUTFChars(env, workspace_, n);
    if (f) (*env)->ReleaseStringUTFChars(env, facet, f);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeAclRevoke(JNIEnv *env, jobject thiz, jstring path, jint effect,
                                         jstring subject, jstring workspace_, jstring facet,
                                         jint rights_mask, jbyteArray passphrase, jbyteArray kek,
                                         jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return JNI_FALSE;
    const char *s = (*env)->GetStringUTFChars(env, subject, NULL);
    const char *n = workspace_ ? (*env)->GetStringUTFChars(env, workspace_, NULL) : NULL;
    const char *f = facet ? (*env)->GetStringUTFChars(env, facet, NULL) : NULL;
    int32_t removed = 0;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0) {
        st = loom_acl_revoke(h, (int32_t)effect, s, n, f, (uint32_t)rights_mask, &removed);
    }
    (*env)->ReleaseStringUTFChars(env, subject, s);
    if (n) (*env)->ReleaseStringUTFChars(env, workspace_, n);
    if (f) (*env)->ReleaseStringUTFChars(env, facet, f);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return removed ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeAclGrantScoped(JNIEnv *env, jobject thiz, jstring path, jint effect,
                                              jstring subject, jstring workspace_, jstring facet,
                                              jint rights_mask, jstring ref_glob, jintArray scope_kinds,
                                              jobjectArray scope_prefixes, jbyteArray passphrase,
                                              jbyteArray kek, jstring auth_principal,
                                              jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *s = (*env)->GetStringUTFChars(env, subject, NULL);
    const char *n = workspace_ ? (*env)->GetStringUTFChars(env, workspace_, NULL) : NULL;
    const char *f = facet ? (*env)->GetStringUTFChars(env, facet, NULL) : NULL;
    const char *r = ref_glob ? (*env)->GetStringUTFChars(env, ref_glob, NULL) : NULL;
    JniAclScopes scopes = {0};
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0 && acl_scopes_acquire(env, scope_kinds, scope_prefixes, &scopes) == 0) {
        st = loom_acl_grant_scoped(h, (int32_t)effect, s, n, f, (uint32_t)rights_mask, r,
                                   (uintptr_t)scopes.count, (const int32_t *)scopes.kinds,
                                   scopes.prefixes, scopes.lengths);
    }
    acl_scopes_release(env, scope_kinds, &scopes);
    (*env)->ReleaseStringUTFChars(env, subject, s);
    if (n) (*env)->ReleaseStringUTFChars(env, workspace_, n);
    if (f) (*env)->ReleaseStringUTFChars(env, facet, f);
    if (r) (*env)->ReleaseStringUTFChars(env, ref_glob, r);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeAclGrantScopedPredicate(JNIEnv *env, jobject thiz, jstring path, jint effect,
                                                       jstring subject, jstring workspace_, jstring facet,
                                                       jint rights_mask, jstring ref_glob, jintArray scope_kinds,
                                                       jobjectArray scope_prefixes, jstring predicate_cel,
                                                       jbyteArray passphrase, jbyteArray kek,
                                                       jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return;
    const char *s = (*env)->GetStringUTFChars(env, subject, NULL);
    const char *n = workspace_ ? (*env)->GetStringUTFChars(env, workspace_, NULL) : NULL;
    const char *f = facet ? (*env)->GetStringUTFChars(env, facet, NULL) : NULL;
    const char *r = ref_glob ? (*env)->GetStringUTFChars(env, ref_glob, NULL) : NULL;
    const char *p = predicate_cel ? (*env)->GetStringUTFChars(env, predicate_cel, NULL) : NULL;
    JniAclScopes scopes = {0};
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0 && acl_scopes_acquire(env, scope_kinds, scope_prefixes, &scopes) == 0) {
        st = loom_acl_grant_scoped_predicate(h, (int32_t)effect, s, n, f, (uint32_t)rights_mask, r,
                                             (uintptr_t)scopes.count, (const int32_t *)scopes.kinds,
                                             scopes.prefixes, scopes.lengths, p ? "cel" : NULL, p);
    }
    acl_scopes_release(env, scope_kinds, &scopes);
    (*env)->ReleaseStringUTFChars(env, subject, s);
    if (n) (*env)->ReleaseStringUTFChars(env, workspace_, n);
    if (f) (*env)->ReleaseStringUTFChars(env, facet, f);
    if (r) (*env)->ReleaseStringUTFChars(env, ref_glob, r);
    if (p) (*env)->ReleaseStringUTFChars(env, predicate_cel, p);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeAclRevokeScoped(JNIEnv *env, jobject thiz, jstring path, jint effect,
                                               jstring subject, jstring workspace_, jstring facet,
                                               jint rights_mask, jstring ref_glob, jintArray scope_kinds,
                                               jobjectArray scope_prefixes, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal,
                                               jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return JNI_FALSE;
    const char *s = (*env)->GetStringUTFChars(env, subject, NULL);
    const char *n = workspace_ ? (*env)->GetStringUTFChars(env, workspace_, NULL) : NULL;
    const char *f = facet ? (*env)->GetStringUTFChars(env, facet, NULL) : NULL;
    const char *r = ref_glob ? (*env)->GetStringUTFChars(env, ref_glob, NULL) : NULL;
    JniAclScopes scopes = {0};
    int32_t removed = 0;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0 && acl_scopes_acquire(env, scope_kinds, scope_prefixes, &scopes) == 0) {
        st = loom_acl_revoke_scoped(h, (int32_t)effect, s, n, f, (uint32_t)rights_mask, r,
                                    (uintptr_t)scopes.count, (const int32_t *)scopes.kinds,
                                    scopes.prefixes, scopes.lengths, &removed);
    }
    acl_scopes_release(env, scope_kinds, &scopes);
    (*env)->ReleaseStringUTFChars(env, subject, s);
    if (n) (*env)->ReleaseStringUTFChars(env, workspace_, n);
    if (f) (*env)->ReleaseStringUTFChars(env, facet, f);
    if (r) (*env)->ReleaseStringUTFChars(env, ref_glob, r);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return removed ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeAclRevokeScopedPredicate(JNIEnv *env, jobject thiz, jstring path, jint effect,
                                                        jstring subject, jstring workspace_, jstring facet,
                                                        jint rights_mask, jstring ref_glob, jintArray scope_kinds,
                                                        jobjectArray scope_prefixes, jstring predicate_cel,
                                                        jbyteArray passphrase, jbyteArray kek,
                                                        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_store_handle(env, path, passphrase, kek);
    if (!h) return JNI_FALSE;
    const char *s = (*env)->GetStringUTFChars(env, subject, NULL);
    const char *n = workspace_ ? (*env)->GetStringUTFChars(env, workspace_, NULL) : NULL;
    const char *f = facet ? (*env)->GetStringUTFChars(env, facet, NULL) : NULL;
    const char *r = ref_glob ? (*env)->GetStringUTFChars(env, ref_glob, NULL) : NULL;
    const char *p = predicate_cel ? (*env)->GetStringUTFChars(env, predicate_cel, NULL) : NULL;
    JniAclScopes scopes = {0};
    int32_t removed = 0;
    int32_t st = authenticate_handle(env, h, auth_principal, auth_passphrase);
    if (st == 0 && acl_scopes_acquire(env, scope_kinds, scope_prefixes, &scopes) == 0) {
        st = loom_acl_revoke_scoped_predicate(h, (int32_t)effect, s, n, f, (uint32_t)rights_mask, r,
                                              (uintptr_t)scopes.count, (const int32_t *)scopes.kinds,
                                              scopes.prefixes, scopes.lengths, p ? "cel" : NULL, p, &removed);
    }
    acl_scopes_release(env, scope_kinds, &scopes);
    (*env)->ReleaseStringUTFChars(env, subject, s);
    if (n) (*env)->ReleaseStringUTFChars(env, workspace_, n);
    if (f) (*env)->ReleaseStringUTFChars(env, facet, f);
    if (r) (*env)->ReleaseStringUTFChars(env, ref_glob, r);
    if (p) (*env)->ReleaseStringUTFChars(env, predicate_cel, p);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return removed ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeProtectedRefListJson(JNIEnv *env, jobject thiz, jstring path,
                                                    jstring workspace_, jbyteArray passphrase,
                                                    jbyteArray kek, jstring auth_principal,
                                                    jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, workspace_, NULL);
    char *out = NULL;
    int32_t st = loom_protected_ref_list_json(h, n, &out);
    (*env)->ReleaseStringUTFChars(env, workspace_, n);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "[]");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeProtectedRefGetJson(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring workspace_, jstring ref_name,
                                                   jbyteArray passphrase, jbyteArray kek,
                                                   jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, workspace_, NULL);
    const char *rname = (*env)->GetStringUTFChars(env, ref_name, NULL);
    char *out = NULL;
    int32_t st = loom_protected_ref_get_json(h, n, rname, &out);
    (*env)->ReleaseStringUTFChars(env, workspace_, n);
    (*env)->ReleaseStringUTFChars(env, ref_name, rname);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "null");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeProtectedRefSet(JNIEnv *env, jobject thiz, jstring path,
                                               jstring workspace_, jstring ref_name,
                                               jboolean fast_forward_only,
                                               jboolean signed_commits_required,
                                               jboolean signed_ref_advance_required,
                                               jint required_review_count,
                                               jboolean retention_lock,
                                               jboolean governance_lock,
                                               jbyteArray passphrase, jbyteArray kek,
                                               jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, workspace_, NULL);
    const char *r = (*env)->GetStringUTFChars(env, ref_name, NULL);
    int32_t st = loom_protected_ref_set(h, n, r,
                                        fast_forward_only == JNI_TRUE,
                                        signed_commits_required == JNI_TRUE,
                                        signed_ref_advance_required == JNI_TRUE,
                                        (uint32_t)required_review_count,
                                        retention_lock == JNI_TRUE,
                                        governance_lock == JNI_TRUE);
    (*env)->ReleaseStringUTFChars(env, workspace_, n);
    (*env)->ReleaseStringUTFChars(env, ref_name, r);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeProtectedRefRemove(JNIEnv *env, jobject thiz, jstring path,
                                                  jstring workspace_, jstring ref_name,
                                                  jbyteArray passphrase, jbyteArray kek,
                                                  jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, workspace_, NULL);
    const char *r = (*env)->GetStringUTFChars(env, ref_name, NULL);
    int32_t removed = 0;
    int32_t st = loom_protected_ref_remove(h, n, r, &removed);
    (*env)->ReleaseStringUTFChars(env, workspace_, n);
    (*env)->ReleaseStringUTFChars(env, ref_name, r);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return removed ? JNI_TRUE : JNI_FALSE;
}

/* --- Queue facade (append-log) + consumer offsets. Each call opens the loom for the op and closes. --- */

/* Copy an owned (ptr, len) result buffer into a Java byte[] and free the C buffer. */
static jbyteArray owned_bytes(JNIEnv *env, unsigned char *ptr, uintptr_t len) {
    jbyteArray arr = (*env)->NewByteArray(env, (jsize)len);
    if (arr != NULL && len > 0) {
        (*env)->SetByteArrayRegion(env, arr, 0, (jsize)len, (const jbyte *)ptr);
    }
    loom_bytes_free(ptr, len);
    return arr;
}

static const char *optional_utf(JNIEnv *env, jstring value) {
    return value ? (*env)->GetStringUTFChars(env, value, NULL) : NULL;
}

static void release_optional_utf(JNIEnv *env, jstring value, const char *chars) {
    if (value && chars) {
        (*env)->ReleaseStringUTFChars(env, value, chars);
    }
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeMetricsPutDescriptor(JNIEnv *env, jobject thiz, jstring path,
                                                          jstring workspace, jbyteArray descriptor,
                                                          jbyteArray passphrase, jbyteArray kek,
                                                          jstring auth_principal,
                                                          jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    jsize len = descriptor ? (*env)->GetArrayLength(env, descriptor) : 0;
    jbyte *bytes = descriptor ? (*env)->GetByteArrayElements(env, descriptor, NULL) : NULL;
    int32_t st = loom_metrics_put_descriptor(h, w, (const unsigned char *)bytes, (uintptr_t)len);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    if (bytes) (*env)->ReleaseByteArrayElements(env, descriptor, bytes, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMetricsGetDescriptor(JNIEnv *env, jobject thiz, jstring path,
                                                          jstring workspace, jstring name,
                                                          jbyteArray passphrase, jbyteArray kek,
                                                          jstring auth_principal,
                                                          jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    const char *n = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_metrics_get_descriptor(h, w, n, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    (*env)->ReleaseStringUTFChars(env, name, n);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return found ? owned_bytes(env, ptr, len) : NULL;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeMetricsPutObservation(JNIEnv *env, jobject thiz, jstring path,
                                                           jstring workspace, jstring descriptor_name,
                                                           jbyteArray observation,
                                                           jbyteArray passphrase, jbyteArray kek,
                                                           jstring auth_principal,
                                                           jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    const char *dname = (*env)->GetStringUTFChars(env, descriptor_name, NULL);
    jsize len = observation ? (*env)->GetArrayLength(env, observation) : 0;
    jbyte *bytes = observation ? (*env)->GetByteArrayElements(env, observation, NULL) : NULL;
    int32_t st = loom_metrics_put_observation(h, w, dname, (const unsigned char *)bytes, (uintptr_t)len);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    (*env)->ReleaseStringUTFChars(env, descriptor_name, dname);
    if (bytes) (*env)->ReleaseByteArrayElements(env, observation, bytes, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMetricsQuery(JNIEnv *env, jobject thiz, jstring path,
                                                  jstring workspace, jstring descriptor_name,
                                                  jlong from_timestamp_ms, jlong to_timestamp_ms,
                                                  jint max_series, jint max_groups, jint max_samples,
                                                  jlong max_output_bytes, jlong now_timestamp_ms,
                                                  jbyteArray passphrase, jbyteArray kek,
                                                  jstring auth_principal,
                                                  jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    const char *dname = (*env)->GetStringUTFChars(env, descriptor_name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_metrics_query_cbor(h, w, dname, (uint64_t)from_timestamp_ms, (uint64_t)to_timestamp_ms,
                                         (uint32_t)max_series, (uint32_t)max_groups, (uint32_t)max_samples,
                                         (uint64_t)max_output_bytes, (uint64_t)now_timestamp_ms, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    (*env)->ReleaseStringUTFChars(env, descriptor_name, dname);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeLogsPutRecord(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring workspace, jbyteArray record,
                                                   jbyteArray passphrase, jbyteArray kek,
                                                   jstring auth_principal,
                                                   jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    jsize len = record ? (*env)->GetArrayLength(env, record) : 0;
    jbyte *bytes = record ? (*env)->GetByteArrayElements(env, record, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t out_len = 0;
    int32_t st = loom_logs_put_record(h, w, (const unsigned char *)bytes, (uintptr_t)len, &ptr, &out_len);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    if (bytes) (*env)->ReleaseByteArrayElements(env, record, bytes, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    char *buf = malloc((size_t)out_len + 1);
    if (!buf) {
        loom_bytes_free(ptr, out_len);
        jclass ex = (*env)->FindClass(env, "java/lang/OutOfMemoryError");
        (*env)->ThrowNew(env, ex, "log record id allocation failed");
        return NULL;
    }
    if (out_len > 0 && ptr) memcpy(buf, ptr, (size_t)out_len);
    buf[out_len] = '\0';
    loom_bytes_free(ptr, out_len);
    jstring out = (*env)->NewStringUTF(env, buf);
    free(buf);
    return out;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeLogsGetRecord(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring workspace, jstring record_id,
                                                   jbyteArray passphrase, jbyteArray kek,
                                                   jstring auth_principal,
                                                   jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    const char *r = (*env)->GetStringUTFChars(env, record_id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_logs_get_record(h, w, r, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    (*env)->ReleaseStringUTFChars(env, record_id, r);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return found ? owned_bytes(env, ptr, len) : NULL;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeLogsQuery(JNIEnv *env, jobject thiz, jstring path,
                                               jstring workspace, jlong from_time_unix_nano,
                                               jlong to_time_unix_nano, jint max_records,
                                               jlong max_output_bytes, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal,
                                               jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_logs_query_cbor(h, w, (uint64_t)from_time_unix_nano, (uint64_t)to_time_unix_nano,
                                      (uint32_t)max_records, (uint64_t)max_output_bytes, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeTracesPutSpan(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring workspace, jbyteArray span,
                                                   jbyteArray passphrase, jbyteArray kek,
                                                   jstring auth_principal,
                                                   jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    jsize len = span ? (*env)->GetArrayLength(env, span) : 0;
    jbyte *bytes = span ? (*env)->GetByteArrayElements(env, span, NULL) : NULL;
    int32_t st = loom_traces_put_span(h, w, (const unsigned char *)bytes, (uintptr_t)len);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    if (bytes) (*env)->ReleaseByteArrayElements(env, span, bytes, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeTracesGetSpan(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring workspace, jstring trace_id, jstring span_id,
                                                   jbyteArray passphrase, jbyteArray kek,
                                                   jstring auth_principal,
                                                   jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    const char *t = (*env)->GetStringUTFChars(env, trace_id, NULL);
    const char *s = (*env)->GetStringUTFChars(env, span_id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_traces_get_span(h, w, t, s, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    (*env)->ReleaseStringUTFChars(env, trace_id, t);
    (*env)->ReleaseStringUTFChars(env, span_id, s);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return found ? owned_bytes(env, ptr, len) : NULL;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeTracesTraceSpans(JNIEnv *env, jobject thiz, jstring path,
                                                      jstring workspace, jstring trace_id,
                                                      jint max_spans, jlong max_output_bytes,
                                                      jbyteArray passphrase, jbyteArray kek,
                                                      jstring auth_principal,
                                                      jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    const char *t = (*env)->GetStringUTFChars(env, trace_id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_traces_trace_spans_cbor(h, w, t, (uint32_t)max_spans, (uint64_t)max_output_bytes, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    (*env)->ReleaseStringUTFChars(env, trace_id, t);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeTracesQuery(JNIEnv *env, jobject thiz, jstring path,
                                                 jstring workspace, jlong from_start_time_ns,
                                                 jlong to_start_time_ns, jint max_spans,
                                                 jlong max_output_bytes, jbyteArray passphrase,
                                                 jbyteArray kek, jstring auth_principal,
                                                 jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *w = (*env)->GetStringUTFChars(env, workspace, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_traces_query_cbor(h, w, (uint64_t)from_start_time_ns, (uint64_t)to_start_time_ns,
                                        (uint32_t)max_spans, (uint64_t)max_output_bytes, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, workspace, w);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSqlReadTable(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring table, jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *t = (*env)->GetStringUTFChars(env, table, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_read_table(h, n, t, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, table, t);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSqlReadTableAt(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring table, jstring commit, jbyteArray passphrase,
                                              jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *t = (*env)->GetStringUTFChars(env, table, NULL);
    const char *c = (*env)->GetStringUTFChars(env, commit, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_read_table_at(h, n, t, c, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, table, t);
    (*env)->ReleaseStringUTFChars(env, commit, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

/* Build capability report (0010 section 5) as canonical CBOR. No handle: a property of the build. */
JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_Loom_capabilities(JNIEnv *env, jobject thiz) {
    (void)thiz;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_capabilities(&ptr, &len);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_Loom_runtimeProfile(JNIEnv *env, jobject thiz) {
    (void)thiz;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_runtime_profile(&ptr, &len);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_Loom_studioSurfaceCatalogJson(JNIEnv *env, jobject thiz, jstring workspace,
                                                  jstring set) {
    (void)thiz;
    const char *workspace_chars = (*env)->GetStringUTFChars(env, workspace, NULL);
    const char *set_chars = (*env)->GetStringUTFChars(env, set, NULL);
    char *out = NULL;
    int32_t st = loom_studio_surface_catalog_json(workspace_chars, set_chars, &out);
    (*env)->ReleaseStringUTFChars(env, workspace, workspace_chars);
    (*env)->ReleaseStringUTFChars(env, set, set_chars);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring result = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return result;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeCasPut(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                      jbyteArray content, jbyteArray passphrase, jbyteArray kek,
                                      jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    jsize clen = content ? (*env)->GetArrayLength(env, content) : 0;
    jbyte *c = content ? (*env)->GetByteArrayElements(env, content, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_cas_put(h, n, (const unsigned char *)c, (uintptr_t)clen, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    if (c) (*env)->ReleaseByteArrayElements(env, content, c, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCasGet(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                      jstring digest, jbyteArray passphrase, jbyteArray kek,
                                      jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, digest, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_cas_get(h, n, d, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, digest, d);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeCasHas(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                      jstring digest, jbyteArray passphrase, jbyteArray kek,
                                      jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, digest, NULL);
    int32_t found = 0;
    int32_t st = loom_cas_has(h, n, d, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, digest, d);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeCasDelete(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring digest, jbyteArray passphrase, jbyteArray kek,
                                         jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, digest, NULL);
    int32_t found = 0;
    int32_t st = loom_cas_delete(h, n, d, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, digest, d);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeCasListJson(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jbyteArray passphrase, jbyteArray kek,
                                           jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    char *out = NULL;
    int32_t st = loom_cas_list_json(h, n, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "[]");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeMeetingsImportSnapshot(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring input_profile,
        jbyteArray snapshot, jboolean dry_run, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *profile = (*env)->GetStringUTFChars(env, input_profile, NULL);
    jsize snapshot_len = snapshot ? (*env)->GetArrayLength(env, snapshot) : 0;
    jbyte *snapshot_bytes = snapshot ? (*env)->GetByteArrayElements(env, snapshot, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_meetings_import_snapshot(h, n, profile, (const unsigned char *)snapshot_bytes,
                                               (uintptr_t)snapshot_len, dry_run ? 1 : 0, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, input_profile, profile);
    if (snapshot_bytes) (*env)->ReleaseByteArrayElements(env, snapshot, snapshot_bytes, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring result = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return result;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMeetingsSourceRead(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring source_id, jstring leaf,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *source = (*env)->GetStringUTFChars(env, source_id, NULL);
    const char *payload_leaf = (*env)->GetStringUTFChars(env, leaf, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_meetings_source_read(h, n, source, payload_leaf, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, source_id, source);
    (*env)->ReleaseStringUTFChars(env, leaf, payload_leaf);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

static jstring drive_string_result(JNIEnv *env, LoomSession *h, int32_t st, char *out) {
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring result = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return result;
}

static jbyteArray drive_bytes_result(JNIEnv *env, LoomSession *h, int32_t st,
                                     unsigned char *ptr, uintptr_t len) {
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

#define DRIVE_OPEN() \
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase); \
    if (!h) return NULL; \
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL); \
    const char *dw = (*env)->GetStringUTFChars(env, drive_workspace_id, NULL)

#define DRIVE_RELEASE_NS() \
    (*env)->ReleaseStringUTFChars(env, ns, n); \
    (*env)->ReleaseStringUTFChars(env, drive_workspace_id, dw)

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveListJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring folder_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *folder = (*env)->GetStringUTFChars(env, folder_id, NULL);
    char *out = NULL;
    int32_t st = loom_drive_list_json(h, n, dw, folder, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, folder_id, folder);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveStatJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring folder_id, jstring name, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *folder = (*env)->GetStringUTFChars(env, folder_id, NULL);
    const char *nm = (*env)->GetStringUTFChars(env, name, NULL);
    char *out = NULL;
    int32_t st = loom_drive_stat_json(h, n, dw, folder, nm, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, folder_id, folder);
    (*env)->ReleaseStringUTFChars(env, name, nm);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveReadFile(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring file_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *file = (*env)->GetStringUTFChars(env, file_id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_drive_read(h, n, dw, file, &ptr, &len);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, file_id, file);
    return drive_bytes_result(env, h, st, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveListVersionsJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring file_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *file = (*env)->GetStringUTFChars(env, file_id, NULL);
    char *out = NULL;
    int32_t st = loom_drive_list_versions_json(h, n, dw, file, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, file_id, file);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveListConflictsJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    char *out = NULL;
    int32_t st = loom_drive_list_conflicts_json(h, n, dw, &out);
    DRIVE_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveListSharesJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    char *out = NULL;
    int32_t st = loom_drive_list_shares_json(h, n, dw, &out);
    DRIVE_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveListRetentionJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    char *out = NULL;
    int32_t st = loom_drive_list_retention_json(h, n, dw, &out);
    DRIVE_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveCreateFolderJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring parent_folder_id, jstring folder_id, jstring name, jstring expected_root,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *parent = (*env)->GetStringUTFChars(env, parent_folder_id, NULL);
    const char *folder = (*env)->GetStringUTFChars(env, folder_id, NULL);
    const char *nm = (*env)->GetStringUTFChars(env, name, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_drive_create_folder_json(h, n, dw, parent, folder, nm, root, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, parent_folder_id, parent);
    (*env)->ReleaseStringUTFChars(env, folder_id, folder);
    (*env)->ReleaseStringUTFChars(env, name, nm);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveCreateUploadJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring upload_id, jstring parent_folder_id, jstring name, jstring file_id,
        jstring expected_root, jlong created_at_ms, jboolean replace_file,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *upload = (*env)->GetStringUTFChars(env, upload_id, NULL);
    const char *parent = (*env)->GetStringUTFChars(env, parent_folder_id, NULL);
    const char *nm = (*env)->GetStringUTFChars(env, name, NULL);
    const char *file = (*env)->GetStringUTFChars(env, file_id, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_drive_create_upload_json(
        h, n, dw, upload, parent, nm, file, root, (uint64_t)created_at_ms,
        replace_file ? 1 : 0, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, upload_id, upload);
    (*env)->ReleaseStringUTFChars(env, parent_folder_id, parent);
    (*env)->ReleaseStringUTFChars(env, name, nm);
    (*env)->ReleaseStringUTFChars(env, file_id, file);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveUploadChunkJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring upload_id, jbyteArray chunk, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *upload = (*env)->GetStringUTFChars(env, upload_id, NULL);
    jsize chunk_len = chunk ? (*env)->GetArrayLength(env, chunk) : 0;
    jbyte *chunk_bytes = chunk ? (*env)->GetByteArrayElements(env, chunk, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_drive_upload_chunk_json(
        h, n, dw, upload, (const unsigned char *)chunk_bytes, (uintptr_t)chunk_len, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, upload_id, upload);
    if (chunk_bytes) (*env)->ReleaseByteArrayElements(env, chunk, chunk_bytes, JNI_ABORT);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveCommitUploadJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring upload_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *upload = (*env)->GetStringUTFChars(env, upload_id, NULL);
    char *out = NULL;
    int32_t st = loom_drive_commit_upload_json(h, n, dw, upload, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, upload_id, upload);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveRenameJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring folder_id, jstring node_id, jstring new_name, jstring expected_root,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *folder = (*env)->GetStringUTFChars(env, folder_id, NULL);
    const char *node = (*env)->GetStringUTFChars(env, node_id, NULL);
    const char *nm = (*env)->GetStringUTFChars(env, new_name, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_drive_rename_json(h, n, dw, folder, node, nm, root, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, folder_id, folder);
    (*env)->ReleaseStringUTFChars(env, node_id, node);
    (*env)->ReleaseStringUTFChars(env, new_name, nm);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveMoveJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring source_folder_id, jstring target_folder_id, jstring node_id,
        jstring expected_root, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *source = (*env)->GetStringUTFChars(env, source_folder_id, NULL);
    const char *target = (*env)->GetStringUTFChars(env, target_folder_id, NULL);
    const char *node = (*env)->GetStringUTFChars(env, node_id, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_drive_move_json(h, n, dw, source, target, node, root, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, source_folder_id, source);
    (*env)->ReleaseStringUTFChars(env, target_folder_id, target);
    (*env)->ReleaseStringUTFChars(env, node_id, node);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveDeleteJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring folder_id, jstring node_id, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *folder = (*env)->GetStringUTFChars(env, folder_id, NULL);
    const char *node = (*env)->GetStringUTFChars(env, node_id, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_drive_delete_json(h, n, dw, folder, node, root, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, folder_id, folder);
    (*env)->ReleaseStringUTFChars(env, node_id, node);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveResolveConflictJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring conflict_id, jstring resolution, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *conflict = (*env)->GetStringUTFChars(env, conflict_id, NULL);
    const char *res = (*env)->GetStringUTFChars(env, resolution, NULL);
    char *out = NULL;
    int32_t st = loom_drive_resolve_conflict_json(h, n, dw, conflict, res, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, conflict_id, conflict);
    (*env)->ReleaseStringUTFChars(env, resolution, res);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveGrantShareJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring grant_id, jstring target_kind, jstring target_id, jstring principal,
        jstring role, jlong granted_at_ms, jlong expires_at_ms, jboolean has_expires_at_ms,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *grant = (*env)->GetStringUTFChars(env, grant_id, NULL);
    const char *kind = (*env)->GetStringUTFChars(env, target_kind, NULL);
    const char *target = (*env)->GetStringUTFChars(env, target_id, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *rl = (*env)->GetStringUTFChars(env, role, NULL);
    char *out = NULL;
    int32_t st = loom_drive_grant_share_json(
        h, n, dw, grant, kind, target, pr, rl, (uint64_t)granted_at_ms,
        (uint64_t)expires_at_ms, has_expires_at_ms ? 1 : 0, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, grant_id, grant);
    (*env)->ReleaseStringUTFChars(env, target_kind, kind);
    (*env)->ReleaseStringUTFChars(env, target_id, target);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, role, rl);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveRevokeShareJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring grant_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *grant = (*env)->GetStringUTFChars(env, grant_id, NULL);
    char *out = NULL;
    int32_t st = loom_drive_revoke_share_json(h, n, dw, grant, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, grant_id, grant);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveApplyShareExpiryJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jlong now_ms, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    char *out = NULL;
    int32_t st = loom_drive_apply_share_expiry_json(h, n, dw, (uint64_t)now_ms, &out);
    DRIVE_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDrivePinRetentionJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring pin_id, jstring kind, jstring root, jstring target_entity_id,
        jlong added_at_ms, jlong expires_at_ms, jboolean has_expires_at_ms,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *pin = (*env)->GetStringUTFChars(env, pin_id, NULL);
    const char *kd = (*env)->GetStringUTFChars(env, kind, NULL);
    const char *rt = (*env)->GetStringUTFChars(env, root, NULL);
    const char *target = target_entity_id ? (*env)->GetStringUTFChars(env, target_entity_id, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_drive_pin_retention_json(
        h, n, dw, pin, kd, rt, target, (uint64_t)added_at_ms, (uint64_t)expires_at_ms,
        has_expires_at_ms ? 1 : 0, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, pin_id, pin);
    (*env)->ReleaseStringUTFChars(env, kind, kd);
    (*env)->ReleaseStringUTFChars(env, root, rt);
    if (target) (*env)->ReleaseStringUTFChars(env, target_entity_id, target);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveUnpinRetentionJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jstring pin_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    const char *pin = (*env)->GetStringUTFChars(env, pin_id, NULL);
    char *out = NULL;
    int32_t st = loom_drive_unpin_retention_json(h, n, dw, pin, &out);
    DRIVE_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, pin_id, pin);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDriveApplyRetentionJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring drive_workspace_id,
        jlong now_ms, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    DRIVE_OPEN();
    char *out = NULL;
    int32_t st = loom_drive_apply_retention_json(h, n, dw, (uint64_t)now_ms, &out);
    DRIVE_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

#define TICKETS_OPEN() \
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase); \
    if (!h) return NULL; \
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL); \
    const char *tw = (*env)->GetStringUTFChars(env, ticket_workspace_id, NULL)

#define TICKETS_RELEASE_NS() \
    (*env)->ReleaseStringUTFChars(env, ns, n); \
    (*env)->ReleaseStringUTFChars(env, ticket_workspace_id, tw)

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsProjectCreateJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring project_id, jstring key_prefix, jstring name, jstring expected_root,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *prefix = (*env)->GetStringUTFChars(env, key_prefix, NULL);
    const char *nm = (*env)->GetStringUTFChars(env, name, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_project_create_json(h, n, tw, project, prefix, nm, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    (*env)->ReleaseStringUTFChars(env, key_prefix, prefix);
    (*env)->ReleaseStringUTFChars(env, name, nm);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsProjectRekeyJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring project_id, jstring key_prefix, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *prefix = (*env)->GetStringUTFChars(env, key_prefix, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_project_rekey_json(h, n, tw, project, prefix, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    (*env)->ReleaseStringUTFChars(env, key_prefix, prefix);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsProjectSettingsGetJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring project_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_project_settings_get_json(h, n, tw, project, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsProjectSettingsSetJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring project_id, jstring default_projection, jstring enable_projections_json,
        jstring disable_projections_json, jstring actor_enforcement,
        jstring project_owner_principal, jboolean clear_project_owner_principal,
        jstring acceptance_authorities_json, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *def = default_projection ? (*env)->GetStringUTFChars(env, default_projection, NULL) : NULL;
    const char *enable = (*env)->GetStringUTFChars(env, enable_projections_json, NULL);
    const char *disable = (*env)->GetStringUTFChars(env, disable_projections_json, NULL);
    const char *actor = actor_enforcement ? (*env)->GetStringUTFChars(env, actor_enforcement, NULL) : NULL;
    const char *owner = project_owner_principal ? (*env)->GetStringUTFChars(env, project_owner_principal, NULL) : NULL;
    const char *authorities = acceptance_authorities_json ? (*env)->GetStringUTFChars(env, acceptance_authorities_json, NULL) : NULL;
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_project_settings_set_json(
            h, n, tw, project, def, enable, disable, actor, owner,
            clear_project_owner_principal, authorities, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    if (def) (*env)->ReleaseStringUTFChars(env, default_projection, def);
    (*env)->ReleaseStringUTFChars(env, enable_projections_json, enable);
    (*env)->ReleaseStringUTFChars(env, disable_projections_json, disable);
    if (actor) (*env)->ReleaseStringUTFChars(env, actor_enforcement, actor);
    if (owner) (*env)->ReleaseStringUTFChars(env, project_owner_principal, owner);
    if (authorities) (*env)->ReleaseStringUTFChars(env, acceptance_authorities_json, authorities);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsFieldsJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring project_id, jstring projection, jstring operation, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *proj = (*env)->GetStringUTFChars(env, projection, NULL);
    const char *op = (*env)->GetStringUTFChars(env, operation, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_fields_json(h, n, tw, project, proj, op, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    (*env)->ReleaseStringUTFChars(env, projection, proj);
    (*env)->ReleaseStringUTFChars(env, operation, op);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsFieldPutJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring project_id, jstring field_id, jstring key, jstring name, jstring description,
        jstring field_type, jstring option_set, jint max_length, jboolean has_max_length,
        jboolean required, jboolean searchable, jboolean orderable, jstring cardinality,
        jstring applicable_type_ids_json, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *field = (*env)->GetStringUTFChars(env, field_id, NULL);
    const char *field_key = (*env)->GetStringUTFChars(env, key, NULL);
    const char *field_name = (*env)->GetStringUTFChars(env, name, NULL);
    const char *desc = description ? (*env)->GetStringUTFChars(env, description, NULL) : NULL;
    const char *typ = (*env)->GetStringUTFChars(env, field_type, NULL);
    const char *options = option_set ? (*env)->GetStringUTFChars(env, option_set, NULL) : NULL;
    const char *card = (*env)->GetStringUTFChars(env, cardinality, NULL);
    const char *applicable = (*env)->GetStringUTFChars(env, applicable_type_ids_json, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_field_put_json(
            h, n, tw, project, field, field_key, field_name, desc, typ, options,
            (uint32_t)max_length, has_max_length, required, searchable, orderable,
            card, applicable, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    (*env)->ReleaseStringUTFChars(env, field_id, field);
    (*env)->ReleaseStringUTFChars(env, key, field_key);
    (*env)->ReleaseStringUTFChars(env, name, field_name);
    if (desc) (*env)->ReleaseStringUTFChars(env, description, desc);
    (*env)->ReleaseStringUTFChars(env, field_type, typ);
    if (options) (*env)->ReleaseStringUTFChars(env, option_set, options);
    (*env)->ReleaseStringUTFChars(env, cardinality, card);
    (*env)->ReleaseStringUTFChars(env, applicable_type_ids_json, applicable);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsFieldRetireJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring project_id, jstring field_id, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *field = (*env)->GetStringUTFChars(env, field_id, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_field_retire_json(h, n, tw, project, field, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    (*env)->ReleaseStringUTFChars(env, field_id, field);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsCreateJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring project_id, jstring ticket_type, jstring external_source, jstring external_id,
        jstring fields_json, jstring policy_labels_json, jstring expected_root,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *typ = (*env)->GetStringUTFChars(env, ticket_type, NULL);
    const char *source = external_source ? (*env)->GetStringUTFChars(env, external_source, NULL) : NULL;
    const char *external = external_id ? (*env)->GetStringUTFChars(env, external_id, NULL) : NULL;
    const char *fields = (*env)->GetStringUTFChars(env, fields_json, NULL);
    const char *labels = (*env)->GetStringUTFChars(env, policy_labels_json, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_create_json(h, n, tw, project, typ, source, external, fields, labels, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    (*env)->ReleaseStringUTFChars(env, ticket_type, typ);
    if (source) (*env)->ReleaseStringUTFChars(env, external_source, source);
    if (external) (*env)->ReleaseStringUTFChars(env, external_id, external);
    (*env)->ReleaseStringUTFChars(env, fields_json, fields);
    (*env)->ReleaseStringUTFChars(env, policy_labels_json, labels);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsUpdateJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring ticket_id, jstring set_fields_json, jstring delete_fields_json, jstring action,
        jstring target_status, jstring observed_source_status, jstring observed_workflow_version,
        jstring assignee, jstring expected_root, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase, jstring comment_id,
        jstring comment_type, jstring comment_body, jstring comments_json,
        jstring relation_sets_json, jstring relation_removes_json) {
    (void)thiz;
    TICKETS_OPEN();
    const char *ticket = (*env)->GetStringUTFChars(env, ticket_id, NULL);
    const char *set_fields = (*env)->GetStringUTFChars(env, set_fields_json, NULL);
    const char *delete_fields = (*env)->GetStringUTFChars(env, delete_fields_json, NULL);
    const char *act = action ? (*env)->GetStringUTFChars(env, action, NULL) : NULL;
    const char *status = target_status ? (*env)->GetStringUTFChars(env, target_status, NULL) : NULL;
    const char *source_status = observed_source_status ? (*env)->GetStringUTFChars(env, observed_source_status, NULL) : NULL;
    const char *workflow = observed_workflow_version ? (*env)->GetStringUTFChars(env, observed_workflow_version, NULL) : NULL;
    const char *assign = assignee ? (*env)->GetStringUTFChars(env, assignee, NULL) : NULL;
    const char *comment = comment_id ? (*env)->GetStringUTFChars(env, comment_id, NULL) : NULL;
    const char *comment_kind = comment_type ? (*env)->GetStringUTFChars(env, comment_type, NULL) : NULL;
    const char *body = comment_body ? (*env)->GetStringUTFChars(env, comment_body, NULL) : NULL;
    const char *comments = comments_json ? (*env)->GetStringUTFChars(env, comments_json, NULL) : NULL;
    const char *relation_sets = relation_sets_json ? (*env)->GetStringUTFChars(env, relation_sets_json, NULL) : NULL;
    const char *relation_removes = relation_removes_json ? (*env)->GetStringUTFChars(env, relation_removes_json, NULL) : NULL;
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_update_json(
        h, n, tw, ticket, set_fields, delete_fields, act, status, source_status, workflow, assign,
        comment, comment_kind, body, root, comments, relation_sets, relation_removes, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, ticket_id, ticket);
    (*env)->ReleaseStringUTFChars(env, set_fields_json, set_fields);
    (*env)->ReleaseStringUTFChars(env, delete_fields_json, delete_fields);
    if (act) (*env)->ReleaseStringUTFChars(env, action, act);
    if (status) (*env)->ReleaseStringUTFChars(env, target_status, status);
    if (source_status) (*env)->ReleaseStringUTFChars(env, observed_source_status, source_status);
    if (workflow) (*env)->ReleaseStringUTFChars(env, observed_workflow_version, workflow);
    if (assign) (*env)->ReleaseStringUTFChars(env, assignee, assign);
    if (comment) (*env)->ReleaseStringUTFChars(env, comment_id, comment);
    if (comment_kind) (*env)->ReleaseStringUTFChars(env, comment_type, comment_kind);
    if (body) (*env)->ReleaseStringUTFChars(env, comment_body, body);
    if (comments) (*env)->ReleaseStringUTFChars(env, comments_json, comments);
    if (relation_sets) (*env)->ReleaseStringUTFChars(env, relation_sets_json, relation_sets);
    if (relation_removes) (*env)->ReleaseStringUTFChars(env, relation_removes_json, relation_removes);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsDeleteJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring ticket_id, jstring expected_root, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *ticket = (*env)->GetStringUTFChars(env, ticket_id, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_delete_json(h, n, tw, ticket, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, ticket_id, ticket);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsRelationSetJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring ticket_id, jstring relation_id, jstring kind, jstring target_id,
        jstring expected_root, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *ticket = (*env)->GetStringUTFChars(env, ticket_id, NULL);
    const char *relation = (*env)->GetStringUTFChars(env, relation_id, NULL);
    const char *kd = (*env)->GetStringUTFChars(env, kind, NULL);
    const char *target = (*env)->GetStringUTFChars(env, target_id, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_relation_set_json(h, n, tw, ticket, relation, kd, target, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, ticket_id, ticket);
    (*env)->ReleaseStringUTFChars(env, relation_id, relation);
    (*env)->ReleaseStringUTFChars(env, kind, kd);
    (*env)->ReleaseStringUTFChars(env, target_id, target);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsRelationRemoveJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring ticket_id, jstring relation_id, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *ticket = (*env)->GetStringUTFChars(env, ticket_id, NULL);
    const char *relation = (*env)->GetStringUTFChars(env, relation_id, NULL);
    const char *root = (*env)->GetStringUTFChars(env, expected_root, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_relation_remove_json(h, n, tw, ticket, relation, root, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, ticket_id, ticket);
    (*env)->ReleaseStringUTFChars(env, relation_id, relation);
    (*env)->ReleaseStringUTFChars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsGetJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring ticket_id, jstring projection, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *ticket = (*env)->GetStringUTFChars(env, ticket_id, NULL);
    const char *proj = (*env)->GetStringUTFChars(env, projection, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_get_json(h, n, tw, ticket, proj, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, ticket_id, ticket);
    (*env)->ReleaseStringUTFChars(env, projection, proj);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsListJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring projection, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *proj = (*env)->GetStringUTFChars(env, projection, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_list_json(h, n, tw, proj, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, projection, proj);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeTicketsHistoryJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring ticket_workspace_id,
        jstring ticket_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    TICKETS_OPEN();
    const char *ticket = (*env)->GetStringUTFChars(env, ticket_id, NULL);
    char *out = NULL;
    int32_t st = loom_tickets_history_json(h, n, tw, ticket, &out);
    TICKETS_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, ticket_id, ticket);
    return drive_string_result(env, h, st, out);
}

#undef TICKETS_OPEN
#undef TICKETS_RELEASE_NS

#define PAGES_OPEN() \
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase); \
    if (!h) return NULL; \
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL); \
    const char *pw = (*env)->GetStringUTFChars(env, page_workspace_id, NULL)

#define PAGES_RELEASE_NS() \
    (*env)->ReleaseStringUTFChars(env, ns, n); \
    (*env)->ReleaseStringUTFChars(env, page_workspace_id, pw)

static const char *optional_jstring_chars(JNIEnv *env, jstring value) {
    return value ? (*env)->GetStringUTFChars(env, value, NULL) : NULL;
}

static void release_optional_jstring_chars(JNIEnv *env, jstring value, const char *chars) {
    if (value && chars) {
        (*env)->ReleaseStringUTFChars(env, value, chars);
    }
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeSpacesCreateJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring space_id, jstring title, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *space = (*env)->GetStringUTFChars(env, space_id, NULL);
    const char *ttl = (*env)->GetStringUTFChars(env, title, NULL);
    const char *root = optional_jstring_chars(env, expected_root);
    char *out = NULL;
    int32_t st = loom_spaces_create_json(h, n, pw, space, ttl, root, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, space_id, space);
    (*env)->ReleaseStringUTFChars(env, title, ttl);
    release_optional_jstring_chars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeSpacesListJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    char *out = NULL;
    int32_t st = loom_spaces_list_json(h, n, pw, &out);
    PAGES_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeSpacesGetJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring space_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *space = (*env)->GetStringUTFChars(env, space_id, NULL);
    char *out = NULL;
    int32_t st = loom_spaces_get_json(h, n, pw, space, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, space_id, space);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativePagesCreateJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring page_id, jstring space_id, jstring parent_page_id, jstring title,
        jstring expected_root, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *page = (*env)->GetStringUTFChars(env, page_id, NULL);
    const char *space = (*env)->GetStringUTFChars(env, space_id, NULL);
    const char *parent = optional_jstring_chars(env, parent_page_id);
    const char *ttl = (*env)->GetStringUTFChars(env, title, NULL);
    const char *root = optional_jstring_chars(env, expected_root);
    char *out = NULL;
    int32_t st = loom_pages_create_json(h, n, pw, page, space, parent, ttl, root, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, page_id, page);
    (*env)->ReleaseStringUTFChars(env, space_id, space);
    release_optional_jstring_chars(env, parent_page_id, parent);
    (*env)->ReleaseStringUTFChars(env, title, ttl);
    release_optional_jstring_chars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativePagesUpdateJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring page_id, jstring body_text, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *page = (*env)->GetStringUTFChars(env, page_id, NULL);
    const char *body = (*env)->GetStringUTFChars(env, body_text, NULL);
    const char *root = optional_jstring_chars(env, expected_root);
    char *out = NULL;
    int32_t st = loom_pages_update_json(h, n, pw, page, body, root, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, page_id, page);
    (*env)->ReleaseStringUTFChars(env, body_text, body);
    release_optional_jstring_chars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativePagesPublishJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring page_id, jstring expected_root, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *page = (*env)->GetStringUTFChars(env, page_id, NULL);
    const char *root = optional_jstring_chars(env, expected_root);
    char *out = NULL;
    int32_t st = loom_pages_publish_json(h, n, pw, page, root, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, page_id, page);
    release_optional_jstring_chars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativePagesGetJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring page_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *page = (*env)->GetStringUTFChars(env, page_id, NULL);
    char *out = NULL;
    int32_t st = loom_pages_get_json(h, n, pw, page, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, page_id, page);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativePagesListJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    char *out = NULL;
    int32_t st = loom_pages_list_json(h, n, pw, &out);
    PAGES_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativePagesHistoryJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring page_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *page = (*env)->GetStringUTFChars(env, page_id, NULL);
    char *out = NULL;
    int32_t st = loom_pages_history_json(h, n, pw, page, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, page_id, page);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeStructuresCreateJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring structure_id, jstring space_id, jstring kind, jstring title, jstring expected_root,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *structure = (*env)->GetStringUTFChars(env, structure_id, NULL);
    const char *space = (*env)->GetStringUTFChars(env, space_id, NULL);
    const char *k = (*env)->GetStringUTFChars(env, kind, NULL);
    const char *ttl = (*env)->GetStringUTFChars(env, title, NULL);
    const char *root = optional_jstring_chars(env, expected_root);
    char *out = NULL;
    int32_t st = loom_structures_create_json(h, n, pw, structure, space, k, ttl, root, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, structure_id, structure);
    (*env)->ReleaseStringUTFChars(env, space_id, space);
    (*env)->ReleaseStringUTFChars(env, kind, k);
    (*env)->ReleaseStringUTFChars(env, title, ttl);
    release_optional_jstring_chars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

#define STRUCTURE_NODE_FN(java_name, c_name) \
JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_LoomNative_##java_name( \
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id, \
        jstring structure_id, jstring node_id, jstring kind, jstring label, jstring body_digest, \
        jstring entity_ref, jstring expected_root, jbyteArray passphrase, jbyteArray kek, \
        jstring auth_principal, jbyteArray auth_passphrase) { \
    (void)thiz; \
    PAGES_OPEN(); \
    const char *structure = (*env)->GetStringUTFChars(env, structure_id, NULL); \
    const char *node = (*env)->GetStringUTFChars(env, node_id, NULL); \
    const char *k = (*env)->GetStringUTFChars(env, kind, NULL); \
    const char *lbl = (*env)->GetStringUTFChars(env, label, NULL); \
    const char *digest = optional_jstring_chars(env, body_digest); \
    const char *entity = optional_jstring_chars(env, entity_ref); \
    const char *root = optional_jstring_chars(env, expected_root); \
    char *out = NULL; \
    int32_t st = c_name(h, n, pw, structure, node, k, lbl, digest, entity, root, &out); \
    PAGES_RELEASE_NS(); \
    (*env)->ReleaseStringUTFChars(env, structure_id, structure); \
    (*env)->ReleaseStringUTFChars(env, node_id, node); \
    (*env)->ReleaseStringUTFChars(env, kind, k); \
    (*env)->ReleaseStringUTFChars(env, label, lbl); \
    release_optional_jstring_chars(env, body_digest, digest); \
    release_optional_jstring_chars(env, entity_ref, entity); \
    release_optional_jstring_chars(env, expected_root, root); \
    return drive_string_result(env, h, st, out); \
}

STRUCTURE_NODE_FN(nativeStructuresAddNodeJson, loom_structures_add_node_json)
STRUCTURE_NODE_FN(nativeStructuresUpdateNodeJson, loom_structures_update_node_json)

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeStructuresBindJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring structure_id, jstring node_id, jstring entity_ref, jstring expected_root,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *structure = (*env)->GetStringUTFChars(env, structure_id, NULL);
    const char *node = (*env)->GetStringUTFChars(env, node_id, NULL);
    const char *entity = optional_jstring_chars(env, entity_ref);
    const char *root = optional_jstring_chars(env, expected_root);
    char *out = NULL;
    int32_t st = loom_structures_bind_json(h, n, pw, structure, node, entity, root, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, structure_id, structure);
    (*env)->ReleaseStringUTFChars(env, node_id, node);
    release_optional_jstring_chars(env, entity_ref, entity);
    release_optional_jstring_chars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeStructuresMoveNodeJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring structure_id, jstring node_id, jstring parent_node_id, jstring label,
        jstring expected_root, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *structure = (*env)->GetStringUTFChars(env, structure_id, NULL);
    const char *node = (*env)->GetStringUTFChars(env, node_id, NULL);
    const char *parent = optional_jstring_chars(env, parent_node_id);
    const char *lbl = optional_jstring_chars(env, label);
    const char *root = optional_jstring_chars(env, expected_root);
    char *out = NULL;
    int32_t st = loom_structures_move_node_json(h, n, pw, structure, node, parent, lbl, root, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, structure_id, structure);
    (*env)->ReleaseStringUTFChars(env, node_id, node);
    release_optional_jstring_chars(env, parent_node_id, parent);
    release_optional_jstring_chars(env, label, lbl);
    release_optional_jstring_chars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeStructuresLinkNodeJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring structure_id, jstring edge_id, jstring src_node_id, jstring dst_node_id,
        jstring label, jstring target_ref, jstring expected_root, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *structure = (*env)->GetStringUTFChars(env, structure_id, NULL);
    const char *edge = (*env)->GetStringUTFChars(env, edge_id, NULL);
    const char *src = (*env)->GetStringUTFChars(env, src_node_id, NULL);
    const char *dst = (*env)->GetStringUTFChars(env, dst_node_id, NULL);
    const char *lbl = (*env)->GetStringUTFChars(env, label, NULL);
    const char *target = optional_jstring_chars(env, target_ref);
    const char *root = optional_jstring_chars(env, expected_root);
    char *out = NULL;
    int32_t st = loom_structures_link_node_json(h, n, pw, structure, edge, src, dst, lbl, target, root, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, structure_id, structure);
    (*env)->ReleaseStringUTFChars(env, edge_id, edge);
    (*env)->ReleaseStringUTFChars(env, src_node_id, src);
    (*env)->ReleaseStringUTFChars(env, dst_node_id, dst);
    (*env)->ReleaseStringUTFChars(env, label, lbl);
    release_optional_jstring_chars(env, target_ref, target);
    release_optional_jstring_chars(env, expected_root, root);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeStructuresDecomposeToTicketsJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring structure_id, jstring items_json, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *structure = (*env)->GetStringUTFChars(env, structure_id, NULL);
    const char *items = (*env)->GetStringUTFChars(env, items_json, NULL);
    char *out = NULL;
    int32_t st = loom_structures_decompose_to_tickets_json(h, n, pw, structure, items, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, structure_id, structure);
    (*env)->ReleaseStringUTFChars(env, items_json, items);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeStructuresGetJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jstring structure_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    const char *structure = (*env)->GetStringUTFChars(env, structure_id, NULL);
    char *out = NULL;
    int32_t st = loom_structures_get_json(h, n, pw, structure, &out);
    PAGES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, structure_id, structure);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeStructuresListJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring page_workspace_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    PAGES_OPEN();
    char *out = NULL;
    int32_t st = loom_structures_list_json(h, n, pw, &out);
    PAGES_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

#undef PAGES_OPEN
#undef PAGES_RELEASE_NS
#undef STRUCTURE_NODE_FN

#define LANES_OPEN() \
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase); \
    if (!h) return NULL; \
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL)

#define LANES_RELEASE_NS() \
    (*env)->ReleaseStringUTFChars(env, ns, n)

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeLanesCreate(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jbyteArray lane,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    LANES_OPEN();
    jbyte *bytes = (*env)->GetByteArrayElements(env, lane, NULL);
    jsize len = (*env)->GetArrayLength(env, lane);
    unsigned char *ptr = NULL;
    uintptr_t out_len = 0;
    int32_t st = loom_lanes_create_cbor(h, n, (const unsigned char *)bytes, (uintptr_t)len,
                                        &ptr, &out_len);
    LANES_RELEASE_NS();
    (*env)->ReleaseByteArrayElements(env, lane, bytes, JNI_ABORT);
    return drive_bytes_result(env, h, st, ptr, out_len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeLanesGet(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring lane_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    LANES_OPEN();
    const char *lane = (*env)->GetStringUTFChars(env, lane_id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t out_len = 0;
    int32_t found = 0;
    int32_t st = loom_lanes_get_cbor(h, n, lane, &ptr, &out_len, &found);
    LANES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, lane_id, lane);
    if (st != 0) {
        return drive_bytes_result(env, h, st, ptr, out_len);
    }
    if (!found) {
        loom_close(h);
        return NULL;
    }
    return drive_bytes_result(env, h, st, ptr, out_len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeLanesList(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LANES_OPEN();
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_lanes_list_cbor(h, n, &ptr, &len);
    LANES_RELEASE_NS();
    return drive_bytes_result(env, h, st, ptr, len);
}

#define LANES_MUTATE_STRING(java_name, c_name) \
JNIEXPORT jbyteArray JNICALL \
Java_ai_uldren_loom_LoomNative_##java_name( \
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring lane_id, \
        jstring value, jstring updated_by, jbyteArray passphrase, jbyteArray kek, \
        jstring auth_principal, jbyteArray auth_passphrase) { \
    (void)thiz; \
    LANES_OPEN(); \
    const char *lane = (*env)->GetStringUTFChars(env, lane_id, NULL); \
    const char *val = (*env)->GetStringUTFChars(env, value, NULL); \
    const char *actor = (*env)->GetStringUTFChars(env, updated_by, NULL); \
    unsigned char *ptr = NULL; \
    uintptr_t len = 0; \
    int32_t st = c_name(h, n, lane, val, actor, &ptr, &len); \
    LANES_RELEASE_NS(); \
    (*env)->ReleaseStringUTFChars(env, lane_id, lane); \
    (*env)->ReleaseStringUTFChars(env, value, val); \
    (*env)->ReleaseStringUTFChars(env, updated_by, actor); \
    return drive_bytes_result(env, h, st, ptr, len); \
}

LANES_MUTATE_STRING(nativeLanesTicketRemove, loom_lanes_ticket_remove_cbor)

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeLanesUpdate(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring lane_id, jstring title,
        jstring description, jstring lane_status, jstring status_report,
        jstring reviewer_feedback, jstring updated_by, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LANES_OPEN();
    const char *lane = (*env)->GetStringUTFChars(env, lane_id, NULL);
    const char *title_chars = optional_jstring_chars(env, title);
    const char *description_chars = optional_jstring_chars(env, description);
    const char *lane_status_chars = optional_jstring_chars(env, lane_status);
    const char *status_report_chars = optional_jstring_chars(env, status_report);
    const char *reviewer_feedback_chars = optional_jstring_chars(env, reviewer_feedback);
    const char *actor = (*env)->GetStringUTFChars(env, updated_by, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_lanes_update_cbor(h, n, lane, title_chars, description_chars,
                                        lane_status_chars, status_report_chars,
                                        reviewer_feedback_chars, actor, &ptr, &len);
    LANES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, lane_id, lane);
    release_optional_jstring_chars(env, title, title_chars);
    release_optional_jstring_chars(env, description, description_chars);
    release_optional_jstring_chars(env, lane_status, lane_status_chars);
    release_optional_jstring_chars(env, status_report, status_report_chars);
    release_optional_jstring_chars(env, reviewer_feedback, reviewer_feedback_chars);
    (*env)->ReleaseStringUTFChars(env, updated_by, actor);
    return drive_bytes_result(env, h, st, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeLanesTicketAdd(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring lane_id, jstring ticket_id,
        jstring updated_by, jstring placement, jstring anchor, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LANES_OPEN();
    const char *lane = (*env)->GetStringUTFChars(env, lane_id, NULL);
    const char *ticket = (*env)->GetStringUTFChars(env, ticket_id, NULL);
    const char *actor = (*env)->GetStringUTFChars(env, updated_by, NULL);
    const char *place = optional_jstring_chars(env, placement);
    const char *anchor_chars = optional_jstring_chars(env, anchor);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st =
        loom_lanes_ticket_add_cbor(h, n, lane, ticket, actor, place, anchor_chars, &ptr, &len);
    LANES_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, lane_id, lane);
    (*env)->ReleaseStringUTFChars(env, ticket_id, ticket);
    (*env)->ReleaseStringUTFChars(env, updated_by, actor);
    release_optional_jstring_chars(env, placement, place);
    release_optional_jstring_chars(env, anchor, anchor_chars);
    return drive_bytes_result(env, h, st, ptr, len);
}

#undef LANES_OPEN
#undef LANES_RELEASE_NS
#undef LANES_MUTATE_STRING

#define CHAT_OPEN() \
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase); \
    if (!h) return NULL; \
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL); \
    const char *cw = (*env)->GetStringUTFChars(env, chat_workspace_id, NULL)

#define CHAT_RELEASE_NS() \
    (*env)->ReleaseStringUTFChars(env, ns, n); \
    (*env)->ReleaseStringUTFChars(env, chat_workspace_id, cw)

static uint64_t chat_parse_u64(JNIEnv *env, jstring value) {
    const char *text = (*env)->GetStringUTFChars(env, value, NULL);
    uint64_t parsed = (uint64_t)strtoull(text, NULL, 10);
    (*env)->ReleaseStringUTFChars(env, value, text);
    return parsed;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatCreateChannelJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring channel_handle, jstring name, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *handle = (*env)->GetStringUTFChars(env, channel_handle, NULL);
    const char *nm = (*env)->GetStringUTFChars(env, name, NULL);
    char *out = NULL;
    int32_t st = loom_chat_create_channel_json(h, n, cw, channel, handle, nm, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, channel_handle, handle);
    (*env)->ReleaseStringUTFChars(env, name, nm);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatRenameChannelJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring selector, jstring channel_handle, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *sel = (*env)->GetStringUTFChars(env, selector, NULL);
    const char *handle = (*env)->GetStringUTFChars(env, channel_handle, NULL);
    char *out = NULL;
    int32_t st = loom_chat_rename_channel_json(h, n, cw, sel, handle, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, selector, sel);
    (*env)->ReleaseStringUTFChars(env, channel_handle, handle);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatListChannelsJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    char *out = NULL;
    int32_t st = loom_chat_list_channels_json(h, n, cw, &out);
    CHAT_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatPostMessageJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring message_id, jstring thread_id, jstring body_text,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *message = (*env)->GetStringUTFChars(env, message_id, NULL);
    const char *thread = thread_id ? (*env)->GetStringUTFChars(env, thread_id, NULL) : NULL;
    const char *body = (*env)->GetStringUTFChars(env, body_text, NULL);
    char *out = NULL;
    int32_t st = loom_chat_post_message_json(h, n, cw, channel, message, thread, body, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, message_id, message);
    if (thread) (*env)->ReleaseStringUTFChars(env, thread_id, thread);
    (*env)->ReleaseStringUTFChars(env, body_text, body);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatEditMessageJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring message_id, jstring body_text, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *message = (*env)->GetStringUTFChars(env, message_id, NULL);
    const char *body = (*env)->GetStringUTFChars(env, body_text, NULL);
    char *out = NULL;
    int32_t st = loom_chat_edit_message_json(h, n, cw, channel, message, body, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, message_id, message);
    (*env)->ReleaseStringUTFChars(env, body_text, body);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatRedactMessageJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring message_id, jstring reason, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *message = (*env)->GetStringUTFChars(env, message_id, NULL);
    const char *why = reason ? (*env)->GetStringUTFChars(env, reason, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_chat_redact_message_json(h, n, cw, channel, message, why, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, message_id, message);
    if (why) (*env)->ReleaseStringUTFChars(env, reason, why);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatCreateThreadJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring thread_id, jstring parent_message_id, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *thread = (*env)->GetStringUTFChars(env, thread_id, NULL);
    const char *parent = (*env)->GetStringUTFChars(env, parent_message_id, NULL);
    char *out = NULL;
    int32_t st = loom_chat_create_thread_json(h, n, cw, channel, thread, parent, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, thread_id, thread);
    (*env)->ReleaseStringUTFChars(env, parent_message_id, parent);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatCreateTaskJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring task_id, jstring message_id, jstring title,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *task = (*env)->GetStringUTFChars(env, task_id, NULL);
    const char *message = (*env)->GetStringUTFChars(env, message_id, NULL);
    const char *ttl = (*env)->GetStringUTFChars(env, title, NULL);
    char *out = NULL;
    int32_t st = loom_chat_create_task_json(h, n, cw, channel, task, message, ttl, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, task_id, task);
    (*env)->ReleaseStringUTFChars(env, message_id, message);
    (*env)->ReleaseStringUTFChars(env, title, ttl);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatClaimTaskJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring task_id, jstring claim_id, jstring lease_token,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *task = (*env)->GetStringUTFChars(env, task_id, NULL);
    const char *claim = (*env)->GetStringUTFChars(env, claim_id, NULL);
    const char *lease = lease_token ? (*env)->GetStringUTFChars(env, lease_token, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_chat_claim_task_json(h, n, cw, channel, task, claim, lease, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, task_id, task);
    (*env)->ReleaseStringUTFChars(env, claim_id, claim);
    if (lease) (*env)->ReleaseStringUTFChars(env, lease_token, lease);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatCompleteTaskJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring task_id, jstring claim_id, jstring result_message_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *task = (*env)->GetStringUTFChars(env, task_id, NULL);
    const char *claim = (*env)->GetStringUTFChars(env, claim_id, NULL);
    const char *result = result_message_id ? (*env)->GetStringUTFChars(env, result_message_id, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_chat_complete_task_json(h, n, cw, channel, task, claim, result, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, task_id, task);
    (*env)->ReleaseStringUTFChars(env, claim_id, claim);
    if (result) (*env)->ReleaseStringUTFChars(env, result_message_id, result);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatInvokeAgentJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring invocation_id, jstring agent_principal,
        jstring source_message_ids_json, jstring prompt_text, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *invocation = (*env)->GetStringUTFChars(env, invocation_id, NULL);
    const char *agent = (*env)->GetStringUTFChars(env, agent_principal, NULL);
    const char *sources = (*env)->GetStringUTFChars(env, source_message_ids_json, NULL);
    const char *prompt = (*env)->GetStringUTFChars(env, prompt_text, NULL);
    char *out = NULL;
    int32_t st = loom_chat_invoke_agent_json(h, n, cw, channel, invocation, agent, sources, prompt, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, invocation_id, invocation);
    (*env)->ReleaseStringUTFChars(env, agent_principal, agent);
    (*env)->ReleaseStringUTFChars(env, source_message_ids_json, sources);
    (*env)->ReleaseStringUTFChars(env, prompt_text, prompt);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatAgentReplyJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring invocation_id, jstring message_id, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *invocation = (*env)->GetStringUTFChars(env, invocation_id, NULL);
    const char *message = (*env)->GetStringUTFChars(env, message_id, NULL);
    char *out = NULL;
    int32_t st = loom_chat_agent_reply_json(h, n, cw, channel, invocation, message, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, invocation_id, invocation);
    (*env)->ReleaseStringUTFChars(env, message_id, message);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatRequestHandoffJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring handoff_id, jstring from_agent_principal,
        jstring to_principal, jstring reason, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    const char *handoff = (*env)->GetStringUTFChars(env, handoff_id, NULL);
    const char *from = (*env)->GetStringUTFChars(env, from_agent_principal, NULL);
    const char *to = to_principal ? (*env)->GetStringUTFChars(env, to_principal, NULL) : NULL;
    const char *why = reason ? (*env)->GetStringUTFChars(env, reason, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_chat_request_handoff_json(h, n, cw, channel, handoff, from, to, why, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    (*env)->ReleaseStringUTFChars(env, handoff_id, handoff);
    (*env)->ReleaseStringUTFChars(env, from_agent_principal, from);
    if (to) (*env)->ReleaseStringUTFChars(env, to_principal, to);
    if (why) (*env)->ReleaseStringUTFChars(env, reason, why);
    return drive_string_result(env, h, st, out);
}

#define CHAT_EVENT_FUNC(name, c_name) \
JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_LoomNative_##name( \
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id, \
        jstring channel_id, jstring message_id, jstring kind, jbyteArray passphrase, \
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) { \
    (void)thiz; \
    CHAT_OPEN(); \
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL); \
    const char *message = (*env)->GetStringUTFChars(env, message_id, NULL); \
    const char *kd = (*env)->GetStringUTFChars(env, kind, NULL); \
    char *out = NULL; \
    int32_t st = c_name(h, n, cw, channel, message, kd, &out); \
    CHAT_RELEASE_NS(); \
    (*env)->ReleaseStringUTFChars(env, channel_id, channel); \
    (*env)->ReleaseStringUTFChars(env, message_id, message); \
    (*env)->ReleaseStringUTFChars(env, kind, kd); \
    return drive_string_result(env, h, st, out); \
}

CHAT_EVENT_FUNC(nativeChatAddReactionJson, loom_chat_add_reaction_json)
CHAT_EVENT_FUNC(nativeChatRemoveReactionJson, loom_chat_remove_reaction_json)

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatEmojiListJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    char *out = NULL;
    int32_t st = loom_chat_emoji_list_json(h, n, cw, &out);
    CHAT_RELEASE_NS();
    return drive_string_result(env, h, st, out);
}

#define CHAT_KIND_FUNC(name, c_name) \
JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_LoomNative_##name( \
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id, \
        jstring kind, jbyteArray passphrase, jbyteArray kek, jstring auth_principal, \
        jbyteArray auth_passphrase) { \
    (void)thiz; \
    CHAT_OPEN(); \
    const char *kd = (*env)->GetStringUTFChars(env, kind, NULL); \
    char *out = NULL; \
    int32_t st = c_name(h, n, cw, kd, &out); \
    CHAT_RELEASE_NS(); \
    (*env)->ReleaseStringUTFChars(env, kind, kd); \
    return drive_string_result(env, h, st, out); \
}

CHAT_KIND_FUNC(nativeChatEmojiRegisterJson, loom_chat_emoji_register_json)
CHAT_KIND_FUNC(nativeChatEmojiUnregisterJson, loom_chat_emoji_unregister_json)

#define CHAT_CHANNEL_FUNC(name, c_name) \
JNIEXPORT jstring JNICALL \
Java_ai_uldren_loom_LoomNative_##name( \
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id, \
        jstring channel_id, jbyteArray passphrase, jbyteArray kek, jstring auth_principal, \
        jbyteArray auth_passphrase) { \
    (void)thiz; \
    CHAT_OPEN(); \
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL); \
    char *out = NULL; \
    int32_t st = c_name(h, n, cw, channel, &out); \
    CHAT_RELEASE_NS(); \
    (*env)->ReleaseStringUTFChars(env, channel_id, channel); \
    return drive_string_result(env, h, st, out); \
}

CHAT_CHANNEL_FUNC(nativeChatMessagesJson, loom_chat_messages_json)
CHAT_CHANNEL_FUNC(nativeChatCursorJson, loom_chat_cursor_json)

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatUpdateCursorJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring next_sequence, jbyteArray passphrase, jbyteArray kek,
        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    uint64_t next = chat_parse_u64(env, next_sequence);
    char *out = NULL;
    int32_t st = loom_chat_update_cursor_json(h, n, cw, channel, next, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    return drive_string_result(env, h, st, out);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeChatFetchEventsJson(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring chat_workspace_id,
        jstring channel_id, jstring from_sequence, jstring max, jbyteArray passphrase,
        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    CHAT_OPEN();
    const char *channel = (*env)->GetStringUTFChars(env, channel_id, NULL);
    uint64_t from = chat_parse_u64(env, from_sequence);
    uintptr_t limit = (uintptr_t)chat_parse_u64(env, max);
    char *out = NULL;
    int32_t st = loom_chat_fetch_events_json(h, n, cw, channel, from, limit, &out);
    CHAT_RELEASE_NS();
    (*env)->ReleaseStringUTFChars(env, channel_id, channel);
    return drive_string_result(env, h, st, out);
}

#undef CHAT_EVENT_FUNC
#undef CHAT_KIND_FUNC
#undef CHAT_CHANNEL_FUNC
#undef CHAT_OPEN
#undef CHAT_RELEASE_NS

#undef DRIVE_OPEN
#undef DRIVE_RELEASE_NS

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeFsImport(JNIEnv *env, jobject thiz, jstring path,
                                        jstring ns, jstring src_path, jboolean commit,
                                        jboolean dry_run, jbyteArray passphrase, jbyteArray kek,
                                        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *src = (*env)->GetStringUTFChars(env, src_path, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_fs_import(h, n, src, commit ? 1 : 0, dry_run ? 1 : 0, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, src_path, src);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeFsExport(JNIEnv *env, jobject thiz, jstring path,
                                        jstring ns, jstring dst_path, jstring revision,
                                        jboolean dry_run, jbyteArray passphrase, jbyteArray kek,
                                        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *dst = (*env)->GetStringUTFChars(env, dst_path, NULL);
    const char *rev = optional_utf(env, revision);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_fs_export(h, n, dst, rev, dry_run ? 1 : 0, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, dst_path, dst);
    release_optional_utf(env, revision, rev);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeArchiveImport(JNIEnv *env, jobject thiz, jstring path,
                                             jstring ns, jstring src_path, jstring kind,
                                             jboolean dry_run, jbyteArray passphrase,
                                             jbyteArray kek, jstring auth_principal,
                                             jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *src = (*env)->GetStringUTFChars(env, src_path, NULL);
    const char *k = (*env)->GetStringUTFChars(env, kind, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_archive_import(h, n, src, k, dry_run ? 1 : 0, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, src_path, src);
    (*env)->ReleaseStringUTFChars(env, kind, k);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeArchiveExport(JNIEnv *env, jobject thiz, jstring path,
                                             jstring ns, jstring dst_path, jstring kind,
                                             jstring revision, jboolean dry_run,
                                             jbyteArray passphrase, jbyteArray kek,
                                             jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *dst = (*env)->GetStringUTFChars(env, dst_path, NULL);
    const char *k = (*env)->GetStringUTFChars(env, kind, NULL);
    const char *rev = optional_utf(env, revision);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_archive_export(h, n, dst, k, rev, dry_run ? 1 : 0, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, dst_path, dst);
    (*env)->ReleaseStringUTFChars(env, kind, k);
    release_optional_utf(env, revision, rev);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCarImport(JNIEnv *env, jobject thiz, jstring path,
                                         jstring src_path, jboolean dry_run, jbyteArray passphrase,
                                         jbyteArray kek, jstring auth_principal,
                                         jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *src = (*env)->GetStringUTFChars(env, src_path, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_car_import(h, src, dry_run ? 1 : 0, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, src_path, src);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCarExport(JNIEnv *env, jobject thiz, jstring path,
                                         jstring ns, jstring dst_path, jboolean dry_run,
                                         jbyteArray passphrase, jbyteArray kek,
                                         jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *dst = (*env)->GetStringUTFChars(env, dst_path, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_car_export(h, n, dst, dry_run ? 1 : 0, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, dst_path, dst);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeKvPut(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                     jstring collection, jbyteArray key, jbyteArray value,
                                     jbyteArray passphrase, jbyteArray kek,
                                     jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    jsize klen = key ? (*env)->GetArrayLength(env, key) : 0;
    jbyte *k = key ? (*env)->GetByteArrayElements(env, key, NULL) : NULL;
    jsize vlen = value ? (*env)->GetArrayLength(env, value) : 0;
    jbyte *v = value ? (*env)->GetByteArrayElements(env, value, NULL) : NULL;
    int32_t st = loom_kv_put(h, n, m, (const unsigned char *)k, (uintptr_t)klen,
                             (const unsigned char *)v, (uintptr_t)vlen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    if (k) (*env)->ReleaseByteArrayElements(env, key, k, JNI_ABORT);
    if (v) (*env)->ReleaseByteArrayElements(env, value, v, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeKvGet(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                     jstring collection, jbyteArray key, jbyteArray passphrase,
                                     jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    jsize klen = key ? (*env)->GetArrayLength(env, key) : 0;
    jbyte *k = key ? (*env)->GetByteArrayElements(env, key, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_kv_get(h, n, m, (const unsigned char *)k, (uintptr_t)klen, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    if (k) (*env)->ReleaseByteArrayElements(env, key, k, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeKvDelete(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                        jstring collection, jbyteArray key, jbyteArray passphrase,
                                        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    jsize klen = key ? (*env)->GetArrayLength(env, key) : 0;
    jbyte *k = key ? (*env)->GetByteArrayElements(env, key, NULL) : NULL;
    int32_t found = 0;
    int32_t st = loom_kv_delete(h, n, m, (const unsigned char *)k, (uintptr_t)klen, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    if (k) (*env)->ReleaseByteArrayElements(env, key, k, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeKvList(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                      jstring collection, jbyteArray passphrase, jbyteArray kek,
                                      jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_kv_list_cbor(h, n, m, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeKvRange(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                       jstring collection, jbyteArray lo, jbyteArray hi,
                                       jbyteArray passphrase, jbyteArray kek,
                                       jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    jsize lolen = lo ? (*env)->GetArrayLength(env, lo) : 0;
    jbyte *lob = lo ? (*env)->GetByteArrayElements(env, lo, NULL) : NULL;
    jsize hilen = hi ? (*env)->GetArrayLength(env, hi) : 0;
    jbyte *hib = hi ? (*env)->GetByteArrayElements(env, hi, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_kv_range_cbor(h, n, m, (const unsigned char *)lob, (uintptr_t)lolen,
                                    (const unsigned char *)hib, (uintptr_t)hilen, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    if (lob) (*env)->ReleaseByteArrayElements(env, lo, lob, JNI_ABORT);
    if (hib) (*env)->ReleaseByteArrayElements(env, hi, hib, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

static jobjectArray document_text_result(JNIEnv *env, char *text, char *digest, char *entity_tag) {
    jclass object_class = (*env)->FindClass(env, "java/lang/Object");
    jobjectArray out = (*env)->NewObjectArray(env, 3, object_class, NULL);
    jstring text_value = (*env)->NewStringUTF(env, text ? text : "");
    jstring digest_value = (*env)->NewStringUTF(env, digest ? digest : "");
    jstring entity_tag_value = (*env)->NewStringUTF(env, entity_tag ? entity_tag : "");
    (*env)->SetObjectArrayElement(env, out, 0, text_value);
    (*env)->SetObjectArrayElement(env, out, 1, digest_value);
    (*env)->SetObjectArrayElement(env, out, 2, entity_tag_value);
    (*env)->DeleteLocalRef(env, text_value);
    (*env)->DeleteLocalRef(env, digest_value);
    (*env)->DeleteLocalRef(env, entity_tag_value);
    if (text) loom_string_free(text);
    if (digest) loom_string_free(digest);
    if (entity_tag) loom_string_free(entity_tag);
    return out;
}

static jobjectArray document_binary_result(JNIEnv *env, unsigned char *ptr, uintptr_t len, char *digest, char *entity_tag) {
    jclass object_class = (*env)->FindClass(env, "java/lang/Object");
    jobjectArray out = (*env)->NewObjectArray(env, 3, object_class, NULL);
    jbyteArray bytes = owned_bytes(env, ptr, len);
    jstring digest_value = (*env)->NewStringUTF(env, digest ? digest : "");
    jstring entity_tag_value = (*env)->NewStringUTF(env, entity_tag ? entity_tag : "");
    (*env)->SetObjectArrayElement(env, out, 0, bytes);
    (*env)->SetObjectArrayElement(env, out, 1, digest_value);
    (*env)->SetObjectArrayElement(env, out, 2, entity_tag_value);
    (*env)->DeleteLocalRef(env, bytes);
    (*env)->DeleteLocalRef(env, digest_value);
    (*env)->DeleteLocalRef(env, entity_tag_value);
    if (digest) loom_string_free(digest);
    if (entity_tag) loom_string_free(entity_tag);
    return out;
}

static jobjectArray document_put_result(JNIEnv *env, char *digest, char *entity_tag) {
    jclass object_class = (*env)->FindClass(env, "java/lang/Object");
    jobjectArray out = (*env)->NewObjectArray(env, 2, object_class, NULL);
    jstring digest_value = (*env)->NewStringUTF(env, digest ? digest : "");
    jstring entity_tag_value = (*env)->NewStringUTF(env, entity_tag ? entity_tag : "");
    (*env)->SetObjectArrayElement(env, out, 0, digest_value);
    (*env)->SetObjectArrayElement(env, out, 1, entity_tag_value);
    (*env)->DeleteLocalRef(env, digest_value);
    (*env)->DeleteLocalRef(env, entity_tag_value);
    if (digest) loom_string_free(digest);
    if (entity_tag) loom_string_free(entity_tag);
    return out;
}

JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocPutText(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                      jstring collection, jstring id, jstring text,
                                      jstring expected_entity_tag, jbyteArray passphrase, jbyteArray kek,
                                      jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    const char *t = (*env)->GetStringUTFChars(env, text, NULL);
    const char *e = expected_entity_tag ? (*env)->GetStringUTFChars(env, expected_entity_tag, NULL) : NULL;
    char *digest = NULL;
    char *entity_tag = NULL;
    int32_t st = loom_doc_put_text(h, n, m, i, t, e, &digest, &entity_tag);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, id, i);
    (*env)->ReleaseStringUTFChars(env, text, t);
    if (e) (*env)->ReleaseStringUTFChars(env, expected_entity_tag, e);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return document_put_result(env, digest, entity_tag);
}

JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocGetText(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                      jstring collection, jstring id, jbyteArray passphrase,
                                      jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    char *text = NULL;
    char *digest = NULL;
    char *entity_tag = NULL;
    int32_t found = 0;
    int32_t st = loom_doc_get_text(h, n, m, i, &text, &digest, &entity_tag, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return document_text_result(env, text, digest, entity_tag);
}

JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocPutBinary(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                      jstring collection, jstring id, jbyteArray doc,
                                      jstring expected_entity_tag, jbyteArray passphrase, jbyteArray kek,
                                      jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    const char *e = expected_entity_tag ? (*env)->GetStringUTFChars(env, expected_entity_tag, NULL) : NULL;
    jsize dlen = doc ? (*env)->GetArrayLength(env, doc) : 0;
    jbyte *d = doc ? (*env)->GetByteArrayElements(env, doc, NULL) : NULL;
    char *digest = NULL;
    char *entity_tag = NULL;
    int32_t st = loom_doc_put_binary(h, n, m, i, (const unsigned char *)d, (uintptr_t)dlen, e, &digest, &entity_tag);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, id, i);
    if (e) (*env)->ReleaseStringUTFChars(env, expected_entity_tag, e);
    if (d) (*env)->ReleaseByteArrayElements(env, doc, d, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return document_put_result(env, digest, entity_tag);
}

JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocGetBinary(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                      jstring collection, jstring id, jbyteArray passphrase,
                                      jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    char *digest = NULL;
    char *entity_tag = NULL;
    int32_t found = 0;
    int32_t st = loom_doc_get_binary(h, n, m, i, &ptr, &len, &digest, &entity_tag, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return document_binary_result(env, ptr, len, digest, entity_tag);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocDelete(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring collection, jstring id, jbyteArray passphrase,
                                         jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    int32_t found = 0;
    int32_t st = loom_doc_delete(h, n, m, i, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocListBinary(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                       jstring collection, jbyteArray passphrase, jbyteArray kek,
                                       jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_doc_list_binary_cbor(h, n, m, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocIndexCreate(JNIEnv *env, jobject thiz, jstring path,
                                           jstring ns, jstring collection, jstring name,
                                           jstring field_path, jboolean unique,
                                           jbyteArray passphrase, jbyteArray kek,
                                           jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *idx = (*env)->GetStringUTFChars(env, name, NULL);
    const char *p = (*env)->GetStringUTFChars(env, field_path, NULL);
    int32_t st = loom_doc_index_create(h, n, m, idx, p, unique ? 1 : 0);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, name, idx);
    (*env)->ReleaseStringUTFChars(env, field_path, p);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocIndexCreateJson(JNIEnv *env, jobject thiz, jstring path,
                                           jstring ns, jstring collection, jbyteArray declaration_json,
                                           jbyteArray passphrase, jbyteArray kek,
                                           jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    jbyte *d = declaration_json ? (*env)->GetByteArrayElements(env, declaration_json, NULL) : NULL;
    jsize d_len = declaration_json ? (*env)->GetArrayLength(env, declaration_json) : 0;
    int32_t st = loom_doc_index_create_json(h, n, m, (const uint8_t *)d, (uintptr_t)d_len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    if (d) (*env)->ReleaseByteArrayElements(env, declaration_json, d, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocIndexDrop(JNIEnv *env, jobject thiz, jstring path,
                                         jstring ns, jstring collection, jstring name,
                                         jbyteArray passphrase, jbyteArray kek,
                                         jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *idx = (*env)->GetStringUTFChars(env, name, NULL);
    int32_t found = 0;
    int32_t st = loom_doc_index_drop(h, n, m, idx, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, name, idx);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocIndexRebuild(JNIEnv *env, jobject thiz, jstring path,
                                            jstring ns, jstring collection, jstring name,
                                            jbyteArray passphrase, jbyteArray kek,
                                            jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *idx = (*env)->GetStringUTFChars(env, name, NULL);
    int32_t st = loom_doc_index_rebuild(h, n, m, idx);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, name, idx);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocIndexListJson(JNIEnv *env, jobject thiz, jstring path,
                                             jstring ns, jstring collection, jbyteArray passphrase,
                                             jbyteArray kek, jstring auth_principal,
                                             jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_doc_index_list_json(h, n, m, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_json_string(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocIndexStatusJson(JNIEnv *env, jobject thiz, jstring path,
                                               jstring ns, jstring collection, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal,
                                               jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_doc_index_status_json(h, n, m, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_json_string(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocFindJson(JNIEnv *env, jobject thiz, jstring path,
                                        jstring ns, jstring collection, jstring index,
                                        jstring value_json, jbyteArray passphrase, jbyteArray kek,
                                        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *idx = (*env)->GetStringUTFChars(env, index, NULL);
    const char *value = (*env)->GetStringUTFChars(env, value_json, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_doc_find_json(h, n, m, idx, (const unsigned char *)value, (uintptr_t)strlen(value), &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, index, idx);
    (*env)->ReleaseStringUTFChars(env, value_json, value);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_json_string(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDocQueryJson(JNIEnv *env, jobject thiz, jstring path,
                                         jstring ns, jstring collection, jstring query_json,
                                         jbyteArray passphrase, jbyteArray kek,
                                         jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *query = (*env)->GetStringUTFChars(env, query_json, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_doc_query_json(h, n, m, (const unsigned char *)query, (uintptr_t)strlen(query), &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    (*env)->ReleaseStringUTFChars(env, query_json, query);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_json_string(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeTsPut(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                     jstring collection, jlong ts, jbyteArray value,
                                     jbyteArray passphrase, jbyteArray kek,
                                     jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    jsize vlen = value ? (*env)->GetArrayLength(env, value) : 0;
    jbyte *v = value ? (*env)->GetByteArrayElements(env, value, NULL) : NULL;
    int32_t st = loom_ts_put(h, n, m, (int64_t)ts, (const unsigned char *)v, (uintptr_t)vlen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    if (v) (*env)->ReleaseByteArrayElements(env, value, v, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeTsGet(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                     jstring collection, jlong ts, jbyteArray passphrase, jbyteArray kek,
                                     jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_ts_get(h, n, m, (int64_t)ts, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeTsRange(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                       jstring collection, jlong from, jlong to, jbyteArray passphrase,
                                       jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_ts_range_cbor(h, n, m, (int64_t)from, (int64_t)to, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeTsLatest(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                        jstring collection, jlongArray outTs, jbyteArray passphrase,
                                        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    int64_t ts = 0;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_ts_latest(h, n, m, &ts, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    if (outTs) {
        jlong tsj = (jlong)ts;
        (*env)->SetLongArrayRegion(env, outTs, 0, 1, &tsj);
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomNative_nativeLedgerAppend(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring collection, jbyteArray payload, jbyteArray passphrase,
                                            jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return 0;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    jsize plen = payload ? (*env)->GetArrayLength(env, payload) : 0;
    jbyte *p = payload ? (*env)->GetByteArrayElements(env, payload, NULL) : NULL;
    uint64_t seq = 0;
    int32_t st = loom_ledger_append(h, n, m, (const unsigned char *)p, (uintptr_t)plen, &seq);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    if (p) (*env)->ReleaseByteArrayElements(env, payload, p, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)seq;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeLedgerGet(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring collection, jlong seq, jbyteArray passphrase,
                                         jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_ledger_get(h, n, m, (uint64_t)seq, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeLedgerHead(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                          jstring collection, jbyteArray passphrase, jbyteArray kek,
                                          jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    char *out = NULL;
    int32_t found = 0;
    int32_t st = loom_ledger_head(h, n, m, &out, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomNative_nativeLedgerLen(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring collection, jbyteArray passphrase, jbyteArray kek,
                                         jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return 0;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    uint64_t out = 0;
    int32_t st = loom_ledger_len(h, n, m, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)out;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeLedgerVerify(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring collection, jbyteArray passphrase, jbyteArray kek,
                                            jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *m = (*env)->GetStringUTFChars(env, collection, NULL);
    int32_t st = loom_ledger_verify(h, n, m);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, collection, m);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSqlIndexScan(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring table, jstring index, jbyteArray prefix,
                                            jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *t = (*env)->GetStringUTFChars(env, table, NULL);
    const char *i = (*env)->GetStringUTFChars(env, index, NULL);
    jbyte *p = NULL;
    jsize plen = 0;
    if (prefix != NULL) {
        plen = (*env)->GetArrayLength(env, prefix);
        p = (*env)->GetByteArrayElements(env, prefix, NULL);
    }
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_index_scan(h, n, t, i, (const unsigned char *)p, (uintptr_t)plen, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, table, t);
    (*env)->ReleaseStringUTFChars(env, index, i);
    if (p) (*env)->ReleaseByteArrayElements(env, prefix, p, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSqlIndexScanAt(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring table, jstring index, jbyteArray prefix,
                                              jstring commit, jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *t = (*env)->GetStringUTFChars(env, table, NULL);
    const char *i = (*env)->GetStringUTFChars(env, index, NULL);
    const char *c = (*env)->GetStringUTFChars(env, commit, NULL);
    jbyte *p = NULL;
    jsize plen = 0;
    if (prefix != NULL) {
        plen = (*env)->GetArrayLength(env, prefix);
        p = (*env)->GetByteArrayElements(env, prefix, NULL);
    }
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_index_scan_at(h, n, t, i, (const unsigned char *)p, (uintptr_t)plen, c, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, table, t);
    (*env)->ReleaseStringUTFChars(env, index, i);
    (*env)->ReleaseStringUTFChars(env, commit, c);
    if (p) (*env)->ReleaseByteArrayElements(env, prefix, p, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSqlBlame(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                        jstring branch, jstring table, jbyteArray passphrase,
                                        jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *b = (*env)->GetStringUTFChars(env, branch, NULL);
    const char *t = (*env)->GetStringUTFChars(env, table, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_blame(h, n, b, t, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, branch, b);
    (*env)->ReleaseStringUTFChars(env, table, t);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVcsBlame(JNIEnv *env, jobject thiz, jstring path,
                                        jstring ns, jstring branch, jbyteArray passphrase,
                                        jbyteArray kek, jstring auth_principal,
                                        jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *b = (*env)->GetStringUTFChars(env, branch, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_vcs_blame(h, n, b, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, branch, b);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVcsDiff(JNIEnv *env, jobject thiz, jstring path,
                                       jstring ns, jstring fromCommit, jstring toCommit,
                                       jbyteArray passphrase, jbyteArray kek,
                                       jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *from = (*env)->GetStringUTFChars(env, fromCommit, NULL);
    const char *to = (*env)->GetStringUTFChars(env, toCommit, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_vcs_diff(h, n, from, to, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, fromCommit, from);
    (*env)->ReleaseStringUTFChars(env, toCommit, to);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeWatchSubscribe(
        JNIEnv *env, jobject thiz, jstring path, jstring ns, jstring branch,
        jstring facet, jstring pathPrefix, jstring changeKinds, jstring fromCommit,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *b = (*env)->GetStringUTFChars(env, branch, NULL);
    const char *f = optional_utf(env, facet);
    const char *prefix = optional_utf(env, pathPrefix);
    const char *kinds = optional_utf(env, changeKinds);
    const char *from = optional_utf(env, fromCommit);
    char *out = NULL;
    int32_t st = loom_watch_subscribe(h, n, b, f, prefix, kinds, from, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, branch, b);
    release_optional_utf(env, facet, f);
    release_optional_utf(env, pathPrefix, prefix);
    release_optional_utf(env, changeKinds, kinds);
    release_optional_utf(env, fromCommit, from);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeWatchPoll(
        JNIEnv *env, jobject thiz, jstring path, jstring cursor, jint max,
        jbyteArray passphrase, jbyteArray kek, jstring auth_principal,
        jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *c = (*env)->GetStringUTFChars(env, cursor, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_watch_poll(h, c, (uint32_t)max, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, cursor, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSqlDiff(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                       jstring table, jstring fromCommit, jstring toCommit,
                                       jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *t = (*env)->GetStringUTFChars(env, table, NULL);
    const char *from = (*env)->GetStringUTFChars(env, fromCommit, NULL);
    const char *to = (*env)->GetStringUTFChars(env, toCommit, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_diff(h, n, t, from, to, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, table, t);
    (*env)->ReleaseStringUTFChars(env, fromCommit, from);
    (*env)->ReleaseStringUTFChars(env, toCommit, to);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSqlTableDiff(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring table, jstring fromCommit, jstring toCommit,
                                            jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *t = (*env)->GetStringUTFChars(env, table, NULL);
    const char *from = (*env)->GetStringUTFChars(env, fromCommit, NULL);
    const char *to = (*env)->GetStringUTFChars(env, toCommit, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_table_diff(h, n, t, from, to, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, table, t);
    (*env)->ReleaseStringUTFChars(env, fromCommit, from);
    (*env)->ReleaseStringUTFChars(env, toCommit, to);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomNative_nativeQueueAppend(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jstring stream, jbyteArray entry, jbyteArray passphrase,
                                           jbyteArray kek, jstring auth_principal,
                                           jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return 0;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *s = (*env)->GetStringUTFChars(env, stream, NULL);
    jbyte *e = NULL;
    jsize elen = 0;
    if (entry != NULL) {
        elen = (*env)->GetArrayLength(env, entry);
        e = (*env)->GetByteArrayElements(env, entry, NULL);
    }
    uint64_t seq = 0;
    int32_t st = loom_queue_append(h, n, s, (const unsigned char *)e, (uintptr_t)elen, &seq);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, stream, s);
    if (e) (*env)->ReleaseByteArrayElements(env, entry, e, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)seq;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeQueueGet(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                        jstring stream, jlong seq, jbyteArray passphrase,
                                        jbyteArray kek, jstring auth_principal,
                                        jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *s = (*env)->GetStringUTFChars(env, stream, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_queue_get(h, n, s, (uint64_t)seq, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, stream, s);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) {
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeQueueRange(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                          jstring stream, jlong lo, jlong hi, jbyteArray passphrase,
                                          jbyteArray kek, jstring auth_principal,
                                          jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *s = (*env)->GetStringUTFChars(env, stream, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_queue_range(h, n, s, (uint64_t)lo, (uint64_t)hi, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, stream, s);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomNative_nativeQueueLen(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                        jstring stream, jbyteArray passphrase, jbyteArray kek,
                                        jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return 0;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *s = (*env)->GetStringUTFChars(env, stream, NULL);
    uint64_t len = 0;
    int32_t st = loom_queue_len(h, n, s, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, stream, s);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)len;
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomNative_nativeQueueConsumerPosition(JNIEnv *env, jobject thiz, jstring path,
                                                     jstring ns, jstring stream, jstring consumerId,
                                                     jbyteArray passphrase, jbyteArray kek,
                                                     jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return 0;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *s = (*env)->GetStringUTFChars(env, stream, NULL);
    const char *c = (*env)->GetStringUTFChars(env, consumerId, NULL);
    uint64_t seq = 0;
    int32_t st = loom_queue_consumer_position(h, n, s, c, &seq);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, stream, s);
    (*env)->ReleaseStringUTFChars(env, consumerId, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)seq;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeQueueConsumerRead(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                 jstring stream, jstring consumerId, jint max,
                                                 jbyteArray passphrase, jbyteArray kek,
                                                 jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *s = (*env)->GetStringUTFChars(env, stream, NULL);
    const char *c = (*env)->GetStringUTFChars(env, consumerId, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_queue_consumer_read(h, n, s, c, (uint32_t)max, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, stream, s);
    (*env)->ReleaseStringUTFChars(env, consumerId, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeQueueConsumerAdvance(JNIEnv *env, jobject thiz, jstring path,
                                                    jstring ns, jstring stream, jstring consumerId,
                                                    jlong nextSeq, jbyteArray passphrase,
                                                    jbyteArray kek, jstring auth_principal,
                                                    jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *s = (*env)->GetStringUTFChars(env, stream, NULL);
    const char *c = (*env)->GetStringUTFChars(env, consumerId, NULL);
    int32_t st = loom_queue_consumer_advance(h, n, s, c, (uint64_t)nextSeq);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, stream, s);
    (*env)->ReleaseStringUTFChars(env, consumerId, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeQueueConsumerReset(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                  jstring stream, jstring consumerId, jlong nextSeq,
                                                  jbyteArray passphrase, jbyteArray kek,
                                                  jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *s = (*env)->GetStringUTFChars(env, stream, NULL);
    const char *c = (*env)->GetStringUTFChars(env, consumerId, NULL);
    int32_t st = loom_queue_consumer_reset(h, n, s, c, (uint64_t)nextSeq);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, stream, s);
    (*env)->ReleaseStringUTFChars(env, consumerId, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
    }
}

/* --- Calendar facade (CalDAV collections + entries). Each call opens the loom for the op and closes. --- */

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalCreateCollection(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring ns, jstring principal, jstring collection,
                                                   jstring displayName, jstring components,
                                                   jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *dn = (*env)->GetStringUTFChars(env, displayName, NULL);
    const char *cmp = (*env)->GetStringUTFChars(env, components, NULL);
    int32_t st = loom_cal_create_collection(h, n, pr, col, dn, cmp);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    (*env)->ReleaseStringUTFChars(env, displayName, dn);
    (*env)->ReleaseStringUTFChars(env, components, cmp);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalDeleteCollection(JNIEnv *env, jobject thiz, jstring path,
                                                   jstring ns, jstring principal, jstring collection,
                                                   jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    int32_t found = 0;
    int32_t st = loom_cal_delete_collection(h, n, pr, col, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalListCollections(JNIEnv *env, jobject thiz, jstring path,
                                                  jstring ns, jstring principal,
                                                  jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_cal_list_collections(h, n, pr, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalPutEntry(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jstring principal, jstring collection, jbyteArray entry,
                                           jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    jsize elen = entry ? (*env)->GetArrayLength(env, entry) : 0;
    jbyte *e = entry ? (*env)->GetByteArrayElements(env, entry, NULL) : NULL;
    int32_t st = loom_cal_put_entry(h, n, pr, col, (const unsigned char *)e, (uintptr_t)elen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    if (e) (*env)->ReleaseByteArrayElements(env, entry, e, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalGetEntry(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jstring principal, jstring collection, jstring uid,
                                           jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_cal_get_entry(h, n, pr, col, u, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalDeleteEntry(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring principal, jstring collection, jstring uid,
                                              jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    int32_t found = 0;
    int32_t st = loom_cal_delete_entry(h, n, pr, col, u, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalListEntries(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring principal, jstring collection,
                                              jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_cal_list_entries(h, n, pr, col, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalRange(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                        jstring principal, jstring collection, jstring from,
                                        jstring to, jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *fr = (*env)->GetStringUTFChars(env, from, NULL);
    const char *t = (*env)->GetStringUTFChars(env, to, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_cal_range(h, n, pr, col, fr, t, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    (*env)->ReleaseStringUTFChars(env, from, fr);
    (*env)->ReleaseStringUTFChars(env, to, t);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalSearch(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring principal, jstring collection, jstring component,
                                         jstring text, jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *cmp = (*env)->GetStringUTFChars(env, component, NULL);
    const char *tx = (*env)->GetStringUTFChars(env, text, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_cal_search(h, n, pr, col, cmp, tx, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    (*env)->ReleaseStringUTFChars(env, component, cmp);
    (*env)->ReleaseStringUTFChars(env, text, tx);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalEntryIcs(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jstring principal, jstring collection, jstring uid,
                                           jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    char *out = NULL;
    int32_t found = 0;
    int32_t st = loom_cal_entry_ics(h, n, pr, col, u, &out, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeCalPutIcs(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring principal, jstring collection, jstring ics,
                                         jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *col = (*env)->GetStringUTFChars(env, collection, NULL);
    const char *ic = (*env)->GetStringUTFChars(env, ics, NULL);
    char *out = NULL;
    int32_t st = loom_cal_put_ics(h, n, pr, col, ic, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, collection, col);
    (*env)->ReleaseStringUTFChars(env, ics, ic);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

/* --- Contacts facade (CardDAV address books + contacts). Each call opens the loom for the op and closes. --- */

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardCreateBook(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring principal, jstring book, jstring displayName,
                                              jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    const char *dn = (*env)->GetStringUTFChars(env, displayName, NULL);
    int32_t st = loom_card_create_book(h, n, pr, bk, dn);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    (*env)->ReleaseStringUTFChars(env, displayName, dn);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardDeleteBook(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring principal, jstring book, jbyteArray passphrase,
                                              jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    int32_t found = 0;
    int32_t st = loom_card_delete_book(h, n, pr, bk, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardListBooks(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                             jstring principal, jbyteArray passphrase,
                                             jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_card_list_books(h, n, pr, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardPutEntry(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring principal, jstring book, jbyteArray entry,
                                            jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    jsize elen = entry ? (*env)->GetArrayLength(env, entry) : 0;
    jbyte *e = entry ? (*env)->GetByteArrayElements(env, entry, NULL) : NULL;
    int32_t st = loom_card_put_entry(h, n, pr, bk, (const unsigned char *)e, (uintptr_t)elen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    if (e) (*env)->ReleaseByteArrayElements(env, entry, e, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardGetEntry(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring principal, jstring book, jstring uid,
                                            jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_card_get_entry(h, n, pr, bk, u, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardDeleteEntry(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring principal, jstring book, jstring uid,
                                               jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    int32_t found = 0;
    int32_t st = loom_card_delete_entry(h, n, pr, bk, u, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardListEntries(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring principal, jstring book, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_card_list_entries(h, n, pr, bk, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardSearch(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                          jstring principal, jstring book, jstring text,
                                          jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    const char *tx = (*env)->GetStringUTFChars(env, text, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_card_search(h, n, pr, bk, tx, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    (*env)->ReleaseStringUTFChars(env, text, tx);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardEntryVcard(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring principal, jstring book, jstring uid,
                                              jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    char *out = NULL;
    int32_t found = 0;
    int32_t st = loom_card_entry_vcard(h, n, pr, bk, u, &out, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeCardPutVcard(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring principal, jstring book, jstring vcf,
                                            jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *bk = (*env)->GetStringUTFChars(env, book, NULL);
    const char *vc = (*env)->GetStringUTFChars(env, vcf, NULL);
    char *out = NULL;
    int32_t st = loom_card_put_vcard(h, n, pr, bk, vc, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, book, bk);
    (*env)->ReleaseStringUTFChars(env, vcf, vc);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

/* --- Mail facade (JMAP-style mailboxes + messages). Each call opens the loom for the op and closes. --- */

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailCreateMailbox(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                 jstring principal, jstring mailbox,
                                                 jstring displayName, jbyteArray passphrase,
                                                 jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    const char *dn = (*env)->GetStringUTFChars(env, displayName, NULL);
    int32_t st = loom_mail_create_mailbox(h, n, pr, mb, dn);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    (*env)->ReleaseStringUTFChars(env, displayName, dn);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailDeleteMailbox(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                 jstring principal, jstring mailbox,
                                                 jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    int32_t found = 0;
    int32_t st = loom_mail_delete_mailbox(h, n, pr, mb, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailListMailboxes(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                 jstring principal, jbyteArray passphrase,
                                                 jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_mail_list_mailboxes(h, n, pr, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailIngestMessage(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                 jstring principal, jstring mailbox, jstring uid,
                                                 jbyteArray raw, jbyteArray passphrase,
                                                 jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    jsize rlen = raw ? (*env)->GetArrayLength(env, raw) : 0;
    jbyte *r = raw ? (*env)->GetByteArrayElements(env, raw, NULL) : NULL;
    char *out = NULL;
    int32_t st = loom_mail_ingest_message(h, n, pr, mb, u, (const unsigned char *)r, (uintptr_t)rlen,
                                          &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    if (r) (*env)->ReleaseByteArrayElements(env, raw, r, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring res = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return res;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailGetMessage(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring principal, jstring mailbox, jstring uid,
                                              jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_mail_get_message(h, n, pr, mb, u, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailToEml(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jstring principal, jstring mailbox, jstring uid,
                                           jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_mail_to_eml(h, n, pr, mb, u, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailDeleteMessage(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                 jstring principal, jstring mailbox, jstring uid,
                                                 jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    int32_t found = 0;
    int32_t st = loom_mail_delete_message(h, n, pr, mb, u, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailListMessages(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                jstring principal, jstring mailbox,
                                                jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_mail_list_messages(h, n, pr, mb, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailGetFlags(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring principal, jstring mailbox, jstring uid,
                                            jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_mail_get_flags(h, n, pr, mb, u, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailSetFlags(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring principal, jstring mailbox, jstring uid,
                                            jbyteArray flags, jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    const char *u = (*env)->GetStringUTFChars(env, uid, NULL);
    jsize flen = flags ? (*env)->GetArrayLength(env, flags) : 0;
    jbyte *fl = flags ? (*env)->GetByteArrayElements(env, flags, NULL) : NULL;
    int32_t st = loom_mail_set_flags(h, n, pr, mb, u, (const unsigned char *)fl, (uintptr_t)flen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    (*env)->ReleaseStringUTFChars(env, uid, u);
    if (fl) (*env)->ReleaseByteArrayElements(env, flags, fl, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeMailSearch(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                          jstring principal, jstring mailbox, jstring text,
                                          jbyteArray passphrase, jbyteArray kek, jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *pr = (*env)->GetStringUTFChars(env, principal, NULL);
    const char *mb = (*env)->GetStringUTFChars(env, mailbox, NULL);
    const char *tx = (*env)->GetStringUTFChars(env, text, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_mail_search(h, n, pr, mb, tx, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, principal, pr);
    (*env)->ReleaseStringUTFChars(env, mailbox, mb);
    (*env)->ReleaseStringUTFChars(env, text, tx);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSql_nativeOpen(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                       jstring db) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, db, NULL);
    LoomSqlSession *s = NULL;
    int32_t st = loom_sql_open(p, n, d, &s);
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, db, d);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)s;
}

/* Open a session over an encrypted loom, unlocking it with the passphrase bytes.
 * `passphrase` may be null (behaves like nativeOpen). */
JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSql_nativeOpenKeyed(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring db, jbyteArray passphrase) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, db, NULL);
    jbyte *pass = NULL;
    jsize plen = 0;
    if (passphrase != NULL) {
        plen = (*env)->GetArrayLength(env, passphrase);
        pass = (*env)->GetByteArrayElements(env, passphrase, NULL);
    }
    LoomSqlSession *s = NULL;
    int32_t st = loom_sql_open_keyed(p, n, d, (const unsigned char *)pass, (uintptr_t)plen, &s);
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, db, d);
    if (pass) (*env)->ReleaseByteArrayElements(env, passphrase, pass, JNI_ABORT);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)s;
}

/* Open a session over an encrypted loom with a host-supplied 256-bit KEK. `kek`
 * must be 32 bytes. */
JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSql_nativeOpenWithKek(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring db, jbyteArray kek) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, db, NULL);
    jbyte *k = NULL;
    jsize klen = 0;
    if (kek != NULL) {
        klen = (*env)->GetArrayLength(env, kek);
        k = (*env)->GetByteArrayElements(env, kek, NULL);
    }
    LoomSqlSession *s = NULL;
    int32_t st = loom_sql_open_with_kek(p, n, d, (const unsigned char *)k, (uintptr_t)klen, &s);
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, db, d);
    if (k) (*env)->ReleaseByteArrayElements(env, kek, k, JNI_ABORT);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)s;
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSql_nativeOpenAuthenticated(JNIEnv *env, jobject thiz, jstring path,
                                                    jstring ns, jstring db, jbyteArray passphrase,
                                                    jbyteArray kek, jstring auth_principal,
                                                    jbyteArray auth_passphrase) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, db, NULL);
    const char *ap = auth_principal ? (*env)->GetStringUTFChars(env, auth_principal, NULL) : NULL;
    jbyte *pass = NULL;
    jsize plen = 0;
    if (passphrase != NULL) {
        plen = (*env)->GetArrayLength(env, passphrase);
        pass = (*env)->GetByteArrayElements(env, passphrase, NULL);
    }
    jbyte *k = NULL;
    jsize klen = 0;
    if (kek != NULL) {
        klen = (*env)->GetArrayLength(env, kek);
        k = (*env)->GetByteArrayElements(env, kek, NULL);
    }
    jbyte *auth = NULL;
    jsize auth_len = 0;
    if (auth_passphrase != NULL) {
        auth_len = (*env)->GetArrayLength(env, auth_passphrase);
        auth = (*env)->GetByteArrayElements(env, auth_passphrase, NULL);
    }
    LoomSqlSession *s = NULL;
    int32_t st;
    if (ap == NULL || auth == NULL || auth_len == 0) {
        st = loom_sql_open(p, n, d, &s);
    } else if (k != NULL) {
        st = loom_sql_open_with_kek_authenticated(p, n, d, (const unsigned char *)k,
                                                  (uintptr_t)klen, ap,
                                                  (const unsigned char *)auth,
                                                  (uintptr_t)auth_len, &s);
    } else if (pass != NULL) {
        st = loom_sql_open_keyed_authenticated(p, n, d, (const unsigned char *)pass,
                                               (uintptr_t)plen, ap,
                                               (const unsigned char *)auth,
                                               (uintptr_t)auth_len, &s);
    } else {
        st = loom_sql_open_authenticated(p, n, d, ap, (const unsigned char *)auth,
                                         (uintptr_t)auth_len, &s);
    }
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, db, d);
    if (ap) (*env)->ReleaseStringUTFChars(env, auth_principal, ap);
    if (pass) (*env)->ReleaseByteArrayElements(env, passphrase, pass, JNI_ABORT);
    if (k) (*env)->ReleaseByteArrayElements(env, kek, k, JNI_ABORT);
    if (auth) (*env)->ReleaseByteArrayElements(env, auth_passphrase, auth, JNI_ABORT);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)s;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomSql_nativeExec(JNIEnv *env, jobject thiz, jlong handle, jstring sql) {
    (void)thiz;
    LoomSqlSession *s = (LoomSqlSession *)(intptr_t)handle;
    const char *q = (*env)->GetStringUTFChars(env, sql, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_exec(s, q, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, sql, q);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    /* Render the canonical-CBOR result to JSON (debug form) for the string-returning API. */
    char *json = NULL;
    int32_t rst = loom_result_to_json(ptr, len, &json);
    loom_bytes_free(ptr, len);
    if (rst != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, json ? json : "");
    if (json) loom_string_free(json);
    return r;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomSql_nativeExecBytes(JNIEnv *env, jobject thiz, jlong handle, jstring sql) {
    (void)thiz;
    LoomSqlSession *s = (LoomSqlSession *)(intptr_t)handle;
    const char *q = (*env)->GetStringUTFChars(env, sql, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_exec(s, q, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, sql, q);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    /* Return the canonical-CBOR result payload as a Java byte[]. */
    jbyteArray arr = (*env)->NewByteArray(env, (jsize)len);
    if (arr != NULL && len > 0) {
        (*env)->SetByteArrayRegion(env, arr, 0, (jsize)len, (const jbyte *)ptr);
    }
    loom_bytes_free(ptr, len);
    return arr;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomSql_nativeCommit(JNIEnv *env, jobject thiz, jlong handle, jstring message,
                                         jstring author) {
    (void)thiz;
    LoomSqlSession *s = (LoomSqlSession *)(intptr_t)handle;
    const char *m = (*env)->GetStringUTFChars(env, message, NULL);
    const char *a = (*env)->GetStringUTFChars(env, author, NULL);
    char *out = NULL;
    int32_t st = loom_sql_commit(s, m, a, &out);
    (*env)->ReleaseStringUTFChars(env, message, m);
    (*env)->ReleaseStringUTFChars(env, author, a);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomSql_nativeClose(JNIEnv *env, jobject thiz, jlong handle) {
    (void)env;
    (void)thiz;
    loom_sql_close((LoomSqlSession *)(intptr_t)handle);
}

/* --- Typed result view (the typed `exec` path; one shared decoder behind the C ABI). --- */

/* Run SQL, decode the canonical result into an owned result-view, and return it as a handle. */
JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSql_nativeResultOpen(JNIEnv *env, jobject thiz, jlong handle, jstring sql) {
    (void)thiz;
    LoomSqlSession *s = (LoomSqlSession *)(intptr_t)handle;
    const char *q = (*env)->GetStringUTFChars(env, sql, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_exec(s, q, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, sql, q);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    LoomResultView *view = NULL;
    int32_t op = loom_result_open(ptr, len, &view);
    loom_bytes_free(ptr, len); /* result_open decodes into an owned view; the bytes are done. */
    if (op != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)view;
}

/* --- Streaming iterator: query -> next-row -> free. --- */

/* Run SQL and open a streaming row iterator; returns the LoomIter* as a handle (0 on error + throw). */
JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSql_nativeQueryOpen(JNIEnv *env, jobject thiz, jlong handle, jstring sql) {
    (void)thiz;
    LoomSqlSession *s = (LoomSqlSession *)(intptr_t)handle;
    const char *q = (*env)->GetStringUTFChars(env, sql, NULL);
    LoomIter *it = NULL;
    int32_t st = loom_sql_query(s, q, &it);
    (*env)->ReleaseStringUTFChars(env, sql, q);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)it;
}

/* Advance the iterator and decode the next row into a one-row LoomResultView, returned as a handle;
 * 0 means the stream is exhausted (no exception) or an error occurred (exception pending). */
JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomRowStream_nativeIterNextRow(JNIEnv *env, jobject thiz, jlong iter) {
    (void)thiz;
    LoomIter *it = (LoomIter *)(intptr_t)iter;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t done = 0;
    if (loom_iter_next(it, &ptr, &len, &done) != 0) {
        throw_loom(env);
        return 0;
    }
    if (done != 0) {
        return 0; /* end of stream */
    }
    LoomResultView *view = NULL;
    int32_t op = loom_row_open(ptr, len, &view);
    loom_bytes_free(ptr, len);
    if (op != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)view;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomRowStream_nativeIterFree(JNIEnv *env, jobject thiz, jlong iter) {
    (void)env;
    (void)thiz;
    loom_iter_free((LoomIter *)(intptr_t)iter);
}

/* Build a Kotlin `ai.uldren.loom.LoomCell` from a decoded LoomValue (byte fields copied out). */
static jobject make_cell(JNIEnv *env, const LoomValue *v) {
    jclass cls = (*env)->FindClass(env, "ai/uldren/loom/LoomCell");
    if (cls == NULL) {
        return NULL;
    }
    jmethodID ctor = (*env)->GetMethodID(env, cls, "<init>", "(IIJJJDDJJ[B[B)V");
    if (ctor == NULL) {
        return NULL;
    }
    jbyteArray b16 = (*env)->NewByteArray(env, 16);
    if (b16 != NULL) {
        (*env)->SetByteArrayRegion(env, b16, 0, 16, (const jbyte *)v->bytes16);
    }
    jbyteArray data = NULL;
    if (v->data != NULL && v->data_len > 0) {
        data = (*env)->NewByteArray(env, (jsize)v->data_len);
        if (data != NULL) {
            (*env)->SetByteArrayRegion(env, data, 0, (jsize)v->data_len, (const jbyte *)v->data);
        }
    }
    return (*env)->NewObject(env, cls, ctor, (jint)v->tag, (jint)v->scale, (jlong)v->int_val,
                            (jlong)v->int_val2, (jlong)v->uint_val, (jdouble)v->float_val,
                            (jdouble)v->float_val2, (jlong)v->bits, (jlong)v->bits2, b16, data);
}

/* Copy a borrowed (ptr, len) into a Java byte[] (UTF-8 text or commit address; Kotlin decodes). */
static jbyteArray borrowed_bytes(JNIEnv *env, const unsigned char *ptr, uintptr_t len) {
    jbyteArray arr = (*env)->NewByteArray(env, (jsize)len);
    if (arr != NULL && len > 0) {
        (*env)->SetByteArrayRegion(env, arr, 0, (jsize)len, (const jbyte *)ptr);
    }
    return arr;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultClose(JNIEnv *env, jobject thiz, jlong view) {
    (void)env;
    (void)thiz;
    loom_result_close((LoomResultView *)(intptr_t)view);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultLen(JNIEnv *env, jobject thiz, jlong view) {
    (void)env;
    (void)thiz;
    return (jlong)loom_result_len((LoomResultView *)(intptr_t)view);
}

JNIEXPORT jint JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultItemKind(JNIEnv *env, jobject thiz, jlong view,
                                                    jlong item) {
    (void)env;
    (void)thiz;
    return loom_result_item_kind((LoomResultView *)(intptr_t)view, (uintptr_t)item);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultColumnCount(JNIEnv *env, jobject thiz, jlong view,
                                                       jlong item) {
    (void)env;
    (void)thiz;
    return (jlong)loom_result_column_count((LoomResultView *)(intptr_t)view, (uintptr_t)item);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultColumnName(JNIEnv *env, jobject thiz, jlong view,
                                                      jlong item, jlong col) {
    (void)thiz;
    const unsigned char *p = NULL;
    uintptr_t l = 0;
    if (loom_result_column_name((LoomResultView *)(intptr_t)view, (uintptr_t)item, (uintptr_t)col, &p,
                                &l) != 0) {
        throw_loom(env);
        return NULL;
    }
    return borrowed_bytes(env, p, l);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultColumnType(JNIEnv *env, jobject thiz, jlong view,
                                                      jlong item, jlong col) {
    (void)thiz;
    const unsigned char *p = NULL;
    uintptr_t l = 0;
    if (loom_result_column_type((LoomResultView *)(intptr_t)view, (uintptr_t)item, (uintptr_t)col, &p,
                                &l) != 0) {
        throw_loom(env);
        return NULL;
    }
    return borrowed_bytes(env, p, l);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultRowCount(JNIEnv *env, jobject thiz, jlong view,
                                                    jlong item) {
    (void)env;
    (void)thiz;
    return (jlong)loom_result_row_count((LoomResultView *)(intptr_t)view, (uintptr_t)item);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultRowLen(JNIEnv *env, jobject thiz, jlong view, jlong item,
                                                  jlong row) {
    (void)env;
    (void)thiz;
    return (jlong)loom_result_row_len((LoomResultView *)(intptr_t)view, (uintptr_t)item,
                                      (uintptr_t)row);
}

JNIEXPORT jobject JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultCell(JNIEnv *env, jobject thiz, jlong view, jlong item,
                                                jlong row, jlong col) {
    (void)thiz;
    LoomValue v;
    if (loom_result_cell((LoomResultView *)(intptr_t)view, (uintptr_t)item, (uintptr_t)row,
                         (uintptr_t)col, &v) != 0) {
        throw_loom(env);
        return NULL;
    }
    return make_cell(env, &v);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultCount(JNIEnv *env, jobject thiz, jlong view, jlong item) {
    (void)thiz;
    uint64_t n = 0;
    if (loom_result_count((LoomResultView *)(intptr_t)view, (uintptr_t)item, &n) != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)n;
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultStringCount(JNIEnv *env, jobject thiz, jlong view,
                                                       jlong item) {
    (void)env;
    (void)thiz;
    return (jlong)loom_result_string_count((LoomResultView *)(intptr_t)view, (uintptr_t)item);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultString(JNIEnv *env, jobject thiz, jlong view, jlong item,
                                                  jlong index) {
    (void)thiz;
    const unsigned char *p = NULL;
    uintptr_t l = 0;
    if (loom_result_string((LoomResultView *)(intptr_t)view, (uintptr_t)item, (uintptr_t)index, &p,
                           &l) != 0) {
        throw_loom(env);
        return NULL;
    }
    return borrowed_bytes(env, p, l);
}

JNIEXPORT jint JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultVariableKind(JNIEnv *env, jobject thiz, jlong view,
                                                        jlong item) {
    (void)thiz;
    int32_t k = 0;
    if (loom_result_variable_kind((LoomResultView *)(intptr_t)view, (uintptr_t)item, &k) != 0) {
        throw_loom(env);
        return -1;
    }
    return k;
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultMapLen(JNIEnv *env, jobject thiz, jlong view, jlong item,
                                                  jlong row) {
    (void)env;
    (void)thiz;
    return (jlong)loom_result_map_len((LoomResultView *)(intptr_t)view, (uintptr_t)item,
                                      (uintptr_t)row);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultMapKey(JNIEnv *env, jobject thiz, jlong view, jlong item,
                                                  jlong row, jlong idx) {
    (void)thiz;
    const unsigned char *p = NULL;
    uintptr_t l = 0;
    LoomValue v;
    if (loom_result_map_entry((LoomResultView *)(intptr_t)view, (uintptr_t)item, (uintptr_t)row,
                              (uintptr_t)idx, &p, &l, &v) != 0) {
        throw_loom(env);
        return NULL;
    }
    return borrowed_bytes(env, p, l);
}

JNIEXPORT jobject JNICALL
Java_ai_uldren_loom_LoomResult_nativeResultMapValue(JNIEnv *env, jobject thiz, jlong view,
                                                    jlong item, jlong row, jlong idx) {
    (void)thiz;
    const unsigned char *p = NULL;
    uintptr_t l = 0;
    LoomValue v;
    if (loom_result_map_entry((LoomResultView *)(intptr_t)view, (uintptr_t)item, (uintptr_t)row,
                              (uintptr_t)idx, &p, &l, &v) != 0) {
        throw_loom(env);
        return NULL;
    }
    return make_cell(env, &v);
}

/* --- Transaction/batch scope (the held-open writer). --- */

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeBegin(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                             jstring db) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, db, NULL);
    LoomSqlBatch *b = NULL;
    int32_t st = loom_sql_batch_begin(p, n, d, &b);
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, db, d);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)b;
}

/* Begin a batch over an encrypted loom, unlocking it with the passphrase bytes.
 * `passphrase` may be null (behaves like nativeBegin). */
JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeBeginKeyed(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                  jstring db, jbyteArray passphrase) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, db, NULL);
    jbyte *pass = NULL;
    jsize plen = 0;
    if (passphrase != NULL) {
        plen = (*env)->GetArrayLength(env, passphrase);
        pass = (*env)->GetByteArrayElements(env, passphrase, NULL);
    }
    LoomSqlBatch *b = NULL;
    int32_t st = loom_sql_batch_begin_keyed(p, n, d, (const unsigned char *)pass, (uintptr_t)plen, &b);
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, db, d);
    if (pass) (*env)->ReleaseByteArrayElements(env, passphrase, pass, JNI_ABORT);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)b;
}

/* Begin a batch over an encrypted loom with a host-supplied 256-bit KEK. `kek`
 * must be 32 bytes. */
JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeBeginWithKek(JNIEnv *env, jobject thiz, jstring path,
                                                    jstring ns, jstring db, jbyteArray kek) {
    (void)thiz;
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *d = (*env)->GetStringUTFChars(env, db, NULL);
    jbyte *k = NULL;
    jsize klen = 0;
    if (kek != NULL) {
        klen = (*env)->GetArrayLength(env, kek);
        k = (*env)->GetByteArrayElements(env, kek, NULL);
    }
    LoomSqlBatch *b = NULL;
    int32_t st = loom_sql_batch_begin_with_kek(p, n, d, (const unsigned char *)k, (uintptr_t)klen, &b);
    (*env)->ReleaseStringUTFChars(env, path, p);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, db, d);
    if (k) (*env)->ReleaseByteArrayElements(env, kek, k, JNI_ABORT);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)b;
}

/* Run SQL in the batch, decode the canonical result into an owned result-view, return it as a handle. */
JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeResultOpen(JNIEnv *env, jobject thiz, jlong handle,
                                                  jstring sql) {
    (void)thiz;
    LoomSqlBatch *b = (LoomSqlBatch *)(intptr_t)handle;
    const char *q = (*env)->GetStringUTFChars(env, sql, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_batch_exec(b, q, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, sql, q);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    LoomResultView *view = NULL;
    int32_t op = loom_result_open(ptr, len, &view);
    loom_bytes_free(ptr, len);
    if (op != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)(intptr_t)view;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeExecBytes(JNIEnv *env, jobject thiz, jlong handle,
                                                 jstring sql) {
    (void)thiz;
    LoomSqlBatch *b = (LoomSqlBatch *)(intptr_t)handle;
    const char *q = (*env)->GetStringUTFChars(env, sql, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_sql_batch_exec(b, q, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, sql, q);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jbyteArray arr = (*env)->NewByteArray(env, (jsize)len);
    if (arr != NULL && len > 0) {
        (*env)->SetByteArrayRegion(env, arr, 0, (jsize)len, (const jbyte *)ptr);
    }
    loom_bytes_free(ptr, len);
    return arr;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeCommit(JNIEnv *env, jobject thiz, jlong handle) {
    (void)thiz;
    if (loom_sql_batch_commit((LoomSqlBatch *)(intptr_t)handle) != 0) {
        throw_loom(env);
    }
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeCommitVcs(JNIEnv *env, jobject thiz, jlong handle,
                                                 jstring message, jstring author) {
    (void)thiz;
    LoomSqlBatch *b = (LoomSqlBatch *)(intptr_t)handle;
    const char *m = (*env)->GetStringUTFChars(env, message, NULL);
    const char *a = (*env)->GetStringUTFChars(env, author, NULL);
    char *out = NULL;
    int32_t st = loom_sql_batch_commit_vcs(b, m, a, &out);
    (*env)->ReleaseStringUTFChars(env, message, m);
    (*env)->ReleaseStringUTFChars(env, author, a);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeAbort(JNIEnv *env, jobject thiz, jlong handle) {
    (void)thiz;
    if (loom_sql_batch_abort((LoomSqlBatch *)(intptr_t)handle) != 0) {
        throw_loom(env);
    }
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomSqlBatch_nativeClose(JNIEnv *env, jobject thiz, jlong handle) {
    (void)env;
    (void)thiz;
    loom_sql_batch_close((LoomSqlBatch *)(intptr_t)handle);
}

/* --- Graph facade (property graph nodes/edges + traversal). Each call opens the loom for the op and closes. --- */

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphUpsertNode(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring name, jstring id, jbyteArray props,
                                               jbyteArray passphrase, jbyteArray kek,
                                               jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    jsize plen = props ? (*env)->GetArrayLength(env, props) : 0;
    jbyte *p = props ? (*env)->GetByteArrayElements(env, props, NULL) : NULL;
    int32_t st = loom_graph_upsert_node(h, n, g, i, (const unsigned char *)p, (uintptr_t)plen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    if (p) (*env)->ReleaseByteArrayElements(env, props, p, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphGetNode(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jstring id, jbyteArray passphrase,
                                            jbyteArray kek, jstring auth_principal,
                                            jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_graph_get_node(h, n, g, i, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphRemoveNode(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring name, jstring id, jboolean cascade,
                                               jbyteArray passphrase, jbyteArray kek,
                                               jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    int32_t st = loom_graph_remove_node(h, n, g, i, cascade != JNI_FALSE ? 1 : 0);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphUpsertEdge(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring name, jstring id, jstring src, jstring dst,
                                               jstring label, jbyteArray props, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal,
                                               jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    const char *s = (*env)->GetStringUTFChars(env, src, NULL);
    const char *d = (*env)->GetStringUTFChars(env, dst, NULL);
    const char *lb = (*env)->GetStringUTFChars(env, label, NULL);
    jsize plen = props ? (*env)->GetArrayLength(env, props) : 0;
    jbyte *p = props ? (*env)->GetByteArrayElements(env, props, NULL) : NULL;
    int32_t st = loom_graph_upsert_edge(h, n, g, i, s, d, lb, (const unsigned char *)p, (uintptr_t)plen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    (*env)->ReleaseStringUTFChars(env, src, s);
    (*env)->ReleaseStringUTFChars(env, dst, d);
    (*env)->ReleaseStringUTFChars(env, label, lb);
    if (p) (*env)->ReleaseByteArrayElements(env, props, p, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphGetEdge(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jstring id, jbyteArray passphrase,
                                            jbyteArray kek, jstring auth_principal,
                                            jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_graph_get_edge(h, n, g, i, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphRemoveEdge(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring name, jstring id, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal,
                                               jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    int32_t found = 0;
    int32_t st = loom_graph_remove_edge(h, n, g, i, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphNeighbors(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring name, jstring id, jbyteArray passphrase,
                                              jbyteArray kek, jstring auth_principal,
                                              jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_graph_neighbors_cbor(h, n, g, i, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphOutEdges(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                             jstring name, jstring id, jbyteArray passphrase,
                                             jbyteArray kek, jstring auth_principal,
                                             jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_graph_out_edges_cbor(h, n, g, i, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphInEdges(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jstring id, jbyteArray passphrase,
                                            jbyteArray kek, jstring auth_principal,
                                            jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_graph_in_edges_cbor(h, n, g, i, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphReachable(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring name, jstring start, jlong maxDepth,
                                              jstring viaLabel, jbyteArray passphrase, jbyteArray kek,
                                              jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *s = (*env)->GetStringUTFChars(env, start, NULL);
    const char *vl = viaLabel ? (*env)->GetStringUTFChars(env, viaLabel, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_graph_reachable_cbor(h, n, g, s, (int64_t)maxDepth, vl, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, start, s);
    if (vl) (*env)->ReleaseStringUTFChars(env, viaLabel, vl);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeGraphShortestPath(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                 jstring name, jstring from, jstring to,
                                                 jstring viaLabel, jbyteArray passphrase,
                                                 jbyteArray kek, jstring auth_principal,
                                                 jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *g = (*env)->GetStringUTFChars(env, name, NULL);
    const char *fr = (*env)->GetStringUTFChars(env, from, NULL);
    const char *t = (*env)->GetStringUTFChars(env, to, NULL);
    const char *vl = viaLabel ? (*env)->GetStringUTFChars(env, viaLabel, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_graph_shortest_path_cbor(h, n, g, fr, t, vl, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, g);
    (*env)->ReleaseStringUTFChars(env, from, fr);
    (*env)->ReleaseStringUTFChars(env, to, t);
    if (vl) (*env)->ReleaseStringUTFChars(env, viaLabel, vl);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

/* --- Vector facade (embeddings + metadata + nearest-neighbour search). Each call opens the loom for the op and closes. --- */

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorCreate(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jlong dim, jint metric, jbyteArray passphrase,
                                            jbyteArray kek, jstring auth_principal,
                                            jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    int32_t st = loom_vector_create(h, n, v, (uintptr_t)dim, (int32_t)metric);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorUpsert(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jstring id, jbyteArray vector,
                                            jbyteArray metadata, jbyteArray passphrase, jbyteArray kek,
                                            jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    jsize veclen = vector ? (*env)->GetArrayLength(env, vector) : 0;
    jbyte *vec = vector ? (*env)->GetByteArrayElements(env, vector, NULL) : NULL;
    jsize mlen = metadata ? (*env)->GetArrayLength(env, metadata) : 0;
    jbyte *md = metadata ? (*env)->GetByteArrayElements(env, metadata, NULL) : NULL;
    int32_t st = loom_vector_upsert(h, n, v, i, (const unsigned char *)vec, (uintptr_t)veclen,
                                    (const unsigned char *)md, (uintptr_t)mlen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    (*env)->ReleaseStringUTFChars(env, id, i);
    if (vec) (*env)->ReleaseByteArrayElements(env, vector, vec, JNI_ABORT);
    if (md) (*env)->ReleaseByteArrayElements(env, metadata, md, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorUpsertSource(JNIEnv *env, jobject thiz, jstring path,
                                                  jstring ns, jstring name, jstring id,
                                                  jbyteArray vector, jbyteArray metadata,
                                                  jbyteArray sourceText, jstring modelId,
                                                  jstring weightsDigest, jbyteArray passphrase,
                                                  jbyteArray kek, jstring auth_principal,
                                                  jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    const char *model = modelId ? (*env)->GetStringUTFChars(env, modelId, NULL) : NULL;
    const char *weights = weightsDigest ? (*env)->GetStringUTFChars(env, weightsDigest, NULL) : NULL;
    jsize veclen = vector ? (*env)->GetArrayLength(env, vector) : 0;
    jbyte *vec = vector ? (*env)->GetByteArrayElements(env, vector, NULL) : NULL;
    jsize mlen = metadata ? (*env)->GetArrayLength(env, metadata) : 0;
    jbyte *md = metadata ? (*env)->GetByteArrayElements(env, metadata, NULL) : NULL;
    jsize slen = sourceText ? (*env)->GetArrayLength(env, sourceText) : 0;
    jbyte *src = sourceText ? (*env)->GetByteArrayElements(env, sourceText, NULL) : NULL;
    int32_t st = loom_vector_upsert_source(
        h, n, v, i, (const unsigned char *)vec, (uintptr_t)veclen,
        (const unsigned char *)md, (uintptr_t)mlen, (const unsigned char *)src, (uintptr_t)slen,
        model, modelId ? 1 : 0, weights, weightsDigest ? 1 : 0);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    (*env)->ReleaseStringUTFChars(env, id, i);
    if (model) (*env)->ReleaseStringUTFChars(env, modelId, model);
    if (weights) (*env)->ReleaseStringUTFChars(env, weightsDigest, weights);
    if (vec) (*env)->ReleaseByteArrayElements(env, vector, vec, JNI_ABORT);
    if (md) (*env)->ReleaseByteArrayElements(env, metadata, md, JNI_ABORT);
    if (src) (*env)->ReleaseByteArrayElements(env, sourceText, src, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorGet(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring name, jstring id, jbyteArray passphrase,
                                         jbyteArray kek, jstring auth_principal,
                                         jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_vector_get(h, n, v, i, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorSourceText(JNIEnv *env, jobject thiz, jstring path,
                                                jstring ns, jstring name, jstring id,
                                                jbyteArray passphrase, jbyteArray kek,
                                                jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_vector_source_text(h, n, v, i, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorEmbeddingModel(JNIEnv *env, jobject thiz, jstring path,
                                                    jstring ns, jstring name,
                                                    jbyteArray passphrase, jbyteArray kek,
                                                    jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_vector_embedding_model_cbor(h, n, v, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorIds(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring name, jstring prefix, jbyteArray passphrase,
                                         jbyteArray kek, jstring auth_principal,
                                         jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    const char *p = prefix ? (*env)->GetStringUTFChars(env, prefix, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_vector_ids_cbor(h, n, v, p, prefix ? 1 : 0, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    if (p) (*env)->ReleaseStringUTFChars(env, prefix, p);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorMetadataIndexKeys(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                       jstring name, jbyteArray passphrase,
                                                       jbyteArray kek, jstring auth_principal,
                                                       jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_vector_metadata_index_keys_cbor(h, n, v, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorCreateMetadataIndex(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                         jstring name, jstring key, jbyteArray passphrase,
                                                         jbyteArray kek, jstring auth_principal,
                                                         jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    const char *k = (*env)->GetStringUTFChars(env, key, NULL);
    int32_t changed = 0;
    int32_t st = loom_vector_create_metadata_index(h, n, v, k, &changed);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    (*env)->ReleaseStringUTFChars(env, key, k);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return changed != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorDropMetadataIndex(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                       jstring name, jstring key, jbyteArray passphrase,
                                                       jbyteArray kek, jstring auth_principal,
                                                       jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    const char *k = (*env)->GetStringUTFChars(env, key, NULL);
    int32_t changed = 0;
    int32_t st = loom_vector_drop_metadata_index(h, n, v, k, &changed);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    (*env)->ReleaseStringUTFChars(env, key, k);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return changed != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorDelete(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jstring id, jbyteArray passphrase,
                                            jbyteArray kek, jstring auth_principal,
                                            jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    const char *i = (*env)->GetStringUTFChars(env, id, NULL);
    int32_t found = 0;
    int32_t st = loom_vector_delete(h, n, v, i, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    (*env)->ReleaseStringUTFChars(env, id, i);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorSearch(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jbyteArray query, jlong k, jbyteArray filter,
                                            jbyteArray passphrase, jbyteArray kek,
                                            jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    jsize qlen = query ? (*env)->GetArrayLength(env, query) : 0;
    jbyte *q = query ? (*env)->GetByteArrayElements(env, query, NULL) : NULL;
    jsize flen = filter ? (*env)->GetArrayLength(env, filter) : 0;
    jbyte *f = filter ? (*env)->GetByteArrayElements(env, filter, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_vector_search_cbor(h, n, v, (const unsigned char *)q, (uintptr_t)qlen,
                                         (uintptr_t)k, (const unsigned char *)f, (uintptr_t)flen,
                                         &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    if (q) (*env)->ReleaseByteArrayElements(env, query, q, JNI_ABORT);
    if (f) (*env)->ReleaseByteArrayElements(env, filter, f, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeVectorSearchPolicy(JNIEnv *env, jobject thiz, jstring path,
                                                  jstring ns, jstring name, jbyteArray query,
                                                  jlong k, jbyteArray filter, jint policy,
                                                  jlong threshold, jlong ef, jlong pqM, jlong pqK,
                                                  jlong pqIters, jbyteArray passphrase,
                                                  jbyteArray kek, jstring auth_principal,
                                                  jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *v = (*env)->GetStringUTFChars(env, name, NULL);
    jsize qlen = query ? (*env)->GetArrayLength(env, query) : 0;
    jbyte *q = query ? (*env)->GetByteArrayElements(env, query, NULL) : NULL;
    jsize flen = filter ? (*env)->GetArrayLength(env, filter) : 0;
    jbyte *f = filter ? (*env)->GetByteArrayElements(env, filter, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_vector_search_policy_cbor(
        h, n, v, (const unsigned char *)q, (uintptr_t)qlen, (uintptr_t)k,
        (const unsigned char *)f, (uintptr_t)flen, (int32_t)policy, (uintptr_t)threshold,
        (uintptr_t)ef, (uintptr_t)pqM, (uintptr_t)pqK, (uintptr_t)pqIters, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, v);
    if (q) (*env)->ReleaseByteArrayElements(env, query, q, JNI_ABORT);
    if (f) (*env)->ReleaseByteArrayElements(env, filter, f, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

/* --- Columnar facade (typed columns + append/scan/select). Each call opens the loom for the op and closes. --- */

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarCreate(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring name, jbyteArray columns, jlong targetSegmentRows,
                                              jbyteArray passphrase, jbyteArray kek,
                                              jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize collen = columns ? (*env)->GetArrayLength(env, columns) : 0;
    jbyte *col = columns ? (*env)->GetByteArrayElements(env, columns, NULL) : NULL;
    int32_t st = loom_columnar_create(h, n, c, (const unsigned char *)col, (uintptr_t)collen,
                                      (uintptr_t)targetSegmentRows);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (col) (*env)->ReleaseByteArrayElements(env, columns, col, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarAppend(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring name, jbyteArray row, jbyteArray passphrase,
                                              jbyteArray kek, jstring auth_principal,
                                              jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize rlen = row ? (*env)->GetArrayLength(env, row) : 0;
    jbyte *r = row ? (*env)->GetByteArrayElements(env, row, NULL) : NULL;
    int32_t st = loom_columnar_append(h, n, c, (const unsigned char *)r, (uintptr_t)rlen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (r) (*env)->ReleaseByteArrayElements(env, row, r, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarScan(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jbyteArray passphrase, jbyteArray kek,
                                            jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_columnar_scan_cbor(h, n, c, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarColumns(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring name, jbyteArray passphrase, jbyteArray kek,
                                               jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_columnar_columns_cbor(h, n, c, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jlong JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarRows(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jbyteArray passphrase, jbyteArray kek,
                                            jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return 0;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    uint64_t out = 0;
    int32_t st = loom_columnar_rows(h, n, c, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return 0;
    }
    return (jlong)out;
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarCompact(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring name, jbyteArray passphrase, jbyteArray kek,
                                               jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    int32_t st = loom_columnar_compact(h, n, c);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarInspect(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring name, jbyteArray passphrase, jbyteArray kek,
                                               jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_columnar_inspect_cbor(h, n, c, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarSourceDigest(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                    jstring name, jbyteArray passphrase, jbyteArray kek,
                                                    jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_columnar_source_digest_cbor(h, n, c, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarSelect(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                              jstring name, jbyteArray columns, jbyteArray filter,
                                              jbyteArray passphrase, jbyteArray kek,
                                              jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize collen = columns ? (*env)->GetArrayLength(env, columns) : 0;
    jbyte *col = columns ? (*env)->GetByteArrayElements(env, columns, NULL) : NULL;
    jsize flen = filter ? (*env)->GetArrayLength(env, filter) : 0;
    jbyte *f = filter ? (*env)->GetByteArrayElements(env, filter, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_columnar_select_cbor(h, n, c, (const unsigned char *)col, (uintptr_t)collen,
                                           (const unsigned char *)f, (uintptr_t)flen, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (col) (*env)->ReleaseByteArrayElements(env, columns, col, JNI_ABORT);
    if (f) (*env)->ReleaseByteArrayElements(env, filter, f, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeColumnarAggregate(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                 jstring name, jbyteArray aggregates, jbyteArray filter,
                                                 jbyteArray passphrase, jbyteArray kek,
                                                 jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize alen = aggregates ? (*env)->GetArrayLength(env, aggregates) : 0;
    jbyte *a = aggregates ? (*env)->GetByteArrayElements(env, aggregates, NULL) : NULL;
    jsize flen = filter ? (*env)->GetArrayLength(env, filter) : 0;
    jbyte *f = filter ? (*env)->GetByteArrayElements(env, filter, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_columnar_aggregate_cbor(h, n, c, (const unsigned char *)a, (uintptr_t)alen,
                                              (const unsigned char *)f, (uintptr_t)flen, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (a) (*env)->ReleaseByteArrayElements(env, aggregates, a, JNI_ABORT);
    if (f) (*env)->ReleaseByteArrayElements(env, filter, f, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

/* --- Dataframe facade (plans + collect/preview/materialize). Each call opens the loom for the op and closes. --- */

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeDataframeCreate(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                               jstring name, jbyteArray plan, jbyteArray passphrase,
                                               jbyteArray kek, jstring auth_principal,
                                               jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize plen = plan ? (*env)->GetArrayLength(env, plan) : 0;
    jbyte *p = plan ? (*env)->GetByteArrayElements(env, plan, NULL) : NULL;
    int32_t st = loom_dataframe_create(h, n, c, (const unsigned char *)p, (uintptr_t)plen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (p) (*env)->ReleaseByteArrayElements(env, plan, p, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDataframeCollect(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                jstring name, jbyteArray passphrase, jbyteArray kek,
                                                jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_dataframe_collect_cbor(h, n, c, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDataframePreview(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                jstring name, jlong rows, jbyteArray passphrase,
                                                jbyteArray kek, jstring auth_principal,
                                                jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_dataframe_preview_cbor(h, n, c, (uint64_t)rows, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDataframeMaterialize(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                    jstring name, jbyteArray passphrase, jbyteArray kek,
                                                    jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    char *out = NULL;
    int32_t has_digest = 0;
    int32_t st = loom_dataframe_materialize(h, n, c, &out, &has_digest);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        if (out) loom_string_free(out);
        throw_loom(env);
        return NULL;
    }
    if (!has_digest) return NULL;
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_LoomNative_nativeDataframePlanDigest(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                   jstring name, jbyteArray passphrase, jbyteArray kek,
                                                   jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    char *out = NULL;
    int32_t st = loom_dataframe_plan_digest(h, n, c, &out);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        if (out) loom_string_free(out);
        throw_loom(env);
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, out ? out : "");
    if (out) loom_string_free(out);
    return r;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeDataframeSourceDigests(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                                      jstring name, jbyteArray passphrase, jbyteArray kek,
                                                      jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_dataframe_source_digests_cbor(h, n, c, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

/* --- Search facade (mapped fields + index/get/delete/query). Each call opens the loom for the op and closes. --- */

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeSearchCreate(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jbyteArray mapping, jbyteArray passphrase,
                                            jbyteArray kek, jstring auth_principal,
                                            jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize mlen = mapping ? (*env)->GetArrayLength(env, mapping) : 0;
    jbyte *m = mapping ? (*env)->GetByteArrayElements(env, mapping, NULL) : NULL;
    int32_t st = loom_search_create(h, n, c, (const unsigned char *)m, (uintptr_t)mlen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (m) (*env)->ReleaseByteArrayElements(env, mapping, m, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeSearchIndex(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jstring name, jbyteArray id, jbyteArray doc,
                                           jbyteArray passphrase, jbyteArray kek,
                                           jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize ilen = id ? (*env)->GetArrayLength(env, id) : 0;
    jbyte *i = id ? (*env)->GetByteArrayElements(env, id, NULL) : NULL;
    jsize dlen = doc ? (*env)->GetArrayLength(env, doc) : 0;
    jbyte *d = doc ? (*env)->GetByteArrayElements(env, doc, NULL) : NULL;
    int32_t st = loom_search_index(h, n, c, (const unsigned char *)i, (uintptr_t)ilen,
                                   (const unsigned char *)d, (uintptr_t)dlen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (i) (*env)->ReleaseByteArrayElements(env, id, i, JNI_ABORT);
    if (d) (*env)->ReleaseByteArrayElements(env, doc, d, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSearchGet(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring name, jbyteArray id, jbyteArray passphrase,
                                         jbyteArray kek, jstring auth_principal,
                                         jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize ilen = id ? (*env)->GetArrayLength(env, id) : 0;
    jbyte *i = id ? (*env)->GetByteArrayElements(env, id, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t found = 0;
    int32_t st = loom_search_get(h, n, c, (const unsigned char *)i, (uintptr_t)ilen, &ptr, &len, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (i) (*env)->ReleaseByteArrayElements(env, id, i, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    if (found == 0) return NULL;
    return owned_bytes(env, ptr, len);
}

JNIEXPORT jboolean JNICALL
Java_ai_uldren_loom_LoomNative_nativeSearchDelete(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                            jstring name, jbyteArray id, jbyteArray passphrase,
                                            jbyteArray kek, jstring auth_principal,
                                            jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return JNI_FALSE;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize ilen = id ? (*env)->GetArrayLength(env, id) : 0;
    jbyte *i = id ? (*env)->GetByteArrayElements(env, id, NULL) : NULL;
    int32_t found = 0;
    int32_t st = loom_search_delete(h, n, c, (const unsigned char *)i, (uintptr_t)ilen, &found);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (i) (*env)->ReleaseByteArrayElements(env, id, i, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return JNI_FALSE;
    }
    return found != 0 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSearchIds(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                         jstring name, jbyteArray prefix, jboolean hasPrefix,
                                         jbyteArray passphrase, jbyteArray kek,
                                         jstring auth_principal, jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize plen = prefix ? (*env)->GetArrayLength(env, prefix) : 0;
    jbyte *p = prefix ? (*env)->GetByteArrayElements(env, prefix, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_search_ids_cbor(h, n, c, (const unsigned char *)p, (uintptr_t)plen,
                                      hasPrefix != JNI_FALSE ? 1 : 0, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (p) (*env)->ReleaseByteArrayElements(env, prefix, p, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}

JNIEXPORT void JNICALL
Java_ai_uldren_loom_LoomNative_nativeSearchRemap(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jstring name, jbyteArray mapping, jbyteArray passphrase,
                                           jbyteArray kek, jstring auth_principal,
                                           jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize mlen = mapping ? (*env)->GetArrayLength(env, mapping) : 0;
    jbyte *m = mapping ? (*env)->GetByteArrayElements(env, mapping, NULL) : NULL;
    int32_t st = loom_search_remap(h, n, c, (const unsigned char *)m, (uintptr_t)mlen);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (m) (*env)->ReleaseByteArrayElements(env, mapping, m, JNI_ABORT);
    loom_close(h);
    if (st != 0) throw_loom(env);
}

JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_LoomNative_nativeSearchQuery(JNIEnv *env, jobject thiz, jstring path, jstring ns,
                                           jstring name, jbyteArray request, jbyteArray passphrase,
                                           jbyteArray kek, jstring auth_principal,
                                           jbyteArray auth_passphrase) {
    (void)thiz;
    LoomSession *h = open_authenticated_store_handle(env, path, passphrase, kek, auth_principal, auth_passphrase);
    if (!h) return NULL;
    const char *n = (*env)->GetStringUTFChars(env, ns, NULL);
    const char *c = (*env)->GetStringUTFChars(env, name, NULL);
    jsize rlen = request ? (*env)->GetArrayLength(env, request) : 0;
    jbyte *r = request ? (*env)->GetByteArrayElements(env, request, NULL) : NULL;
    unsigned char *ptr = NULL;
    uintptr_t len = 0;
    int32_t st = loom_search_query_cbor(h, n, c, (const unsigned char *)r, (uintptr_t)rlen, &ptr, &len);
    (*env)->ReleaseStringUTFChars(env, ns, n);
    (*env)->ReleaseStringUTFChars(env, name, c);
    if (r) (*env)->ReleaseByteArrayElements(env, request, r, JNI_ABORT);
    loom_close(h);
    if (st != 0) {
        throw_loom(env);
        return NULL;
    }
    return owned_bytes(env, ptr, len);
}
