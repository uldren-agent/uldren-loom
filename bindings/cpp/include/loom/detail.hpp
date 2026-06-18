#pragma once
#include "error.hpp"

namespace uldren::loom {

namespace detail {

/// Take ownership of a library-returned C string (free it) and copy into a std::string.
inline std::string take_string(char *raw) {
    std::string out = raw ? std::string(raw) : std::string();
    ::loom_string_free(raw);
    return out;
}

inline void check(std::int32_t status);

/// Take ownership of a library-returned canonical-CBOR result buffer, render it to JSON (debug form),
/// and free the buffer. Returns the JSON; throws if the render fails.
inline std::string take_result_json(std::uint8_t *ptr, std::uintptr_t len) {
    char *json = nullptr;
    std::int32_t status = ::loom_result_to_json(ptr, len, &json);
    ::loom_bytes_free(ptr, len);
    check(status);
    return take_string(json);
}

/// Take ownership of a library-returned canonical-CBOR result buffer as raw bytes, freeing it.
inline std::vector<std::uint8_t> take_result_bytes(std::uint8_t *ptr, std::uintptr_t len) {
    std::vector<std::uint8_t> out(ptr, ptr ? ptr + len : ptr);
    ::loom_bytes_free(ptr, len);
    return out;
}

/// Build and throw the thread's last error. Call only after a non-zero status.
[[noreturn]] inline void throw_last_error() {
    std::int32_t code = 0;
    char *msg = nullptr;
    std::uintptr_t len = 0;
    ::loom_last_error(&code, &msg, &len);
    std::string m = msg ? std::string(msg, len) : std::string("loom error");
    ::loom_string_free(msg);
    throw error(code, std::move(m));
}

inline void check(std::int32_t status) {
    if (status != 0) {
        throw_last_error();
    }
}

}  // namespace detail

/// Library version.
inline std::string version() {
    return detail::take_string(::loom_version());
}

/// Build capability report (0010 section 5) as canonical CBOR: a `CapabilitySet` map with
/// `schema_version` and `records`. Build-aware: capabilities owned by the linked crates are reported
/// with operational state `supported`. Mirrors the C ABI `loom_capabilities`.
inline std::vector<std::uint8_t> capabilities() {
    std::uint8_t *ptr = nullptr;
    std::uintptr_t len = 0;
    detail::check(::loom_capabilities(&ptr, &len));
    return detail::take_result_bytes(ptr, len);
}

/// Runtime provider/profile report as canonical CBOR.
inline std::vector<std::uint8_t> runtime_profile() {
    std::uint8_t *ptr = nullptr;
    std::uintptr_t len = 0;
    detail::check(::loom_runtime_profile(&ptr, &len));
    return detail::take_result_bytes(ptr, len);
}

inline std::string studio_surface_catalog_json(const std::string &workspace,
                                               const std::string &set = "all") {
    char *out = nullptr;
    detail::check(::loom_studio_surface_catalog_json(workspace.c_str(), set.c_str(), &out));
    return detail::take_string(out);
}

/// Blob content address ("algo:hex") of the given bytes.
inline std::string blob_digest(const std::vector<std::uint8_t> &data) {
    return detail::take_string(::loom_blob_digest(data.data(), data.size()));
}

/// Local daemon status for `path` as JSON. Missing daemons return a STOPPED JSON payload.
inline std::string daemon_status_json(const std::string &path) {
    char *out = nullptr;
    detail::check(::loom_daemon_status_json(path.c_str(), &out));
    return detail::take_string(out);
}

/// Attach or detach a named session from a running local daemon.
inline void daemon_session_attach(const std::string &path, const std::string &session) {
    detail::check(::loom_daemon_session_attach(path.c_str(), session.c_str()));
}

inline void daemon_session_detach(const std::string &path, const std::string &session) {
    detail::check(::loom_daemon_session_detach(path.c_str(), session.c_str()));
}

/// Add or remove a long-lived pin on a running local daemon.
inline void daemon_pin_add(const std::string &path, const std::string &pin) {
    detail::check(::loom_daemon_pin_add(path.c_str(), pin.c_str()));
}

inline void daemon_pin_remove(const std::string &path, const std::string &pin) {
    detail::check(::loom_daemon_pin_remove(path.c_str(), pin.c_str()));
}

/// Acquire, refresh, or release a daemon-backed lock. Token-returning calls return JSON.
inline std::string lock_acquire_json(const std::string &path, const std::string &key,
                                     const std::string &principal, const std::string &session,
                                     const std::string &mode, std::uint32_t permits,
                                     std::uint32_t capacity, std::uint64_t lease_ms,
                                     std::uint64_t wait_ms = 30000) {
    char *out = nullptr;
    detail::check(::loom_lock_acquire_json(path.c_str(), key.c_str(), principal.c_str(),
                                           session.c_str(), mode.c_str(), permits, capacity,
                                           lease_ms, wait_ms, &out));
    return detail::take_string(out);
}

inline std::string lock_refresh_json(const std::string &path, const std::string &key,
                                     const std::string &principal, const std::string &session,
                                     const std::string &mode, std::uint32_t permits,
                                     std::uint32_t capacity, std::uint64_t fence_low,
                                     std::uint64_t fence_high, std::uint64_t lease_ms) {
    char *out = nullptr;
    detail::check(::loom_lock_refresh_json(path.c_str(), key.c_str(), principal.c_str(),
                                           session.c_str(), mode.c_str(), permits, capacity,
                                           fence_low, fence_high, lease_ms, &out));
    return detail::take_string(out);
}

inline void lock_release(const std::string &path, const std::string &key,
                         const std::string &principal, const std::string &session,
                         const std::string &mode, std::uint32_t permits, std::uint32_t capacity,
                         std::uint64_t fence_low, std::uint64_t fence_high) {
    detail::check(::loom_lock_release(path.c_str(), key.c_str(), principal.c_str(),
                                      session.c_str(), mode.c_str(), permits, capacity, fence_low,
                                      fence_high));
}

struct fence_token {
    std::uint32_t authority;
    std::uint32_t epoch;
    std::uint64_t sequence;

