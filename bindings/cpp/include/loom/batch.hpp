#pragma once
#include "sql.hpp"

namespace uldren::loom {

/// An explicit transaction/batch scope (RAII over `LoomSqlBatch`). Unlike `sql`, a batch
/// holds the `.loom` open - and its exclusive write lock - for its whole lifetime, so an SQL transaction
/// (`BEGIN`/`COMMIT`/`ROLLBACK`) can span `exec` calls; changes become durable through a single atomic
/// save at `commit` (or `commit_vcs`). The SQL `COMMIT` is distinct from the VCS commit. The destructor
/// closes the batch, discarding any un-persisted changes. Throws `uldren::loom::error` on failure.
class batch {
public:
    batch(const std::string &path, const std::string &ns, const std::string &db) {
        detail::check(::loom_sql_batch_begin(path.c_str(), ns.c_str(), db.c_str(), &batch_));
    }
    ~batch() { ::loom_sql_batch_close(batch_); }

    /// Begin a batch over an **encrypted** loom, unlocking it with `passphrase` for the batch's lifetime.
    static batch begin_keyed(const std::string &path, const std::string &ns, const std::string &db,
                             const std::string &passphrase) {
        LoomSqlBatch *b = nullptr;
        detail::check(::loom_sql_batch_begin_keyed(
            path.c_str(), ns.c_str(), db.c_str(),
            reinterpret_cast<const unsigned char *>(passphrase.data()), passphrase.size(), &b));
        return batch(b);
    }

    /// Begin a batch over an **encrypted** loom with a host-supplied 256-bit `kek`. `kek` must be 32 bytes.
    static batch begin_with_kek(const std::string &path, const std::string &ns,
                                const std::string &db, const std::vector<std::uint8_t> &kek) {
        LoomSqlBatch *b = nullptr;
        detail::check(::loom_sql_batch_begin_with_kek(path.c_str(), ns.c_str(), db.c_str(),
                                                      kek.data(), kek.size(), &b));
        return batch(b);
    }

    static batch authenticated(const std::string &path, const std::string &ns,
                               const std::string &db, const std::string &auth_principal,
                               const std::string &auth_passphrase) {
        LoomSqlBatch *b = nullptr;
        detail::check(::loom_sql_batch_begin_authenticated(
            path.c_str(), ns.c_str(), db.c_str(), auth_principal.c_str(),
            reinterpret_cast<const unsigned char *>(auth_passphrase.data()),
            auth_passphrase.size(), &b));
        return batch(b);
    }

    static batch begin_keyed_authenticated(const std::string &path, const std::string &ns,
                                           const std::string &db,
                                           const std::string &passphrase,
                                           const std::string &auth_principal,
                                           const std::string &auth_passphrase) {
        LoomSqlBatch *b = nullptr;
        detail::check(::loom_sql_batch_begin_keyed_authenticated(
            path.c_str(), ns.c_str(), db.c_str(),
            reinterpret_cast<const unsigned char *>(passphrase.data()), passphrase.size(),
            auth_principal.c_str(),
            reinterpret_cast<const unsigned char *>(auth_passphrase.data()),
            auth_passphrase.size(), &b));
        return batch(b);
    }

    static batch begin_with_kek_authenticated(const std::string &path, const std::string &ns,
                                              const std::string &db,
                                              const std::vector<std::uint8_t> &kek,
                                              const std::string &auth_principal,
                                              const std::string &auth_passphrase) {
        LoomSqlBatch *b = nullptr;
        detail::check(::loom_sql_batch_begin_with_kek_authenticated(
            path.c_str(), ns.c_str(), db.c_str(), kek.data(), kek.size(), auth_principal.c_str(),
            reinterpret_cast<const unsigned char *>(auth_passphrase.data()),
            auth_passphrase.size(), &b));
        return batch(b);
    }

    batch(const batch &) = delete;
    batch &operator=(const batch &) = delete;
    batch(batch &&other) noexcept : batch_(other.batch_) { other.batch_ = nullptr; }
    batch &operator=(batch &&other) noexcept {
        if (this != &other) {
            ::loom_sql_batch_close(batch_);
            batch_ = other.batch_;
            other.batch_ = nullptr;
        }
        return *this;
    }

