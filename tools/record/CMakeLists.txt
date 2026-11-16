# widget-sandbox corpus widgets recorded for twidget_parity_corpus: each widget through the desktop host
# (QuickJS, the validator, WidgetHost) on a recorded TwidgetCore over CoreGraphics, with a fixed input
# script, into one recording that twidget_replay replays without the engine (tests/recording.h).
#   cmake -S tools/record -B build-record -G Ninja -DCMAKE_BUILD_TYPE=Debug && cmake --build build-record
#   build-record/twidget-record --out tests/recordings/corpus.twrec <corpus path>...
# widget-sandbox is found as twidget's own build finds it: next to twidget, or TWIDGET_WIDGET_SANDBOX.
cmake_minimum_required(VERSION 3.22)
project(twidget-record C CXX)

if(NOT APPLE OR CMAKE_SYSTEM_NAME STREQUAL "iOS")
    message(FATAL_ERROR "the recorder measures and draws with CoreGraphics: macOS only")
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

add_executable(twidget-record main.cpp platform_backend.cpp)
target_include_directories(twidget-record PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/../../tests")
target_link_libraries(twidget-record PRIVATE twidget_litehtml twidget-sandbox twidget::remote)
target_compile_definitions(twidget-record PRIVATE
    TW_SANDBOX="${TWIDGET_WIDGET_SANDBOX}"
    TW_FRAMEWORK="${TW_FRAMEWORK}"
    TW_COMMIT="${TW_COMMIT}")
