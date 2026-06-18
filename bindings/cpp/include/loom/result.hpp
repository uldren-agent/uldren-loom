#pragma once
#include "value.hpp"

namespace uldren::loom {

/// A decoded, immutable, indexed result (RAII over `LoomResultView`). Built by `sql::exec`; navigate it
/// with the indexed accessors (mirroring the C result-view ABI) and read cells as `value`. One decoder
/// (the shared `result_view`) backs every C-ABI binding, so no CBOR is parsed here. Throws on a bad
/// index / type.
class result {
public:
    explicit result(LoomResultView *view) : view_(view) {}
    ~result() { ::loom_result_close(view_); }

    result(const result &) = delete;
    result &operator=(const result &) = delete;
    result(result &&other) noexcept : view_(other.view_) { other.view_ = nullptr; }
    result &operator=(result &&other) noexcept {
        if (this != &other) {
            ::loom_result_close(view_);
            view_ = other.view_;
            other.view_ = nullptr;
        }
        return *this;
    }

    /// Number of items (SQL statements, or 1 for a reader result).
    std::size_t len() const { return ::loom_result_len(view_); }
    /// True if this result is a list of SQL statements (vs a single reader result).
    bool is_statements() const { return ::loom_result_is_statements(view_) == 1; }
    /// The kind of item `item` (a `LOOM_RESULT_*` value).
    int item_kind(std::size_t item) const { return ::loom_result_item_kind(view_, item); }

    std::size_t column_count(std::size_t item) const { return ::loom_result_column_count(view_, item); }
    std::string column_name(std::size_t item, std::size_t col) const {
        const std::uint8_t *p = nullptr;
        std::uintptr_t l = 0;
        detail::check(::loom_result_column_name(view_, item, col, &p, &l));
        return to_str(p, l);
    }
    std::string column_type(std::size_t item, std::size_t col) const {
        const std::uint8_t *p = nullptr;
        std::uintptr_t l = 0;
        detail::check(::loom_result_column_type(view_, item, col, &p, &l));
        return to_str(p, l);
    }

    std::size_t row_count(std::size_t item) const { return ::loom_result_row_count(view_, item); }
    std::size_t row_len(std::size_t item, std::size_t row) const {
        return ::loom_result_row_len(view_, item, row);
    }
    value cell(std::size_t item, std::size_t row, std::size_t col) const {
        LoomValue v;
        detail::check(::loom_result_cell(view_, item, row, col, &v));
        return value(v);
    }

    /// A lightweight view of one row of item `item`: `size()` columns, `[col]` reads a cell.
    class row_view {
    public:
        row_view(const result *r, std::size_t item, std::size_t row) : r_(r), item_(item), row_(row) {}
        std::size_t size() const { return r_->row_len(item_, row_); }
        value operator[](std::size_t col) const { return r_->cell(item_, row_, col); }

    private:
        const result *r_;
        std::size_t item_;
        std::size_t row_;
    };

    /// A forward range over the rows of item `item`, so a `result` iterates idiomatically:
    /// `for (auto row : res.rows()) { ... row[0] ... }` (over the
    /// already-decoded typed result; no extra ABI call per row).
    class row_range {
    public:
        row_range(const result *r, std::size_t item, std::size_t count)
            : r_(r), item_(item), count_(count) {}
        class iterator {
        public:
            iterator(const result *r, std::size_t item, std::size_t row)
                : r_(r), item_(item), row_(row) {}
            row_view operator*() const { return row_view(r_, item_, row_); }
            iterator &operator++() {
                ++row_;
                return *this;
            }
            bool operator!=(const iterator &o) const { return row_ != o.row_; }

        private:
            const result *r_;
            std::size_t item_;
            std::size_t row_;
        };
        iterator begin() const { return iterator(r_, item_, 0); }
        iterator end() const { return iterator(r_, item_, count_); }

    private:
        const result *r_;
        std::size_t item_;
        std::size_t count_;
    };

    /// Iterate the rows of item `item` (default 0, the first/only result).
    row_range rows(std::size_t item = 0) const { return row_range(this, item, row_count(item)); }

    /// Row count of an Insert/Delete/Update/DropTable item.
    std::uint64_t count(std::size_t item) const {
        std::uint64_t n = 0;
        detail::check(::loom_result_count(view_, item, &n));
        return n;
    }

    std::size_t string_count(std::size_t item) const { return ::loom_result_string_count(view_, item); }
    std::string string(std::size_t item, std::size_t i) const {
        const std::uint8_t *p = nullptr;
        std::uintptr_t l = 0;
        detail::check(::loom_result_string(view_, item, i, &p, &l));
        return to_str(p, l);
    }
    /// ShowVariable variable kind (`LOOM_VARIABLE_*`).
    int variable_kind(std::size_t item) const {
        int k = 0;
        detail::check(::loom_result_variable_kind(view_, item, &k));
        return k;
    }

    /// Commit address of blame row `row`.
    std::string row_commit(std::size_t item, std::size_t row) const {
        const std::uint8_t *p = nullptr;
        std::uintptr_t l = 0;
        detail::check(::loom_result_row_commit(view_, item, row, &p, &l));
        return to_str(p, l);
    }

    std::size_t diff_count(std::size_t item) const { return ::loom_result_diff_count(view_, item); }
    /// Diff change kind (`LOOM_DIFF_*`).
    int diff_change(std::size_t item, std::size_t entry) const {
        int c = 0;
        detail::check(::loom_result_diff_change(view_, item, entry, &c));
        return c;
    }
    std::size_t diff_len(std::size_t item, std::size_t entry, int side) const {
        return ::loom_result_diff_len(view_, item, entry, side);
    }
    value diff_cell(std::size_t item, std::size_t entry, int side, std::size_t col) const {
        LoomValue v;
        detail::check(::loom_result_diff_cell(view_, item, entry, side, col, &v));
        return value(v);
    }

    /// Merge outcome (`LOOM_MERGE_*`).
    int merge_outcome(std::size_t item) const {
        int o = 0;
        detail::check(::loom_result_merge_outcome(view_, item, &o));
        return o;
    }

    std::size_t map_len(std::size_t item, std::size_t row) const {
        return ::loom_result_map_len(view_, item, row);
    }
    std::pair<std::string, value> map_entry(std::size_t item, std::size_t row, std::size_t idx) const {
        const std::uint8_t *p = nullptr;
        std::uintptr_t l = 0;
        LoomValue v;
        detail::check(::loom_result_map_entry(view_, item, row, idx, &p, &l, &v));
        return {to_str(p, l), value(v)};
    }

    const LoomResultView *raw() const { return view_; }

private:
    static std::string to_str(const std::uint8_t *p, std::uintptr_t l) {
        return p ? std::string(reinterpret_cast<const char *>(p), l) : std::string();
    }
    LoomResultView *view_ = nullptr;
};

}  // namespace uldren::loom