    /// Run SQL in the batch (including `BEGIN`/`COMMIT`/`ROLLBACK`) and return a typed `result`. Changes
    /// accumulate until `commit`.
    result exec(const std::string &statement) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_batch_exec(batch_, statement.c_str(), &ptr, &len));
        LoomResultView *view = nullptr;
        std::int32_t status = ::loom_result_open(ptr, len, &view);
        ::loom_bytes_free(ptr, len);
        detail::check(status);
        return result(view);
    }

    /// Run SQL in the batch; returns the JSON debug form.
    std::string exec_json(const std::string &statement) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_batch_exec(batch_, statement.c_str(), &ptr, &len));
        return detail::take_result_json(ptr, len);
    }

    /// Run SQL in the batch; returns the result payloads as canonical CBOR bytes.
    std::vector<std::uint8_t> exec_bytes(const std::string &statement) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_batch_exec(batch_, statement.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Make the batch's changes durable with one atomic save (no history entry). Rejected while an SQL
    /// transaction is open. The batch stays open.
    void commit() { detail::check(::loom_sql_batch_commit(batch_)); }

    /// Like `commit`, but also records a VCS commit; returns its content address. Distinct from a SQL
    /// `COMMIT`. Rejected while an SQL transaction is open.
    std::string commit_vcs(const std::string &message, const std::string &author) {
        char *out = nullptr;
        detail::check(::loom_sql_batch_commit_vcs(batch_, message.c_str(), author.c_str(), &out));
        return detail::take_string(out);
    }

    /// Discard un-persisted in-memory changes (and any open SQL transaction); the batch stays open.
    void abort() { detail::check(::loom_sql_batch_abort(batch_)); }

private:
    explicit batch(LoomSqlBatch *b) : batch_(b) {}
    LoomSqlBatch *batch_ = nullptr;
};

inline batch Loom::sql_batch(const std::string &ns, const std::string &db) const {
    if (!auth_principal_.empty()) {
        if (!kek_.empty()) {
            return batch::begin_with_kek_authenticated(
                path_, ns, db, kek_, auth_principal_, auth_passphrase_);
        }
        if (!passphrase_.empty()) {
            return batch::begin_keyed_authenticated(
                path_, ns, db, passphrase_, auth_principal_, auth_passphrase_);
        }
        return batch::authenticated(path_, ns, db, auth_principal_, auth_passphrase_);
    }
    if (!kek_.empty()) {
        return batch::begin_with_kek(path_, ns, db, kek_);
    }
    if (!passphrase_.empty()) {
        return batch::begin_keyed(path_, ns, db, passphrase_);
    }
    return batch(path_, ns, db);
}

/// Create a fresh `.loom` under an identity `profile` (`"default"`/`"blake3"` or `"fips"`/`"sha256"`),
/// optionally encrypted - the binding counterpart of `loom init`. A non-empty
/// `passphrase` encrypts the store; the DEK is wrapped under it with `suite`, or the profile default
/// when `suite` is empty; an empty `passphrase` makes an unencrypted store. Throws
/// `ALREADY_EXISTS` if a non-empty file already exists at `path`.
inline void create(const std::string &path, const std::string &profile,
                   const std::string &suite = "", const std::string &passphrase = "") {
    detail::check(::loom_create(
        path.c_str(), profile.c_str(), suite.empty() ? nullptr : suite.c_str(),
        passphrase.empty() ? nullptr : reinterpret_cast<const unsigned char *>(passphrase.data()),
        passphrase.size()));
}

/// Create a fresh **encrypted** `.loom` whose DEK is wrapped under a host-supplied 256-bit `kek`.
/// `profile` selects the content-address algorithm and `suite` the object AEAD (profile default when
/// empty). `kek` must be 32 bytes.
inline void create_with_kek(const std::string &path, const std::string &profile,
                            const std::vector<std::uint8_t> &kek, const std::string &suite = "") {
    detail::check(::loom_create_with_kek(path.c_str(), profile.c_str(),
                                         suite.empty() ? nullptr : suite.c_str(), kek.data(),
                                         kek.size()));
}

}  // namespace uldren::loom
