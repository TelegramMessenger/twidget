# The portable half of twidget for every platform but Apple's SwiftPM build: the litehtml fork and
# TwidgetCore, the C++ renderer behind widget-sandbox's renderer protocol. A platform links
# twidget::core and implements twlite::Backend (fonts, text, images, canvas) and a painter for the
# display list.
cmake_minimum_required(VERSION 3.22)
project(twidget C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

option(TWIDGET_BUILD_TESTS "Build the TwidgetCore checks" ${PROJECT_IS_TOP_LEVEL})

set(TW_LITEHTML "${CMAKE_CURRENT_SOURCE_DIR}/Sources/LiteHTML/litehtml")
set(TW_CORE "${CMAKE_CURRENT_SOURCE_DIR}/Sources/TwidgetCore")

file(GLOB TW_GUMBO_SOURCES CONFIGURE_DEPENDS "${TW_LITEHTML}/src/gumbo/*.c")
file(GLOB TW_LITEHTML_SOURCES CONFIGURE_DEPENDS "${TW_LITEHTML}/src/*.cpp")
add_library(twidget_litehtml STATIC ${TW_GUMBO_SOURCES} ${TW_LITEHTML_SOURCES})
target_include_directories(twidget_litehtml
    PUBLIC "${TW_LITEHTML}/include" "${TW_LITEHTML}/include/litehtml" "${TW_LITEHTML}/gumbo-include"
    PRIVATE "${TW_LITEHTML}/src")
if(MSVC)
    # gumbo expects POSIX strings.h; litehtml is compiled as it comes, without its warnings.
    target_include_directories(twidget_litehtml PRIVATE "$<$<COMPILE_LANGUAGE:C>:${TW_LITEHTML}/src/gumbo/visualc/include>")
    target_compile_options(twidget_litehtml PRIVATE /utf-8 /w)
else()
    target_compile_options(twidget_litehtml PRIVATE -w)
endif()

add_library(twidget_core STATIC
    "${TW_CORE}/src/renderer.cpp"
    "${TW_CORE}/src/css.cpp"
    "${TW_CORE}/src/animations.cpp"
    "${TW_CORE}/src/canvas.cpp"
    "${TW_CORE}/src/compositor.cpp")
add_library(twidget::core ALIAS twidget_core)
target_include_directories(twidget_core PUBLIC "${TW_CORE}/src" "${TW_CORE}/include")
target_link_libraries(twidget_core PUBLIC twidget_litehtml)
target_compile_definitions(twidget_core PUBLIC TW_LITEHTML_TWIDGET)
if(MSVC)
    target_compile_options(twidget_core PRIVATE /utf-8 /W3)
    target_compile_definitions(twidget_core PRIVATE _CRT_SECURE_NO_WARNINGS NOMINMAX)
else()
    target_compile_options(twidget_core PRIVATE -Wall -Wextra -Wno-unused-parameter)
endif()

if(TWIDGET_BUILD_TESTS)
    enable_testing()
    add_executable(twidget_core_tests "${CMAKE_CURRENT_SOURCE_DIR}/Tests/TwidgetCoreTests/core_tests.cpp")
    target_link_libraries(twidget_core_tests PRIVATE twidget::core)
    if(MSVC)
        target_compile_options(twidget_core_tests PRIVATE /utf-8)
    endif()
    add_test(NAME twidget_core COMMAND twidget_core_tests)
endif()
