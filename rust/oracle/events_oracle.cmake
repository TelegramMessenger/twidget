# events_oracle: renderer.cpp and renderer.h copied with the probes of events_probe.h at the input code's calls (each pattern found exactly as often as written, or the configure stops), the copy included whole by the driver over the rest of the core.
set(EVENTS_SEAM "${CMAKE_CURRENT_BINARY_DIR}/events_seam")
file(READ "${TW_CORE}/renderer.cpp" EVENTS_CPP)
file(READ "${TW_CORE}/renderer.h" EVENTS_H)
string(REPLACE "\r\n" "\n" EVENTS_CPP "${EVENTS_CPP}")
string(REPLACE "\r\n" "\n" EVENTS_H "${EVENTS_H}")

include("${CMAKE_CURRENT_SOURCE_DIR}/probe.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/input_probes.cmake")
set(TW_PROBE_WHAT events_oracle)
tw_input_probes(EVENTS_CPP EVENTS_H events_probe.h EventsOracle)

file(WRITE "${EVENTS_SEAM}/renderer.cpp.tmp" "${EVENTS_CPP}")
file(WRITE "${EVENTS_SEAM}/renderer.h.tmp" "${EVENTS_H}")
configure_file("${EVENTS_SEAM}/renderer.cpp.tmp" "${EVENTS_SEAM}/renderer.cpp" COPYONLY)
configure_file("${EVENTS_SEAM}/renderer.h.tmp" "${EVENTS_SEAM}/renderer.h" COPYONLY)
add_library(events_core STATIC
    "${TW_CORE}/computed_style.cpp" "${TW_CORE}/css.cpp" "${TW_CORE}/animations.cpp" "${TW_CORE}/canvas.cpp" "${TW_CORE}/compositor.cpp"
    "${TW_CORE}/encode.cpp" "${TW_CORE}/filters.cpp" "${TW_CORE}/paint_policy.cpp" "${TW_CORE}/png.cpp" "${TW_CORE}/wire.cpp"
    "${TWIDGET_WIDGET_SANDBOX}/packages/desktop/sandbox/src/gesture_policy.cpp")
target_include_directories(events_core PUBLIC "${TW_CORE}" "${TW_ROOT}/core/include" "${TW_SANDBOX_INCLUDE}")
target_link_libraries(events_core PUBLIC lhdump_litehtml)
target_compile_definitions(events_core PUBLIC TW_LITEHTML_TWIDGET)
if (MSVC)
    target_compile_definitions(events_core PUBLIC _CRT_SECURE_NO_WARNINGS NOMINMAX)
endif()
# events_probe.h beside this file is a dependency MSBuild does not see: the copy is written again when it changes.
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/events_probe.h" "${CMAKE_CURRENT_SOURCE_DIR}/probe.cmake" "${CMAKE_CURRENT_SOURCE_DIR}/input_probes.cmake")
add_executable(events_oracle events_oracle.cpp)
target_include_directories(events_oracle BEFORE PRIVATE "${EVENTS_SEAM}" "${CMAKE_CURRENT_SOURCE_DIR}" "${TW_GENERATED}")
target_link_libraries(events_oracle PRIVATE events_core)
