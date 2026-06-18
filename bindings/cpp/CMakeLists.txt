cmake_minimum_required(VERSION 3.16)
project(uldren_loom_cpp CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(LOOM_LIB_DIR "${CMAKE_SOURCE_DIR}/../../target/release" CACHE PATH "Dir containing libuldren_loom")

find_library(LOOM_LIB NAMES uldren_loom PATHS "${LOOM_LIB_DIR}" REQUIRED)

function(add_loom_cpp_target target source)
    add_executable(${target} ${source})
    target_include_directories(${target} PRIVATE
        "${CMAKE_SOURCE_DIR}/include"
        "${CMAKE_SOURCE_DIR}/../../include"
    )
    target_link_libraries(${target} PRIVATE "${LOOM_LIB}")
    set_target_properties(${target} PROPERTIES BUILD_RPATH "${LOOM_LIB_DIR}")
endfunction()

add_loom_cpp_target(loom_example example/main.cpp)
add_loom_cpp_target(loom_cpp_runtime_test test/main.cpp)

enable_testing()
add_test(NAME loom_cpp_runtime COMMAND loom_cpp_runtime_test)
