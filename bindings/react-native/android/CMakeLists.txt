cmake_minimum_required(VERSION 3.22)
project(uldren_loom_rn CXX)

# JNI bridge for the React Native TurboModule, over the Uldren Loom C ABI. Links the per-ABI Rust
# static lib that cargo-ndk builds into target/<rust-triple>/release/ (see README.md).
add_library(uldren_loom_rn SHARED
    src/main/cpp/UldrenLoom_common.cpp
    src/main/cpp/UldrenLoom_lifecycle.cpp
    src/main/cpp/UldrenLoom_workspace.cpp
    src/main/cpp/UldrenLoom_identity.cpp
    src/main/cpp/UldrenLoom_sql.cpp
    src/main/cpp/UldrenLoom_vcs.cpp
    src/main/cpp/UldrenLoom_queue.cpp
    src/main/cpp/UldrenLoom_cas.cpp
    src/main/cpp/UldrenLoom_meetings.cpp
    src/main/cpp/UldrenLoom_drive.cpp
    src/main/cpp/UldrenLoom_tickets.cpp
    src/main/cpp/UldrenLoom_pages.cpp
    src/main/cpp/UldrenLoom_lanes.cpp
    src/main/cpp/UldrenLoom_chat.cpp
    src/main/cpp/UldrenLoom_archive.cpp
    src/main/cpp/UldrenLoom_kv.cpp
    src/main/cpp/UldrenLoom_document.cpp
    src/main/cpp/UldrenLoom_timeseries.cpp
    src/main/cpp/UldrenLoom_telemetry.cpp
    src/main/cpp/UldrenLoom_ledger.cpp
    src/main/cpp/UldrenLoom_calendar.cpp
    src/main/cpp/UldrenLoom_contacts.cpp
    src/main/cpp/UldrenLoom_mail.cpp
    src/main/cpp/UldrenLoom_graph.cpp
    src/main/cpp/UldrenLoom_vector.cpp
    src/main/cpp/UldrenLoom_columnar.cpp
    src/main/cpp/UldrenLoom_dataframe.cpp
    src/main/cpp/UldrenLoom_search.cpp)
target_include_directories(uldren_loom_rn PRIVATE ${CMAKE_SOURCE_DIR}/../../../include)

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

find_library(log-lib log)
target_link_libraries(uldren_loom_rn
    ${CMAKE_SOURCE_DIR}/../../../target/${RUST_TARGET}/release/libuldren_loom.a
    ${log-lib})
