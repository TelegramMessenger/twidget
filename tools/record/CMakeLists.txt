# widget-sandbox corpus widgets recorded for the parity checks: each widget through the desktop host
# (Nova, the validator, WidgetHost) on a recorded TwidgetCore, with a fixed input script, into a recording
# that twidget_replay replays without the engine (tests/recording.h).
#   cmake -S tools/record -B build-record -G Ninja -DCMAKE_BUILD_TYPE=Debug -DTW_NOVA_ROOT=<nova-safe> && cmake --build build-record
#   build-record/twidget-record --out tests/recordings/corpus.twrec <corpus path>...
#   cmake --build build-record --target record-corpus     every corpus widget, each in a recording of its own, in build-record/recordings
# The Backend (TW_RECORD_BACKEND): coregraphics measures and draws with CoreGraphics, macOS only; stub, the
# default elsewhere, answers each call as a fixed function of the calls (stub_backend.cpp).
# widget-sandbox is found as twidget's own build finds it: next to twidget, or TWIDGET_WIDGET_SANDBOX; its
# desktop core builds Nova from TW_NOVA_ROOT (or links TW_NOVA_LIBRARY), see its CMakeLists.txt.
cmake_minimum_required(VERSION 3.22)
project(twidget-record C CXX)

if(CMAKE_SYSTEM_NAME STREQUAL "iOS" OR CMAKE_SYSTEM_NAME STREQUAL "Android" OR EMSCRIPTEN)
    message(FATAL_ERROR "the recorder runs widgets through widget-sandbox's desktop host: a desktop program")
endif()
if(APPLE)
    set(TW_RECORD_BACKEND_DEFAULT coregraphics)
else()
    set(TW_RECORD_BACKEND_DEFAULT stub)
endif()
set(TW_RECORD_BACKEND "${TW_RECORD_BACKEND_DEFAULT}" CACHE STRING "The recorder's Backend: coregraphics (macOS) or stub")
set_property(CACHE TW_RECORD_BACKEND PROPERTY STRINGS coregraphics stub)
if(TW_RECORD_BACKEND STREQUAL "coregraphics")
    if(NOT APPLE)
        message(FATAL_ERROR "TW_RECORD_BACKEND=coregraphics measures and draws with CoreGraphics: macOS only; TW_RECORD_BACKEND=stub records anywhere")
    endif()
elseif(NOT TW_RECORD_BACKEND STREQUAL "stub")
    message(FATAL_ERROR "TW_RECORD_BACKEND is coregraphics or stub, not ${TW_RECORD_BACKEND}")
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(TWIDGET_WIDGET_SANDBOX "$ENV{TWIDGET_WIDGET_SANDBOX}" CACHE PATH "widget-sandbox checkout")
if(NOT TWIDGET_WIDGET_SANDBOX)
    set(TWIDGET_WIDGET_SANDBOX "${CMAKE_CURRENT_SOURCE_DIR}/../../../widget-sandbox" CACHE PATH "widget-sandbox checkout" FORCE)
endif()
set(TWIDGET_BUILD_TESTS OFF CACHE BOOL "" FORCE)
add_subdirectory("${CMAKE_CURRENT_SOURCE_DIR}/../.." twidget)
add_subdirectory("${TWIDGET_WIDGET_SANDBOX}/packages/desktop" widget-sandbox EXCLUDE_FROM_ALL)

# The recording's origin names widget-sandbox's version and twidget's commit as they were at configure time.
find_package(Git REQUIRED)
execute_process(COMMAND "${GIT_EXECUTABLE}" -C "${TWIDGET_WIDGET_SANDBOX}" describe --tags --always --dirty
    OUTPUT_VARIABLE TW_FRAMEWORK OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
execute_process(COMMAND "${GIT_EXECUTABLE}" -C "${CMAKE_CURRENT_SOURCE_DIR}/../.." describe --always --dirty
    OUTPUT_VARIABLE TW_COMMIT OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
if(NOT TW_FRAMEWORK OR NOT TW_COMMIT)
    message(FATAL_ERROR "the recording's origin needs git describe of widget-sandbox (${TWIDGET_WIDGET_SANDBOX}) and of twidget")
endif()

add_executable(twidget-record main.cpp system_version.cpp)
target_include_directories(twidget-record PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/../../tests")
if(TW_RECORD_BACKEND STREQUAL "coregraphics")
    target_sources(twidget-record PRIVATE platform_backend.cpp)
    target_link_libraries(twidget-record PRIVATE twidget_litehtml twidget-sandbox twidget::remote)
else()
    target_sources(twidget-record PRIVATE stub_backend.cpp)
    target_link_libraries(twidget-record PRIVATE twidget::core twidget-sandbox twidget::remote)
endif()
target_compile_definitions(twidget-record PRIVATE
    TW_SANDBOX="${TWIDGET_WIDGET_SANDBOX}"
    TW_FRAMEWORK="${TW_FRAMEWORK}"
    TW_COMMIT="${TW_COMMIT}")
if(MSVC)
    target_compile_options(twidget-record PRIVATE /utf-8)
    target_compile_definitions(twidget-record PRIVATE NOMINMAX _CRT_SECURE_NO_WARNINGS)
endif()

# Every corpus widget, each through its own recorder process into its own recording, a trouble kept (record_corpus.py).
find_package(Python3 COMPONENTS Interpreter)
if(Python3_FOUND)
    add_custom_target(record-corpus
        COMMAND "${Python3_EXECUTABLE}" -I "${CMAKE_CURRENT_SOURCE_DIR}/record_corpus.py" --recorder "$<TARGET_FILE:twidget-record>"
            --sandbox "${TWIDGET_WIDGET_SANDBOX}" --out "${CMAKE_CURRENT_BINARY_DIR}/recordings"
        DEPENDS twidget-record
        USES_TERMINAL
        VERBATIM)
endif()
