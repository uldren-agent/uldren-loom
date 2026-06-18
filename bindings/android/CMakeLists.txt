cmake_minimum_required(VERSION 3.22)
project(uldren_loom_jni C)

add_library(uldren_loom_jni SHARED native/uldren_loom_jni.c)
target_include_directories(uldren_loom_jni PRIVATE ${CMAKE_SOURCE_DIR}/../../include)

if(ANDROID)
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
else()
    find_package(JNI REQUIRED)
    set(LOOM_NATIVE_DIR "${CMAKE_SOURCE_DIR}/../../target/release" CACHE PATH "Directory containing libuldren_loom")
    find_library(ULDREN_LOOM_LIB NAMES uldren_loom libuldren_loom
        PATHS "${LOOM_NATIVE_DIR}"
        NO_DEFAULT_PATH)
    if(NOT ULDREN_LOOM_LIB)
        message(FATAL_ERROR "libuldren_loom not found in ${LOOM_NATIVE_DIR}")
    endif()

    target_include_directories(uldren_loom_jni PRIVATE ${JNI_INCLUDE_DIRS})
    target_link_libraries(uldren_loom_jni ${JNI_LIBRARIES} ${ULDREN_LOOM_LIB})
    set_target_properties(uldren_loom_jni PROPERTIES
        BUILD_RPATH "${LOOM_NATIVE_DIR}"
        INSTALL_RPATH "${LOOM_NATIVE_DIR}")
endif()
