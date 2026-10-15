# The portable half of twidget for every platform but Apple's SwiftPM build: the litehtml fork,
# TwidgetCore (the C++ renderer behind widget-sandbox's renderer protocol), and each platform's
# backend. What a build produces depends on the platform:
#   Android      libtwidget_litehtml.so, the JNI surface (platform/android)
#   iOS, macOS   libtwidget_litehtml.a, the C ABI on CoreGraphics (platform/apple), merged by tools/build-apple.sh
#   Emscripten   twidget_litehtml.js and .wasm, the C ABI for a page (platform/web)
#   Windows      twidget_d2d, the Direct2D backend a host paints with (platform/windows)
#   Qt           twidget_qt with -DTWIDGET_QT=ON, the QPainter backend (platform/qt; demo in tools/qt-demo)
cmake_minimum_required(VERSION 3.22)
project(twidget C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

option(TWIDGET_BUILD_TESTS "Build the TwidgetCore checks" ${PROJECT_IS_TOP_LEVEL})
option(TWIDGET_QT "Build twidget_qt, the QPainter backend (needs Qt 6)" OFF)

# widget-sandbox owns the protocol and its JSON helper. Package.swift finds it the same way: a checkout
# next to twidget, or the path in TWIDGET_WIDGET_SANDBOX (a URL there is for SwiftPM only).
set(TWIDGET_WIDGET_SANDBOX "$ENV{TWIDGET_WIDGET_SANDBOX}" CACHE PATH "widget-sandbox checkout")
if(NOT TWIDGET_WIDGET_SANDBOX)
    set(TWIDGET_WIDGET_SANDBOX "${CMAKE_CURRENT_SOURCE_DIR}/../widget-sandbox")
endif()
set(TW_SANDBOX_INCLUDE "${TWIDGET_WIDGET_SANDBOX}/packages/desktop/sandbox/include")
if(NOT EXISTS "${TW_SANDBOX_INCLUDE}/twidget/json.h")
    message(FATAL_ERROR "widget-sandbox not found at ${TWIDGET_WIDGET_SANDBOX}: check it out next to twidget or set TWIDGET_WIDGET_SANDBOX to its path")
endif()

set(TW_LITEHTML "${CMAKE_CURRENT_SOURCE_DIR}/Sources/LiteHTML/litehtml")
set(TW_CORE "${CMAKE_CURRENT_SOURCE_DIR}/Sources/TwidgetCore")
set(TW_PLATFORM "${TW_CORE}/platform")

if(EMSCRIPTEN)
    # The renderer reports failures as exceptions; wasm exceptions keep them cheap.
    add_compile_options(-fwasm-exceptions)
endif()

file(GLOB TW_GUMBO_SOURCES CONFIGURE_DEPENDS "${TW_LITEHTML}/src/gumbo/*.c")
file(GLOB TW_LITEHTML_SOURCES CONFIGURE_DEPENDS "${TW_LITEHTML}/src/*.cpp")
add_library(litehtml STATIC ${TW_GUMBO_SOURCES} ${TW_LITEHTML_SOURCES})
add_library(twidget::litehtml ALIAS litehtml)
target_include_directories(litehtml
    PUBLIC "${TW_LITEHTML}/include" "${TW_LITEHTML}/include/litehtml" "${TW_LITEHTML}/gumbo-include"
    PRIVATE "${TW_LITEHTML}/src")
if(APPLE)
    target_link_libraries(litehtml PUBLIC "-framework CoreFoundation")
endif()
if(MSVC)
    # gumbo expects POSIX strings.h; litehtml is compiled as it comes, without its warnings.
    target_include_directories(litehtml PRIVATE "$<$<COMPILE_LANGUAGE:C>:${TW_LITEHTML}/src/gumbo/visualc/include>")
    target_compile_options(litehtml PRIVATE /utf-8 /w)
else()
    target_compile_options(litehtml PRIVATE -w)
endif()

add_library(twidget_core STATIC
    "${TW_CORE}/src/renderer.cpp"
    "${TW_CORE}/src/css.cpp"
    "${TW_CORE}/src/animations.cpp"
    "${TW_CORE}/src/canvas.cpp"
    "${TW_CORE}/src/compositor.cpp"
    "${TW_CORE}/src/encode.cpp")
add_library(twidget::core ALIAS twidget_core)
target_include_directories(twidget_core PUBLIC "${TW_CORE}/src" "${TW_CORE}/include" "${TW_SANDBOX_INCLUDE}")
target_link_libraries(twidget_core PUBLIC litehtml)
target_compile_definitions(twidget_core PUBLIC TW_LITEHTML_TWIDGET)
if(MSVC)
    target_compile_options(twidget_core PRIVATE /utf-8 /W3)
    target_compile_definitions(twidget_core PRIVATE _CRT_SECURE_NO_WARNINGS NOMINMAX)
else()
    target_compile_options(twidget_core PRIVATE -Wall -Wextra -Wno-unused-parameter)
endif()

if(ANDROID)
    add_library(twidget_litehtml SHARED "${TW_PLATFORM}/android/jni.cpp")
    target_link_libraries(twidget_litehtml PRIVATE twidget_core log)
    target_compile_options(twidget_litehtml PRIVATE -fvisibility=hidden)
    target_link_options(twidget_litehtml PRIVATE -Wl,--gc-sections)
elseif(APPLE)
    add_library(twidget_litehtml STATIC "${TW_PLATFORM}/apple/coregraphics.cpp" "${TW_PLATFORM}/apple/ffi.cpp")
    target_include_directories(twidget_litehtml PUBLIC "${TW_PLATFORM}/apple")
    target_link_libraries(twidget_litehtml PUBLIC twidget_core
        "-framework CoreGraphics" "-framework CoreText" "-framework ImageIO" "-framework CoreFoundation")
elseif(EMSCRIPTEN)
    set(TW_WASM_EXPORTS twl_create twl_backend_id twl_destroy twl_receive twl_set_width twl_set_max_height twl_set_theme twl_touch
        twl_image_ready twl_size twl_dirty twl_display_list twl_damage twl_stats twl_html twl_string_free twl_alloc twl_free
        twl_pointer twl_hover twl_wheel twl_key twl_tick twl_wants_tick twl_next_tick_in twl_set_compositing
        twl_composition twl_clock twl_layer_content)
    list(TRANSFORM TW_WASM_EXPORTS PREPEND "_")
    list(JOIN TW_WASM_EXPORTS "," TW_WASM_EXPORT_LIST)
    add_executable(twidget_litehtml "${TW_PLATFORM}/web/wasm.cpp")
    target_link_libraries(twidget_litehtml PRIVATE twidget_core)
    set_target_properties(twidget_litehtml PROPERTIES SUFFIX ".js")
    target_link_options(twidget_litehtml PRIVATE -fwasm-exceptions
        -sMODULARIZE=1 -sEXPORT_ES6=1 -sENVIRONMENT=web,node -sFILESYSTEM=0 -sALLOW_MEMORY_GROWTH=1 -sINITIAL_MEMORY=32MB
        "-sEXPORTED_FUNCTIONS=${TW_WASM_EXPORT_LIST}"
        -sEXPORTED_RUNTIME_METHODS=UTF8ToString,stringToUTF8,lengthBytesUTF8,HEAPU8,HEAPU32,HEAPF32)
elseif(WIN32)
    add_library(twidget_d2d STATIC "${TW_PLATFORM}/windows/d2d_backend.cpp")
    add_library(twidget::d2d ALIAS twidget_d2d)
    target_include_directories(twidget_d2d PUBLIC "${TW_PLATFORM}/windows")
    target_link_libraries(twidget_d2d PUBLIC twidget_core d2d1 dwrite windowscodecs)
    if(MSVC)
        target_compile_options(twidget_d2d PRIVATE /utf-8 /W3)
        target_compile_definitions(twidget_d2d PRIVATE NOMINMAX)
    endif()
endif()

# Qt hosts (Telegram Desktop) on any desktop OS: fonts, images, canvases and the painter on QPainter.
if(TWIDGET_QT)
    find_package(Qt6 REQUIRED COMPONENTS Gui)
    add_library(twidget_qt STATIC "${TW_PLATFORM}/qt/qt_backend.cpp")
    add_library(twidget::qt ALIAS twidget_qt)
    target_include_directories(twidget_qt PUBLIC "${TW_PLATFORM}/qt")
    target_link_libraries(twidget_qt PUBLIC twidget_core Qt6::Gui)
    if(MSVC)
        target_compile_options(twidget_qt PRIVATE /utf-8 /W3)
        target_compile_definitions(twidget_qt PRIVATE NOMINMAX)
    endif()
endif()

if(TWIDGET_BUILD_TESTS)
    enable_testing()
    add_executable(twidget_core_tests "${CMAKE_CURRENT_SOURCE_DIR}/Tests/TwidgetCoreTests/core_tests.cpp")
    target_link_libraries(twidget_core_tests PRIVATE twidget::core)
    if(MSVC)
        target_compile_options(twidget_core_tests PRIVATE /utf-8)
    endif()
    add_test(NAME twidget_core COMMAND twidget_core_tests)
    # The Swift renderer's FrameCostBenchTests scenarios against the core; run by hand, not by ctest.
    add_executable(twidget_core_bench "${CMAKE_CURRENT_SOURCE_DIR}/Tests/TwidgetCoreTests/bench.cpp")
    target_link_libraries(twidget_core_bench PRIVATE twidget::core)
    if(MSVC)
        target_compile_options(twidget_core_bench PRIVATE /utf-8)
    endif()
    if(TWIDGET_QT)
        add_executable(twidget_qt_tests "${CMAKE_CURRENT_SOURCE_DIR}/Tests/TwidgetCoreTests/qt_tests.cpp")
        target_link_libraries(twidget_qt_tests PRIVATE twidget::qt)
        if(MSVC)
            target_compile_options(twidget_qt_tests PRIVATE /utf-8)
        endif()
        add_test(NAME twidget_qt COMMAND twidget_qt_tests)
    endif()
    if(APPLE AND NOT CMAKE_SYSTEM_NAME STREQUAL "iOS")
        add_executable(twidget_apple_tests "${CMAKE_CURRENT_SOURCE_DIR}/Tests/TwidgetCoreTests/apple_tests.cpp")
        target_link_libraries(twidget_apple_tests PRIVATE twidget_litehtml)
        add_test(NAME twidget_apple COMMAND twidget_apple_tests)
        # Messages from widget-sandbox's sandbox-node.ts in, the paint out as PNG.
        add_executable(twidget-core-run "${CMAKE_CURRENT_SOURCE_DIR}/tools/core-run/run.cpp")
        target_link_libraries(twidget-core-run PRIVATE twidget_litehtml)
    endif()
endif()
