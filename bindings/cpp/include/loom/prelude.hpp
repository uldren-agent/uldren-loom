// C++ convenience wrapper over the Uldren Loom C ABI (include/loom.h).
// Header-only RAII helpers that handle the "core allocates, caller frees" ownership rule and the
// int32-status / loom_last_error contract.
// Licensed under BUSL-1.1. (c) Uldren Technologies LLC.
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <future>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "loom.h"
