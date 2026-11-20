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
target_include_directories(css_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}" "${TW_ROOT}/litehtml/include")

add_executable(paint_oracle paint_oracle.cpp "${TW_CORE}/paint_policy.cpp")
target_include_directories(paint_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}" "${TW_ROOT}/litehtml/include")

add_executable(compositor_oracle compositor_oracle.cpp "${TW_CORE}/compositor.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(compositor_oracle PRIVATE "${TW_CORE}")

add_executable(base64_oracle base64_oracle.cpp "${TW_CORE}/encode.cpp")
target_include_directories(base64_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")

add_executable(canvas_oracle canvas_oracle.cpp "${TW_CORE}/canvas.cpp" "${TW_CORE}/css.cpp" "${TW_CORE}/wire.cpp" "${TW_CORE}/encode.cpp")
target_include_directories(canvas_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}" "${TW_ROOT}/litehtml/include")

add_executable(animations_oracle animations_oracle.cpp "${TW_CORE}/animations.cpp" "${TW_CORE}/css.cpp" "${TW_CORE}/paint_policy.cpp")
target_include_directories(animations_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}" "${TW_ROOT}/litehtml/include")

add_executable(filters_oracle filters_oracle.cpp "${TW_CORE}/filters.cpp")
target_include_directories(filters_oracle PRIVATE "${TW_CORE}")

add_executable(png_oracle png_oracle.cpp "${TW_CORE}/png.cpp")
target_include_directories(png_oracle PRIVATE "${TW_CORE}")

add_executable(encode_oracle encode_oracle.cpp "${TW_CORE}/encode.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(encode_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")

# ends_oracle: renderer_end.cpp copied next to the scripted renderer.h of ends_seam, so that its quoted include of renderer.h finds that one; the rest from core/src.
set(ENDS_SEAM "${CMAKE_CURRENT_BINARY_DIR}/ends_seam")
configure_file("${TW_CORE}/renderer_end.h" "${ENDS_SEAM}/renderer_end.h" COPYONLY)
configure_file("${TW_CORE}/renderer_end.cpp" "${ENDS_SEAM}/renderer_end.cpp" COPYONLY)
configure_file("${CMAKE_CURRENT_SOURCE_DIR}/ends_seam/renderer.h" "${ENDS_SEAM}/renderer.h" COPYONLY)
add_executable(ends_oracle ends_oracle.cpp "${ENDS_SEAM}/renderer_end.cpp" "${TW_CORE}/platform_end.cpp" "${TW_CORE}/remote.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(ends_oracle PRIVATE "${ENDS_SEAM}" "${TW_CORE}" "${TW_ROOT}/tests" "${TW_SANDBOX_INCLUDE}")

# lhdump: the litehtml fork compiled as twidget's CMakeLists.txt compiles it, every source with no definitions and its warnings off; the linker takes the objects the dump reaches.
enable_language(C)
set(TW_LITEHTML "${TW_ROOT}/litehtml")
file(GLOB LHDUMP_GUMBO_SOURCES CONFIGURE_DEPENDS "${TW_LITEHTML}/src/gumbo/*.c")
file(GLOB LHDUMP_LITEHTML_SOURCES CONFIGURE_DEPENDS "${TW_LITEHTML}/src/*.cpp")
add_library(lhdump_litehtml STATIC ${LHDUMP_GUMBO_SOURCES} ${LHDUMP_LITEHTML_SOURCES})
target_include_directories(lhdump_litehtml
    PUBLIC "${TW_LITEHTML}/include" "${TW_LITEHTML}/include/litehtml" "${TW_LITEHTML}/gumbo-include"
    PRIVATE "${TW_LITEHTML}/src")
if (MSVC)
    # gumbo expects POSIX strings.h; a per-language include directory is out of reach of the Visual Studio generators.
    set_source_files_properties(${LHDUMP_GUMBO_SOURCES} PROPERTIES INCLUDE_DIRECTORIES "${TW_LITEHTML}/src/gumbo/visualc/include")
    target_compile_options(lhdump_litehtml PRIVATE /w)
else()
    target_compile_options(lhdump_litehtml PRIVATE -w)
endif()
# lhdump's grid probe (stage L11): render_grid.cpp copied with a call of lhdump_grid.h's record() as _render_content returns, compiled into lhdump, whose own definition keeps the library's render_grid object out of its link.
include("${CMAKE_CURRENT_SOURCE_DIR}/probe.cmake")
set(TW_PROBE_WHAT lhdump)
file(READ "${TW_LITEHTML}/src/render_grid.cpp" LHDUMP_GRID_CPP)
string(REPLACE "\r\n" "\n" LHDUMP_GRID_CPP "${LHDUMP_GRID_CPP}")
tw_probe(LHDUMP_GRID_CPP 1 [=[#include "html_tag.h"]=] [=[#include "html_tag.h"
#include "lhdump_grid.h"]=])
tw_probe(LHDUMP_GRID_CPP 1 [=[    return ret;
}]=] [=[    lhdump_grid::record(this, available, column_gap, definite_height, container_height, row_gap, explicit_columns, explicit_rows, columns, rows, cells, column_x, row_y, row_start, row_between, ret);
    return ret;
}]=])
set(LHDUMP_GRID "${CMAKE_CURRENT_BINARY_DIR}/lhdump_grid")
file(WRITE "${LHDUMP_GRID}/render_grid.cpp.tmp" "${LHDUMP_GRID_CPP}")
configure_file("${LHDUMP_GRID}/render_grid.cpp.tmp" "${LHDUMP_GRID}/render_grid.cpp" COPYONLY)
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${TW_LITEHTML}/src/render_grid.cpp")
# The renderer's colour-scheme rewrite, copied into lhdump, reads the core's CSS helpers.
add_executable(lhdump lhdump.cpp "${TW_CORE}/css.cpp" "${LHDUMP_GRID}/render_grid.cpp")
target_include_directories(lhdump PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}" "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(lhdump PRIVATE lhdump_litehtml)

# shadow_oracle: the whole renderer, its sources as twidget's CMakeLists.txt builds twidget_core, on the lhdump_litehtml objects; its UA layer generated from widget-sandbox's profile.json the same way.
file(READ "${TWIDGET_WIDGET_SANDBOX}/packages/core/profile.json" TW_PROFILE)
string(JSON TW_UA_STYLESHEET GET "${TW_PROFILE}" tables uaStylesheet)
set(TW_GENERATED "${CMAKE_CURRENT_BINARY_DIR}/generated")
file(WRITE "${TW_GENERATED}/tw_ua_stylesheet.h.tmp"
    "#pragma once\n"
    "inline constexpr const char* TW_UA_STYLESHEET = R\"tw-ua(${TW_UA_STYLESHEET})tw-ua\";\n")
configure_file("${TW_GENERATED}/tw_ua_stylesheet.h.tmp" "${TW_GENERATED}/tw_ua_stylesheet.h" COPYONLY)
add_library(oracle_core STATIC
    "${TW_CORE}/renderer.cpp" "${TW_CORE}/computed_style.cpp" "${TW_CORE}/css.cpp" "${TW_CORE}/animations.cpp" "${TW_CORE}/canvas.cpp"
    "${TW_CORE}/compositor.cpp" "${TW_CORE}/encode.cpp" "${TW_CORE}/filters.cpp" "${TW_CORE}/paint_policy.cpp" "${TW_CORE}/png.cpp"
    "${TWIDGET_WIDGET_SANDBOX}/packages/desktop/sandbox/src/gesture_policy.cpp")
target_include_directories(oracle_core PUBLIC "${TW_CORE}" "${TW_ROOT}/core/include" "${TW_SANDBOX_INCLUDE}" PRIVATE "${TW_GENERATED}")
target_link_libraries(oracle_core PUBLIC lhdump_litehtml)
target_compile_definitions(oracle_core PUBLIC TW_LITEHTML_TWIDGET)
if (MSVC)
    target_compile_definitions(oracle_core PRIVATE _CRT_SECURE_NO_WARNINGS NOMINMAX)
endif()
add_executable(shadow_oracle shadow_oracle.cpp "${TW_CORE}/wire.cpp")
target_link_libraries(shadow_oracle PRIVATE oracle_core)
# computed_style.cpp included whole, so that its helpers in an unnamed namespace can be called, over the same litehtml as lhdump.
add_executable(computed_style_oracle computed_style_oracle.cpp "${TW_CORE}/css.cpp")
target_include_directories(computed_style_oracle PRIVATE "${TW_CORE}" "${TW_SANDBOX_INCLUDE}")
target_link_libraries(computed_style_oracle PRIVATE lhdump_litehtml)
# svg_oracle: el_svg.cpp included whole, so that its helpers in an unnamed namespace and the painter's private members can be reached, over the same litehtml as lhdump; its own definitions keep the library's el_svg object out of the link.
add_executable(svg_oracle svg_oracle.cpp)
target_include_directories(svg_oracle PRIVATE "${TW_LITEHTML}/src")
target_link_libraries(svg_oracle PRIVATE lhdump_litehtml)
if (MSVC)
    target_compile_definitions(svg_oracle PRIVATE _CRT_SECURE_NO_WARNINGS)
endif()
# events_oracle: the renderer with probes at its input code (events_oracle.cmake).
include("${CMAKE_CURRENT_SOURCE_DIR}/events_oracle.cmake")
# renderer_oracle: the renderer with probes at its calls into the document and stage 4.5's code (renderer_oracle.cmake).
include("${CMAKE_CURRENT_SOURCE_DIR}/renderer_oracle.cmake")
