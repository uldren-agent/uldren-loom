#pragma once
#include "result.hpp"

namespace uldren::loom {

/// A lazy, forward stream of a `SELECT`'s rows: RAII over `LoomIter`,
/// it pulls one row at a time via `loom_iter_next` and decodes it with `loom_row_open`, so a large
/// result is never materialized. Each `next()` yields a one-row `result` whose single row (item 0,
/// row 0) carries the cells: `while (auto row = stream.next()) { auto c = row->cell(0, 0, 0); }`.
class row_stream {
public:
    explicit row_stream(LoomIter *it) : it_(it) {}
    ~row_stream() { ::loom_iter_free(it_); }

    row_stream(const row_stream &) = delete;
    row_stream &operator=(const row_stream &) = delete;
    row_stream(row_stream &&other) noexcept : it_(other.it_) { other.it_ = nullptr; }
    row_stream &operator=(row_stream &&other) noexcept {
        if (this != &other) {
            ::loom_iter_free(it_);
            it_ = other.it_;
            other.it_ = nullptr;
        }
        return *this;
    }

    /// The next row as a one-row `result` (read cells with `r->cell(0, 0, col)`, `r->row_len(0, 0)` for
    /// the column count), or `std::nullopt` at the end of the stream.
    std::optional<result> next() {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t done = 0;
        detail::check(::loom_iter_next(it_, &ptr, &len, &done));
        if (done != 0) {
            return std::nullopt;
        }
        LoomResultView *view = nullptr;
        std::int32_t status = ::loom_row_open(ptr, len, &view);
        ::loom_bytes_free(ptr, len);
        detail::check(status);
        return result(view);
    }

private:
    LoomIter *it_ = nullptr;
};

}  // namespace uldren::loom
