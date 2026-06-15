cmake_minimum_required(VERSION 3.22)
project(uldren_loom_jni C)

# JNI shim over the Uldren Loom C ABI. Links the per-ABI Rust static lib that cargo-ndk builds
# into target/<rust-triple>/release/ (see README.md).
add_library(uldren_loom_jni SHARED native/uldren_loom_jni.c)
target_include_directories(uldren_loom_jni PRIVATE ${CMAKE_SOURCE_DIR}/../../include)

# Map the Android ABI to the Rust target triple so we can link the right static library.
if(ANDROID_ABI STREQUAL "arm64-v8a")
    set(RUST_TARGET aarch64-linux-android)
elseif(ANDROID_ABI STREQUAL "armeabi-v7a")
    set(RUST_TARGET armv7-linux-androideabi)
elseif(ANDROID_ABI STREQUAL "x86_64")
    set(RUST_TARGET x86_64-linux-android)
elseif(ANDROID_ABI STREQUAL "x86")
    set(RUST_TARGET i686-linux-android)
else()
    message(FATAL_ERROR "Unsupported ANDROID_ABI: ${ANDROID_ABI}")
endif()

find_library(LOG_LIB log)
target_link_libraries(uldren_loom_jni
    ${CMAKE_SOURCE_DIR}/../../target/${RUST_TARGET}/release/libuldren_loom.a
    ${LOG_LIB})
