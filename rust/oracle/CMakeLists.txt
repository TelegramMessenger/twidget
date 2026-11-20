# The C++ the Rust port is compared with, built from twidget's own sources: one small program per
# stage that reads cases on stdin and writes what the C++ makes of them on stdout (rust/PLAN.md, T4).
cmake_minimum_required(VERSION 3.20)
project(twidget_oracle CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

get_filename_component(TW_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.." ABSOLUTE)
set(TW_CORE "${TW_ROOT}/core/src")

if (MSVC)
    add_compile_options(/utf-8 /W3 /fp:precise)
else()
    # Rust never fuses a*b+c; the C++ must not either for the two to be compared bit for bit.
    add_compile_options(-ffp-contract=off)
endif()

add_executable(wire_oracle wire_oracle.cpp "${TW_CORE}/wire.cpp")
target_include_directories(wire_oracle PRIVATE "${TW_CORE}")
