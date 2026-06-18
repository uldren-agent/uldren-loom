#include "UldrenLoom_jni.h"

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlReadTable(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring table,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *t = env->GetStringUTFChars(table, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_sql_read_table(h, n, t, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(table, t);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlReadTableAt(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring table,
    jstring commit, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *t = env->GetStringUTFChars(table, nullptr);
  const char *c = env->GetStringUTFChars(commit, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_sql_read_table_at(h, n, t, c, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(table, t);
  env->ReleaseStringUTFChars(commit, c);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlIndexScan(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring table,
    jstring index, jbyteArray prefix, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *t = env->GetStringUTFChars(table, nullptr);
  const char *i = env->GetStringUTFChars(index, nullptr);
  jsize plen = (prefix != nullptr) ? env->GetArrayLength(prefix) : 0;
  jbyte *prefixBytes = (prefix != nullptr) ? env->GetByteArrayElements(prefix, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_sql_index_scan(h, n, t, i, reinterpret_cast<const unsigned char *>(prefixBytes),
                       static_cast<uintptr_t>(plen), &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(table, t);
  env->ReleaseStringUTFChars(index, i);
  if (prefixBytes) {
    env->ReleaseByteArrayElements(prefix, prefixBytes, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlIndexScanAt(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring table,
    jstring index, jbyteArray prefix, jstring commit, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *t = env->GetStringUTFChars(table, nullptr);
  const char *i = env->GetStringUTFChars(index, nullptr);
  const char *c = env->GetStringUTFChars(commit, nullptr);
  jsize plen = (prefix != nullptr) ? env->GetArrayLength(prefix) : 0;
  jbyte *prefixBytes = (prefix != nullptr) ? env->GetByteArrayElements(prefix, nullptr) : nullptr;
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_sql_index_scan_at(h, n, t, i, reinterpret_cast<const unsigned char *>(prefixBytes),
                              static_cast<uintptr_t>(plen), c, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(table, t);
  env->ReleaseStringUTFChars(index, i);
  env->ReleaseStringUTFChars(commit, c);
  if (prefixBytes) {
    env->ReleaseByteArrayElements(prefix, prefixBytes, JNI_ABORT);
  }
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlBlame(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring branch,
    jstring table, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *b = env->GetStringUTFChars(branch, nullptr);
  const char *t = env->GetStringUTFChars(table, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_sql_blame(h, n, b, t, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(branch, b);
  env->ReleaseStringUTFChars(table, t);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlDiff(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring table,
    jstring fromCommit, jstring toCommit, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *t = env->GetStringUTFChars(table, nullptr);
  const char *from = env->GetStringUTFChars(fromCommit, nullptr);
  const char *to = env->GetStringUTFChars(toCommit, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_sql_diff(h, n, t, from, to, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(table, t);
  env->ReleaseStringUTFChars(fromCommit, from);
  env->ReleaseStringUTFChars(toCommit, to);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlTableDiff(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring table,
    jstring fromCommit, jstring toCommit, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  LoomSession *h = nullptr;
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h);
  env->ReleaseStringUTFChars(loomPath, p);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *t = env->GetStringUTFChars(table, nullptr);
  const char *from = env->GetStringUTFChars(fromCommit, nullptr);
  const char *to = env->GetStringUTFChars(toCommit, nullptr);
  unsigned char *ptr = nullptr;
  uintptr_t len = 0;
  st = loom_sql_table_diff(h, n, t, from, to, &ptr, &len);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(table, t);
  env->ReleaseStringUTFChars(fromCommit, from);
  env->ReleaseStringUTFChars(toCommit, to);
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  return ownedBytes(env, ptr, len);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlExec(JNIEnv *env, jobject thiz, jstring loomPath,
                                                      jstring ns, jstring db, jstring sql,
                                                      jbyteArray passphrase, jbyteArray kek,
                                                      jstring authPrincipal,
                                                      jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *d = env->GetStringUTFChars(db, nullptr);
  const char *q = env->GetStringUTFChars(sql, nullptr);
  jstring result = nullptr;
  LoomSqlSession *s = nullptr;
  if (openAuthenticatedSessionKeyed(env, p, n, d, passphrase, kek, authPrincipal, authPassphrase, &s) != 0) {
    throwLoom(env);
  } else {
    unsigned char *ptr = nullptr;
    uintptr_t len = 0;
    if (loom_sql_exec(s, q, &ptr, &len) != 0) {
      throwLoom(env);
    } else {
      // Render the canonical-CBOR result to JSON (debug form) for the string-returning API.
      char *json = nullptr;
      int32_t rst = loom_result_to_json(ptr, len, &json);
      loom_bytes_free(ptr, len);
      if (rst != 0) {
        throwLoom(env);
      } else {
        result = env->NewStringUTF(json ? json : "");
        if (json) {
          loom_string_free(json);
        }
      }
    }
    loom_sql_close(s);
  }
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(db, d);
  env->ReleaseStringUTFChars(sql, q);
  return result;
}

// As `nativeSqlExec`, but renders **lossless bridge JSON** (the typed RN form; TS JSON.parses it).

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlExecTyped(JNIEnv *env, jobject thiz,
                                                           jstring loomPath, jstring ns, jstring db,
                                                           jstring sql, jbyteArray passphrase,
                                                           jbyteArray kek, jstring authPrincipal,
                                                           jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *d = env->GetStringUTFChars(db, nullptr);
  const char *q = env->GetStringUTFChars(sql, nullptr);
  jstring result = nullptr;
  LoomSqlSession *s = nullptr;
  if (openAuthenticatedSessionKeyed(env, p, n, d, passphrase, kek, authPrincipal, authPassphrase, &s) != 0) {
    throwLoom(env);
  } else {
    unsigned char *ptr = nullptr;
    uintptr_t len = 0;
    if (loom_sql_exec(s, q, &ptr, &len) != 0) {
      throwLoom(env);
    } else {
      char *json = nullptr;
      int32_t rst = loom_result_to_bridge_json(ptr, len, &json);
      loom_bytes_free(ptr, len);
      if (rst != 0) {
        throwLoom(env);
      } else {
        result = env->NewStringUTF(json ? json : "");
        if (json) {
          loom_string_free(json);
        }
      }
    }
    loom_sql_close(s);
  }
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(db, d);
  env->ReleaseStringUTFChars(sql, q);
  return result;
}

// Atomic transaction/batch in one native round-trip: open a held-open batch, run every
// statement in order (incl. BEGIN/COMMIT/ROLLBACK), commit with one atomic save on success, abort and
// discard on any error. The writer lock stays entirely inside native code. Returns the lossless bridge
// JSON of the final statement's result (TS JSON.parses it).

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlBatch(JNIEnv *env, jobject thiz, jstring loomPath,
                                                       jstring ns, jstring db,
                                                       jobjectArray statements,
                                                       jbyteArray passphrase, jbyteArray kek,
                                                       jstring authPrincipal,
                                                       jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *d = env->GetStringUTFChars(db, nullptr);
  jstring result = nullptr;
  LoomSqlBatch *b = nullptr;
  if (beginAuthenticatedBatchKeyed(env, p, n, d, passphrase, kek, authPrincipal, authPassphrase, &b) != 0) {
    throwLoom(env);
  } else {
    char *json = nullptr;  // bridge JSON of the most recent statement's result
    bool ok = true;
    jsize count = env->GetArrayLength(statements);
    for (jsize i = 0; i < count; i++) {
      jstring s = (jstring)env->GetObjectArrayElement(statements, i);
      const char *q = env->GetStringUTFChars(s, nullptr);
      unsigned char *ptr = nullptr;
      uintptr_t len = 0;
      int32_t st = loom_sql_batch_exec(b, q, &ptr, &len);
      env->ReleaseStringUTFChars(s, q);
      env->DeleteLocalRef(s);
      if (st != 0) {
        ok = false;
        break;
      }
      if (json) {
        loom_string_free(json);
        json = nullptr;
      }
      int32_t rst = loom_result_to_bridge_json(ptr, len, &json);
      loom_bytes_free(ptr, len);
      if (rst != 0) {
        ok = false;
        break;
      }
    }
    if (ok && loom_sql_batch_commit(b) != 0) {
      ok = false;
    }
    if (!ok) {
      if (json) {
        loom_string_free(json);
      }
      loom_sql_batch_abort(b);
      loom_sql_batch_close(b);
      throwLoom(env);
    } else {
      result = env->NewStringUTF(json ? json : "[]");
      if (json) {
        loom_string_free(json);
      }
      loom_sql_batch_close(b);
    }
  }
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(db, d);
  return result;
}

// As `nativeSqlExec`, but returns the canonical-CBOR result payload as a Java byte[]
// rather than rendering it to JSON.

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlExecBytes(JNIEnv *env, jobject thiz,
                                                           jstring loomPath, jstring ns, jstring db,
                                                           jstring sql, jbyteArray passphrase,
                                                           jbyteArray kek, jstring authPrincipal,
                                                           jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *d = env->GetStringUTFChars(db, nullptr);
  const char *q = env->GetStringUTFChars(sql, nullptr);
  jbyteArray result = nullptr;
  LoomSqlSession *s = nullptr;
  if (openAuthenticatedSessionKeyed(env, p, n, d, passphrase, kek, authPrincipal, authPassphrase, &s) != 0) {
    throwLoom(env);
  } else {
    unsigned char *ptr = nullptr;
    uintptr_t len = 0;
    if (loom_sql_exec(s, q, &ptr, &len) != 0) {
      throwLoom(env);
    } else {
      result = env->NewByteArray(static_cast<jsize>(len));
      if (result != nullptr && len > 0) {
        env->SetByteArrayRegion(result, 0, static_cast<jsize>(len),
                                reinterpret_cast<const jbyte *>(ptr));
      }
      loom_bytes_free(ptr, len);
    }
    loom_sql_close(s);
  }
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(db, d);
  env->ReleaseStringUTFChars(sql, q);
  return result;
}

extern "C" JNIEXPORT jobjectArray JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlQueryBytes(JNIEnv *env, jobject thiz,
                                                            jstring loomPath, jstring ns, jstring db,
                                                            jstring sql, jbyteArray passphrase,
                                                            jbyteArray kek, jstring authPrincipal,
                                                            jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *d = env->GetStringUTFChars(db, nullptr);
  const char *q = env->GetStringUTFChars(sql, nullptr);
  jobjectArray result = nullptr;
  LoomSqlSession *s = nullptr;
  std::vector<jbyteArray> rows;
  if (openAuthenticatedSessionKeyed(env, p, n, d, passphrase, kek, authPrincipal, authPassphrase, &s) != 0) {
    throwLoom(env);
  } else {
    LoomIter *it = nullptr;
    if (loom_sql_query(s, q, &it) != 0) {
      throwLoom(env);
    } else {
      bool ok = true;
      for (;;) {
        unsigned char *ptr = nullptr;
        uintptr_t len = 0;
        int32_t done = 0;
        if (loom_iter_next(it, &ptr, &len, &done) != 0) {
          ok = false;
          break;
        }
        if (done != 0) {
          break;
        }
        jbyteArray row = ownedBytes(env, ptr, len);
        if (row == nullptr) {
          ok = false;
          break;
        }
        rows.push_back(row);
      }
      loom_iter_free(it);
      if (ok) {
        jclass byteArrayClass = env->FindClass("[B");
        if (byteArrayClass != nullptr) {
          result = env->NewObjectArray(static_cast<jsize>(rows.size()), byteArrayClass, nullptr);
        }
        if (result != nullptr) {
          for (jsize i = 0; i < static_cast<jsize>(rows.size()); i++) {
            env->SetObjectArrayElement(result, i, rows[static_cast<size_t>(i)]);
          }
        }
      }
      if (!ok && !env->ExceptionCheck()) {
        throwLoom(env);
      }
    }
    loom_sql_close(s);
  }
  for (jbyteArray row : rows) {
    env->DeleteLocalRef(row);
  }
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(db, d);
  env->ReleaseStringUTFChars(sql, q);
  return result;
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeSqlCommit(JNIEnv *env, jobject thiz, jstring loomPath,
                                                        jstring ns, jstring db, jstring message,
                                                        jstring author, jbyteArray passphrase,
                                                        jbyteArray kek, jstring authPrincipal,
                                                        jbyteArray authPassphrase) {
  (void)thiz;
  const char *p = env->GetStringUTFChars(loomPath, nullptr);
  const char *n = env->GetStringUTFChars(ns, nullptr);
  const char *d = env->GetStringUTFChars(db, nullptr);
  const char *m = env->GetStringUTFChars(message, nullptr);
  const char *a = env->GetStringUTFChars(author, nullptr);
  jstring result = nullptr;
  LoomSqlSession *s = nullptr;
  if (openAuthenticatedSessionKeyed(env, p, n, d, passphrase, kek, authPrincipal, authPassphrase, &s) != 0) {
    throwLoom(env);
  } else {
    char *out = nullptr;
    if (loom_sql_commit(s, m, a, &out) != 0) {
      throwLoom(env);
    } else {
      result = env->NewStringUTF(out ? out : "");
      if (out) {
        loom_string_free(out);
      }
    }
    loom_sql_close(s);
  }
  env->ReleaseStringUTFChars(loomPath, p);
  env->ReleaseStringUTFChars(ns, n);
  env->ReleaseStringUTFChars(db, d);
  env->ReleaseStringUTFChars(message, m);
  env->ReleaseStringUTFChars(author, a);
  return result;
}

// Capability registry bytes are canonical CBOR. This call does not open a loom.
