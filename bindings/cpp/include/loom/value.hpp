#pragma once
#include "detail.hpp"

namespace uldren::loom {

/// One decoded result cell (a thin, faithful view over `LoomValue`). Only the accessors the `tag()`
/// selects are meaningful (`LOOM_VALUE_*`). Text/bytes (and the canonical CBOR of a `LIST`/`MAP`) are
/// borrowed from the owning `result` and valid only while it is alive; the typed accessors copy.
class value {
public:
    explicit value(const LoomValue &v) : v_(v) {}

    int tag() const { return v_.tag; }
    bool is_null() const { return v_.tag == LOOM_VALUE_NULL; }
    /// Signed integer payload: `Bool` (0/1), `Int`/`I8`/`I16`/`I32`, `Date`, `Timestamp`,
    /// and `Interval.months`.
    std::int64_t as_int64() const { return v_.int_val; }
    /// Secondary signed integer: `Interval.micros`.
    std::int64_t as_int64_secondary() const { return v_.int_val2; }
    /// Unsigned integer payload: `U8`/`U16`/`U32`/`U64`, `Time`, and the `Inet` family tag (4 or 6).
    std::uint64_t as_uint64() const { return v_.uint_val; }
    /// Float payload (convenience): `Float`, `F32`, and `Point.x`. See `bits()` for the exact IEEE-754.
    double as_double() const { return v_.float_val; }
    /// `Point.y` (convenience). See `bits_secondary()` for the exact IEEE-754.
    double as_double_secondary() const { return v_.float_val2; }
    /// Raw IEEE-754 bits of the float payload (bit-exact `Float`/`F32`/`Point.x`).
    std::uint64_t bits() const { return v_.bits; }
    /// Raw IEEE-754 bits of `Point.y`.
    std::uint64_t bits_secondary() const { return v_.bits2; }
    /// Decimal scale (with the 16-byte little-endian mantissa from `bytes16()`).
    std::uint32_t scale() const { return v_.scale; }
    /// 16-byte little-endian payload: `I128`/`U128`, `Uuid`, the decimal mantissa, or `Inet` octets.
    std::array<std::uint8_t, 16> bytes16() const {
        std::array<std::uint8_t, 16> a{};
        for (std::size_t i = 0; i < a.size(); ++i) {
            a[i] = v_.bytes16[i];
        }
        return a;
    }
    /// UTF-8 text payload (`Text`; also the `Inet` textual form is not used here, see `bytes16()`).
    std::string text() const {
        return v_.data ? std::string(reinterpret_cast<const char *>(v_.data), v_.data_len)
                       : std::string();
    }
    /// Raw byte payload: `Bytes`, or the canonical CBOR of a `LIST`/`MAP` cell.
    std::vector<std::uint8_t> bytes() const {
        return v_.data ? std::vector<std::uint8_t>(v_.data, v_.data + v_.data_len)
                       : std::vector<std::uint8_t>();
    }
    /// The underlying C struct (for the rare accessor not wrapped above).
    const LoomValue &raw() const { return v_; }

private:
    LoomValue v_;
};

}  // namespace uldren::loom
