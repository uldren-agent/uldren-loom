#pragma once
#include "prelude.hpp"

namespace uldren::loom {

/// Thrown when a C-ABI call returns a non-zero status. Carries the stable numeric `code` and the
/// message from `loom_last_error`.
class error : public std::runtime_error {
public:
    error(int code, std::string message) : std::runtime_error(std::move(message)), code(code) {}
    int code;  // the stable Code; 0 is never an error
};

}  // namespace uldren::loom
