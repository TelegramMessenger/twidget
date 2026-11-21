# The widget helper's tdesktop glue (stage H1), compiled from a tdesktop checkout and never copied into twidget's git: its sparse Telegram/SourceFiles/{tgw,test,_other} are enough.
set(TW_TDESKTOP_SOURCE "${TW_ROOT}/../tdesktop" CACHE PATH "tdesktop checkout whose Telegram/SourceFiles the helper's oracles compile")
set(TW_TDESKTOP_FILES "${TW_TDESKTOP_SOURCE}/Telegram/SourceFiles")
set(TW_ORACLE_IO "${TWIDGET_WIDGET_SANDBOX}/packages/rust/oracle")

# The helper's tests read this marker next to the programs: without tdesktop they skip, and say so.
if (NOT EXISTS "${TW_TDESKTOP_FILES}/tgw/tgw_hosted_wire.h")
    message(STATUS "helper oracles: no tdesktop sources at ${TW_TDESKTOP_SOURCE} (set TW_TDESKTOP_SOURCE), their comparisons skip")
    file(GENERATE OUTPUT "$<TARGET_FILE_DIR:wire_oracle>/helper_oracles.txt" CONTENT "absent ${TW_TDESKTOP_SOURCE}\n")
    return()
endif()
file(GENERATE OUTPUT "$<TARGET_FILE_DIR:wire_oracle>/helper_oracles.txt" CONTENT "built ${TW_TDESKTOP_SOURCE}\n")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
    "${TW_TDESKTOP_FILES}/test/test_widget_helper_tap.cpp" "${TW_TDESKTOP_FILES}/test/test_widget_helper_tap.h")

add_executable(hosted_wire_oracle hosted_wire_oracle.cpp "${TW_CORE}/remote.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(hosted_wire_oracle PRIVATE "${TW_TDESKTOP_FILES}" "${TW_ROOT}" "${TW_CORE}" "${TW_SANDBOX_INCLUDE}" "${TW_ORACLE_IO}")

# The rules over helper_seam's renderer.h, which comes first: its build counts and clock are the cases'.
add_executable(renderer_rules_oracle renderer_rules_oracle.cpp)
target_include_directories(renderer_rules_oracle PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/helper_seam" "${TW_TDESKTOP_FILES}" "${TW_ROOT}" "${TW_SANDBOX_INCLUDE}" "${TW_ORACLE_IO}")

# The tap's Debug half, copied with `#ifdef _DEBUG` on (a Release oracle defining _DEBUG would mismatch the CRT) and its steady clock the oracle's.
set(TW_PROBE_WHAT helper_tap_oracle)
set(HELPER_TAP "${CMAKE_CURRENT_BINARY_DIR}/helper_tap_seam/test")
file(READ "${TW_TDESKTOP_FILES}/test/test_widget_helper_tap.h" HELPER_TAP_H)
string(REPLACE "\r\n" "\n" HELPER_TAP_H "${HELPER_TAP_H}")
tw_probe(HELPER_TAP_H 1 [=[#ifdef _DEBUG]=] [=[#if 1]=])
file(READ "${TW_TDESKTOP_FILES}/test/test_widget_helper_tap.cpp" HELPER_TAP_CPP)
string(REPLACE "\r\n" "\n" HELPER_TAP_CPP "${HELPER_TAP_CPP}")
tw_probe(HELPER_TAP_CPP 1 [=[#ifdef _DEBUG]=] [=[#if 1]=])
tw_probe(HELPER_TAP_CPP 1 [=[	return duration_cast<microseconds>(
		steady_clock::now().time_since_epoch()).count();]=] [=[	return OracleSteadyMicros();]=])
tw_probe(HELPER_TAP_CPP 1 [=[namespace Test {
namespace {]=] [=[std::int64_t OracleSteadyMicros();

namespace Test {
namespace {]=])
file(WRITE "${HELPER_TAP}/test_widget_helper_tap.h.tmp" "${HELPER_TAP_H}")
configure_file("${HELPER_TAP}/test_widget_helper_tap.h.tmp" "${HELPER_TAP}/test_widget_helper_tap.h" COPYONLY)
file(WRITE "${HELPER_TAP}/test_widget_helper_tap.cpp.tmp" "${HELPER_TAP_CPP}")
configure_file("${HELPER_TAP}/test_widget_helper_tap.cpp.tmp" "${HELPER_TAP}/test_widget_helper_tap.cpp" COPYONLY)
add_executable(helper_tap_oracle helper_tap_oracle.cpp "${HELPER_TAP}/test_widget_helper_tap.cpp" "${TW_CORE}/remote.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(helper_tap_oracle PRIVATE
    "${CMAKE_CURRENT_BINARY_DIR}/helper_tap_seam" "${TW_TDESKTOP_FILES}" "${TW_ROOT}" "${TW_CORE}" "${TW_SANDBOX_INCLUDE}" "${TW_ORACLE_IO}")
if (MSVC)
    target_compile_definitions(helper_tap_oracle PRIVATE NOMINMAX)
endif()
