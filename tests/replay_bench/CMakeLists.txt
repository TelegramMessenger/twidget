cmake_minimum_required(VERSION 3.22)
project(twidget_replay_bench C CXX)

# Build the same checked replay driver against any compatible historical renderer checkout.
set(TWIDGET_REPLAY_CORE "${CMAKE_CURRENT_LIST_DIR}/../.." CACHE PATH "twidget C++ source checkout")
set(TWIDGET_BUILD_TESTS OFF)
if(NOT MSVC AND CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    add_compile_options(-ffp-contract=off)
endif()
add_subdirectory("${TWIDGET_REPLAY_CORE}" renderer)

add_executable(twidget_replay ../replay.cpp)
target_compile_features(twidget_replay PRIVATE cxx_std_20)
target_link_libraries(twidget_replay PRIVATE twidget::renderer_end)
if(MSVC)
    target_compile_options(twidget_replay PRIVATE /utf-8)
endif()