    std::uint64_t low() const { return sequence; }

    std::uint64_t high() const {
        return (static_cast<std::uint64_t>(authority) << 32) | static_cast<std::uint64_t>(epoch);
    }
};

struct lock_token {
    std::string key;
    std::string principal;
    std::string session;
    std::string mode;
    std::uint32_t permits;
    std::uint32_t capacity;
    fence_token fence;
    std::uint64_t lease_deadline_ms;
};

namespace detail {

inline std::string lock_json_string(std::string_view json, std::string_view name) {
    std::string needle = "\"" + std::string(name) + "\":\"";
    std::size_t pos = json.find(needle);
    if (pos == std::string_view::npos) {
        throw std::invalid_argument("missing lock token string field");
    }
    pos += needle.size();
    std::string out;
    bool escape = false;
    for (; pos < json.size(); ++pos) {
        char c = json[pos];
        if (escape) {
            out.push_back(c);
            escape = false;
        } else if (c == '\\') {
            escape = true;
        } else if (c == '"') {
            return out;
        } else {
            out.push_back(c);
        }
    }
    throw std::invalid_argument("unterminated lock token string field");
}

inline std::uint64_t lock_json_u64(std::string_view json, std::string_view name) {
    std::string needle = "\"" + std::string(name) + "\":";
    std::size_t pos = json.find(needle);
    if (pos == std::string_view::npos) {
        throw std::invalid_argument("missing lock token numeric field");
    }
    pos += needle.size();
    std::uint64_t value = 0;
    bool found = false;
    for (; pos < json.size() && json[pos] >= '0' && json[pos] <= '9'; ++pos) {
        found = true;
        value = value * 10 + static_cast<std::uint64_t>(json[pos] - '0');
    }
    if (!found) {
        throw std::invalid_argument("invalid lock token numeric field");
    }
    return value;
}

}  // namespace detail

inline lock_token parse_lock_token(std::string_view json) {
    return lock_token{
        detail::lock_json_string(json, "key"),
        detail::lock_json_string(json, "principal"),
        detail::lock_json_string(json, "session"),
        detail::lock_json_string(json, "mode"),
        static_cast<std::uint32_t>(detail::lock_json_u64(json, "permits")),
        static_cast<std::uint32_t>(detail::lock_json_u64(json, "capacity")),
        fence_token{
            static_cast<std::uint32_t>(detail::lock_json_u64(json, "authority")),
            static_cast<std::uint32_t>(detail::lock_json_u64(json, "epoch")),
            detail::lock_json_u64(json, "sequence"),
        },
        detail::lock_json_u64(json, "lease_deadline_ms"),
    };
}

inline lock_token lock_acquire(const std::string &path, const std::string &key,
                               const std::string &principal, const std::string &session,
                               const std::string &mode = "exclusive", std::uint32_t permits = 1,
                               std::uint32_t capacity = 1, std::uint64_t lease_ms = 60000,
                               std::uint64_t wait_ms = 30000) {
    return parse_lock_token(
        lock_acquire_json(path, key, principal, session, mode, permits, capacity, lease_ms, wait_ms));
}

inline lock_token lock_try_acquire(const std::string &path, const std::string &key,
                                   const std::string &principal, const std::string &session,
                                   const std::string &mode = "exclusive",
                                   std::uint32_t permits = 1, std::uint32_t capacity = 1,
                                   std::uint64_t lease_ms = 60000) {
    return lock_acquire(path, key, principal, session, mode, permits, capacity, lease_ms, 0);
}

inline lock_token lock_refresh(const std::string &path, const lock_token &token,
                               std::uint64_t lease_ms = 60000) {
    return parse_lock_token(lock_refresh_json(path, token.key, token.principal, token.session,
                                              token.mode, token.permits, token.capacity,
                                              token.fence.low(), token.fence.high(), lease_ms));
}

inline void lock_release(const std::string &path, const lock_token &token) {
    lock_release(path, token.key, token.principal, token.session, token.mode, token.permits,
                 token.capacity, token.fence.low(), token.fence.high());
}

class lock_guard {
public:
    lock_guard(std::string path, lock_token token)
        : path_(std::move(path)), token_(std::move(token)), released_(false) {}

