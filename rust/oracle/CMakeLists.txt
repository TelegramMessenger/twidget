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

# widget-sandbox next to twidget, as twidget's own build expects it.
set(TWIDGET_WIDGET_SANDBOX "${TW_ROOT}/../widget-sandbox" CACHE PATH "widget-sandbox checkout")
set(TW_SANDBOX_INCLUDE "${TWIDGET_WIDGET_SANDBOX}/packages/desktop/sandbox/include")
if (NOT EXISTS "${TW_SANDBOX_INCLUDE}/twidget/json.h")
    message(FATAL_ERROR "widget-sandbox is not at ${TWIDGET_WIDGET_SANDBOX}: set TWIDGET_WIDGET_SANDBOX.")
endif()


add_executable(remote_oracle remote_oracle.cpp "${TW_CORE}/remote.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(remote_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")

add_executable(css_oracle css_oracle.cpp "${TW_CORE}/css.cpp")
target_include_directories(css_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")

add_executable(paint_oracle paint_oracle.cpp "${TW_CORE}/paint_policy.cpp")
target_include_directories(paint_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")

add_executable(compositor_oracle compositor_oracle.cpp "${TW_CORE}/compositor.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(compositor_oracle PRIVATE "${TW_CORE}")

add_executable(base64_oracle base64_oracle.cpp "${TW_CORE}/encode.cpp")
target_include_directories(base64_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")

add_executable(canvas_oracle canvas_oracle.cpp "${TW_CORE}/canvas.cpp" "${TW_CORE}/css.cpp" "${TW_CORE}/wire.cpp" "${TW_CORE}/encode.cpp")
target_include_directories(canvas_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")

add_executable(animations_oracle animations_oracle.cpp "${TW_CORE}/animations.cpp" "${TW_CORE}/css.cpp" "${TW_CORE}/paint_policy.cpp")
target_include_directories(animations_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")
