# Sample CMake build for the Uldren Loom C++ example.
# Assumes libuldren_loom has been built (cargo build -p uldren-loom-ffi) and the C header is at
# ../../include/loom.h. Licensed under BUSL-1.1.
cmake_minimum_required(VERSION 3.16)
project(uldren_loom_cpp CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Path to the cargo-built native library (override with -DLOOM_LIB_DIR=...).
set(LOOM_LIB_DIR "${CMAKE_SOURCE_DIR}/../../target/release" CACHE PATH "Dir containing libuldren_loom")

add_executable(loom_example example/main.cpp)
target_include_directories(loom_example PRIVATE
    "${CMAKE_SOURCE_DIR}/include"        # loom.hpp
    "${CMAKE_SOURCE_DIR}/../../include"  # loom.h
)
find_library(LOOM_LIB NAMES uldren_loom PATHS "${LOOM_LIB_DIR}" REQUIRED)
target_link_libraries(loom_example PRIVATE "${LOOM_LIB}")