    lock_guard(const lock_guard &) = delete;
    lock_guard &operator=(const lock_guard &) = delete;

    lock_guard(lock_guard &&other) noexcept
        : path_(std::move(other.path_)), token_(std::move(other.token_)),
          released_(other.released_) {
        other.released_ = true;
    }

    lock_guard &operator=(lock_guard &&other) noexcept {
        if (this != &other) {
            release_noexcept();
            path_ = std::move(other.path_);
            token_ = std::move(other.token_);
            released_ = other.released_;
            other.released_ = true;
        }
        return *this;
    }

    ~lock_guard() {
        release_noexcept();
    }

    const lock_token &token() const {
        return token_;
    }

    lock_token refresh(std::uint64_t lease_ms = 60000) {
        token_ = lock_refresh(path_, token_, lease_ms);
        return token_;
    }

    void release() {
        if (!released_) {
            lock_release(path_, token_);
            released_ = true;
        }
    }

private:
    void release_noexcept() noexcept {
        try {
            release();
        } catch (...) {
        }
    }

    std::string path_;
    lock_token token_;
    bool released_;
};

inline lock_guard scoped_lock(const std::string &path, const std::string &key,
                              const std::string &principal, const std::string &session,
                              const std::string &mode = "exclusive", std::uint32_t permits = 1,
                              std::uint32_t capacity = 1, std::uint64_t lease_ms = 60000,
                              std::uint64_t wait_ms = 30000) {
    return lock_guard(path,
                      lock_acquire(path, key, principal, session, mode, permits, capacity, lease_ms,
                                   wait_ms));
}

}  // namespace uldren::loom
