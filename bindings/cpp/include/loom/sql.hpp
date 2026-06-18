#pragma once
#include "engine.hpp"

namespace uldren::loom {

/// A SQL session over a workspace SQL facet in a `.loom` (RAII over `LoomSqlSession`). A reopenable handle:
/// each call opens the loom for its duration and releases it, so sessions are cheap and coexist. Throws
/// `uldren::loom::error` on failure.
class sql {
public:
    sql(const std::string &path, const std::string &ns, const std::string &db) {
        detail::check(::loom_sql_open(path.c_str(), ns.c_str(), db.c_str(), &session_));
    }
    ~sql() { ::loom_sql_close(session_); }

    /// Open a session over an **encrypted** loom, unlocking it with `passphrase`.
    /// The host acquires the passphrase securely; the FFI never reads an environment variable.
    static sql open_keyed(const std::string &path, const std::string &ns, const std::string &db,
                          const std::string &passphrase) {
        LoomSqlSession *session = nullptr;
        detail::check(::loom_sql_open_keyed(
            path.c_str(), ns.c_str(), db.c_str(),
            reinterpret_cast<const unsigned char *>(passphrase.data()), passphrase.size(),
            &session));
        return sql(session);
    }

    /// Open a session over an **encrypted** loom with a host-supplied 256-bit `kek` that directly unwraps
    /// the DEK. `kek` may come from a keychain, Secure Enclave, passkey-PRF, or KMS. `kek` must be 32 bytes.
    static sql open_with_kek(const std::string &path, const std::string &ns, const std::string &db,
                             const std::vector<std::uint8_t> &kek) {
        LoomSqlSession *session = nullptr;
        detail::check(::loom_sql_open_with_kek(path.c_str(), ns.c_str(), db.c_str(), kek.data(),
                                               kek.size(), &session));
        return sql(session);
    }

    static sql authenticated(const std::string &path, const std::string &ns, const std::string &db,
                             const std::string &auth_principal,
                             const std::string &auth_passphrase) {
        LoomSqlSession *session = nullptr;
        detail::check(::loom_sql_open_authenticated(
            path.c_str(), ns.c_str(), db.c_str(), auth_principal.c_str(),
            reinterpret_cast<const unsigned char *>(auth_passphrase.data()),
            auth_passphrase.size(), &session));
        return sql(session);
    }

    static sql open_keyed_authenticated(const std::string &path, const std::string &ns,
                                        const std::string &db, const std::string &passphrase,
                                        const std::string &auth_principal,
                                        const std::string &auth_passphrase) {
        LoomSqlSession *session = nullptr;
        detail::check(::loom_sql_open_keyed_authenticated(
            path.c_str(), ns.c_str(), db.c_str(),
            reinterpret_cast<const unsigned char *>(passphrase.data()), passphrase.size(),
            auth_principal.c_str(),
            reinterpret_cast<const unsigned char *>(auth_passphrase.data()),
            auth_passphrase.size(), &session));
        return sql(session);
    }

    static sql open_with_kek_authenticated(const std::string &path, const std::string &ns,
                                           const std::string &db,
                                           const std::vector<std::uint8_t> &kek,
                                           const std::string &auth_principal,
                                           const std::string &auth_passphrase) {
        LoomSqlSession *session = nullptr;
        detail::check(::loom_sql_open_with_kek_authenticated(
            path.c_str(), ns.c_str(), db.c_str(), kek.data(), kek.size(),
            auth_principal.c_str(),
            reinterpret_cast<const unsigned char *>(auth_passphrase.data()),
            auth_passphrase.size(), &session));
        return sql(session);
    }

    sql(const sql &) = delete;
    sql &operator=(const sql &) = delete;
    sql(sql &&other) noexcept : session_(other.session_) { other.session_ = nullptr; }
    sql &operator=(sql &&other) noexcept {
        if (this != &other) {
            ::loom_sql_close(session_);
            session_ = other.session_;
            other.session_ = nullptr;
        }
        return *this;
    }

    /// Run SQL and return a **typed**, indexed `result` (decoded once via the shared result-view; no
    /// CBOR is parsed in C++). Cells read back through `value` as faithful types. For the raw canonical
    /// bytes use `exec_bytes`; for the JSON debug form use `exec_json`.
    result exec(const std::string &statement) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_exec(session_, statement.c_str(), &ptr, &len));
        LoomResultView *view = nullptr;
        std::int32_t status = ::loom_result_open(ptr, len, &view);
        ::loom_bytes_free(ptr, len);  // result_open decodes into an owned view; the bytes are done.
        detail::check(status);
        return result(view);
    }

    /// Run SQL; returns a JSON array of the result payloads (debug/admin form, rendered from the
    /// canonical-CBOR result - not the type-faithful API; use `exec`).
    std::string exec_json(const std::string &statement) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_exec(session_, statement.c_str(), &ptr, &len));
        return detail::take_result_json(ptr, len);
    }

    /// Run SQL; returns the result payloads as canonical CBOR bytes.
    std::vector<std::uint8_t> exec_bytes(const std::string &statement) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_exec(session_, statement.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Run a `SELECT` and return a lazy [`row_stream`] over its rows (the streaming form):
    /// `for (auto row = q.next(); row; row = q.next()) { row->cell(0, 0, col); }`. Rows are pulled and
    /// decoded one at a time, so a large result is never fully materialized.
    row_stream query(const std::string &statement) {
        LoomIter *it = nullptr;
        detail::check(::loom_sql_query(session_, statement.c_str(), &it));
        return row_stream(it);
    }

    /// Run SQL asynchronously (the poll/handle form). The returned future yields the
    /// canonical-CBOR result bytes; the blocking wait runs on a background thread (`std::async`), off
    /// the caller's thread. The session MUST outlive the returned future.
    std::future<std::vector<std::uint8_t>> exec_async(const std::string &statement) {
        LoomSqlSession *session = session_;
        std::string stmt = statement;
        return std::async(std::launch::async, [session, stmt]() {
            LoomTask *task = nullptr;
            detail::check(::loom_sql_exec_async(session, stmt.c_str(), &task));
            std::uint8_t *ptr = nullptr;
            std::uintptr_t len = 0;
            std::int32_t status = ::loom_task_wait(task, &ptr, &len);
            ::loom_task_free(task);
            detail::check(status);
            return detail::take_result_bytes(ptr, len);
        });
    }

    /// Commit the staged database state; returns the new commit's content address.
    std::string commit(const std::string &message, const std::string &author) {
        char *out = nullptr;
        detail::check(::loom_sql_commit(session_, message.c_str(), author.c_str(), &out));
        return detail::take_string(out);
    }

private:
    explicit sql(LoomSqlSession *session) : session_(session) {}
    LoomSqlSession *session_ = nullptr;
};

inline sql Loom::sql_session(const std::string &ns, const std::string &db) const {
    if (!auth_principal_.empty()) {
        if (!kek_.empty()) {
            return sql::open_with_kek_authenticated(
                path_, ns, db, kek_, auth_principal_, auth_passphrase_);
        }
        if (!passphrase_.empty()) {
            return sql::open_keyed_authenticated(
                path_, ns, db, passphrase_, auth_principal_, auth_passphrase_);
        }
        return sql::authenticated(path_, ns, db, auth_principal_, auth_passphrase_);
    }
    if (!kek_.empty()) {
        return sql::open_with_kek(path_, ns, db, kek_);
    }
    if (!passphrase_.empty()) {
        return sql::open_keyed(path_, ns, db, passphrase_);
    }
    return sql(path_, ns, db);
}

}  // namespace uldren::loom
