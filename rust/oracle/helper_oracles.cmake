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

# tgw_cursor.cpp over helper_seam's renderer.h, its header copied with the members public for the oracle to read them.
set(TW_PROBE_WHAT cursor_oracle)
set(CURSOR_SEAM "${CMAKE_CURRENT_BINARY_DIR}/cursor_seam")
file(READ "${TW_TDESKTOP_FILES}/tgw/tgw_cursor.h" CURSOR_H)
string(REPLACE "\r\n" "\n" CURSOR_H "${CURSOR_H}")
tw_probe(CURSOR_H 1 [=[private:]=] [=[public:]=])
file(WRITE "${CURSOR_SEAM}/tgw/tgw_cursor.h.tmp" "${CURSOR_H}")
configure_file("${CURSOR_SEAM}/tgw/tgw_cursor.h.tmp" "${CURSOR_SEAM}/tgw/tgw_cursor.h" COPYONLY)
add_executable(cursor_oracle cursor_oracle.cpp "${TW_TDESKTOP_FILES}/tgw/tgw_cursor.cpp")
target_include_directories(cursor_oracle PRIVATE
    "${CURSOR_SEAM}" "${CMAKE_CURRENT_SOURCE_DIR}/helper_seam" "${TW_TDESKTOP_FILES}" "${TW_ROOT}" "${TW_ORACLE_IO}")
target_link_libraries(cursor_oracle PRIVATE lhdump_litehtml)

# The hosted glue on the real renderer end and renderer, copied with the renderer's clock and the frame message's on the session's step clock.
set(TW_PROBE_WHAT hosted_glue_oracle)
set(GLUE_SEAM "${CMAKE_CURRENT_BINARY_DIR}/glue_seam")
file(READ "${TW_TDESKTOP_FILES}/tgw/tgw_hosted_glue.cpp" GLUE_CPP)
string(REPLACE "\r\n" "\n" GLUE_CPP "${GLUE_CPP}")
tw_probe(GLUE_CPP 1 [=[		channel,
		_caps);]=] [=[		channel,
		_caps,
		OracleClock());]=])
tw_probe(GLUE_CPP 1 [=[
namespace Tgw {
]=] [=[
twlite::Clock *OracleClock();

namespace Tgw {
]=])
file(WRITE "${GLUE_SEAM}/tgw/tgw_hosted_glue.cpp.tmp" "${GLUE_CPP}")
configure_file("${GLUE_SEAM}/tgw/tgw_hosted_glue.cpp.tmp" "${GLUE_SEAM}/tgw/tgw_hosted_glue.cpp" COPYONLY)
file(READ "${TW_TDESKTOP_FILES}/tgw/tgw_renderer_rules.h" GLUE_RULES)
string(REPLACE "\r\n" "\n" GLUE_RULES "${GLUE_RULES}")
tw_probe(GLUE_RULES 1 [=[twlite::Renderer::clock() * 1000.]=] [=[OracleClockMs() / 1000. * 1000.]=])
tw_probe(GLUE_RULES 1 [=[
namespace Tgw::RendererRules {
]=] [=[
double OracleClockMs();

namespace Tgw::RendererRules {
]=])
file(WRITE "${GLUE_SEAM}/tgw/tgw_renderer_rules.h.tmp" "${GLUE_RULES}")
configure_file("${GLUE_SEAM}/tgw/tgw_renderer_rules.h.tmp" "${GLUE_SEAM}/tgw/tgw_renderer_rules.h" COPYONLY)
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
    "${TW_TDESKTOP_FILES}/tgw/tgw_cursor.h" "${TW_TDESKTOP_FILES}/tgw/tgw_hosted_glue.cpp" "${TW_TDESKTOP_FILES}/tgw/tgw_renderer_rules.h")
add_executable(hosted_glue_oracle hosted_glue_oracle.cpp "${GLUE_SEAM}/tgw/tgw_hosted_glue.cpp" "${TW_TDESKTOP_FILES}/tgw/tgw_cursor.cpp"
    "${TW_CORE}/renderer_end.cpp" "${TW_CORE}/platform_end.cpp" "${TW_CORE}/remote.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(hosted_glue_oracle PRIVATE
    "${GLUE_SEAM}" "${TW_TDESKTOP_FILES}" "${TW_ROOT}" "${TW_CORE}" "${TW_ROOT}/tests" "${TW_SANDBOX_INCLUDE}" "${TW_ORACLE_IO}")
target_link_libraries(hosted_glue_oracle PRIVATE oracle_core)
if (MSVC)
    target_compile_definitions(hosted_glue_oracle PRIVATE _CRT_SECURE_NO_WARNINGS NOMINMAX)
endif()

# The helper's serving end to end (stage H2): WidgetHelper::Serve as the C++ helper links it (_other/widget_helper_renderer.cpp on widget-sandbox's engine end, the hosted glue and the tap) beside twidget_helper_ffi's tw_helper_serve, which cargo builds with Nova's C interface so both serve on one Nova.
set(TW_PROBE_WHAT helper_ffi_oracle)
set(TW_RUST "${TW_ROOT}/rust")
set(TW_SANDBOX_SRC "${TWIDGET_WIDGET_SANDBOX}/packages/desktop/sandbox/src")
get_filename_component(TW_NOVA "${TWIDGET_WIDGET_SANDBOX}/../nova-safe" ABSOLUTE)
find_program(TW_CARGO cargo REQUIRED)
set(HELPER_FFI_LIB "${CMAKE_CURRENT_BINARY_DIR}/helper_ffi/${CMAKE_STATIC_LIBRARY_PREFIX}twidget_helper_ffi${CMAKE_STATIC_LIBRARY_SUFFIX}")
# Always runs: cargo decides what is stale, in the workspace's own target directory and release profile, and the copy changes only with its bytes.
add_custom_target(helper_ffi_cargo
    COMMAND "${TW_CARGO}" build --release --locked --offline --package twidget_helper_ffi --features nova-c-api --lib --target-dir "${TW_RUST}/target"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${TW_RUST}/target/release/${CMAKE_STATIC_LIBRARY_PREFIX}twidget_helper_ffi${CMAKE_STATIC_LIBRARY_SUFFIX}" "${HELPER_FFI_LIB}"
    WORKING_DIRECTORY "${TW_RUST}"
    BYPRODUCTS "${HELPER_FFI_LIB}"
    COMMENT "cargo build twidget_helper_ffi"
    VERBATIM)

# The engine's clock (steady_clock.h) on the oracle's, as the Rust end reads its platform's: nova_context.cpp and sandbox_engine_shared.cpp copied beside the oracle's header, which their quoted include finds first.
set(SERVE_SEAM "${CMAKE_CURRENT_BINARY_DIR}/serve_seam")
file(WRITE "${SERVE_SEAM}/steady_clock.h.tmp" "#pragma once\ndouble OracleSteadySeconds();\nnamespace twidget::detail {\ninline double steadySeconds() { return OracleSteadySeconds(); }\n} // namespace twidget::detail\n")
configure_file("${SERVE_SEAM}/steady_clock.h.tmp" "${SERVE_SEAM}/steady_clock.h" COPYONLY)
configure_file("${TW_SANDBOX_SRC}/nova_context.cpp" "${SERVE_SEAM}/nova_context.cpp" COPYONLY)
configure_file("${TW_SANDBOX_SRC}/sandbox_engine_shared.cpp" "${SERVE_SEAM}/sandbox_engine_shared.cpp" COPYONLY)
# widget_helper_renderer.cpp untapped too, as a Release helper builds it: TapHelperRenderer's Release identity in the tap's place.
file(READ "${TW_TDESKTOP_FILES}/_other/widget_helper_renderer.cpp" SERVE_CPP)
string(REPLACE "\r\n" "\n" SERVE_CPP "${SERVE_CPP}")
tw_probe(SERVE_CPP 1 [=[std::string Serve(]=] [=[std::string ServeUntapped(]=])
tw_probe(SERVE_CPP 1 [=[Test::TapHelperRenderer(]=] [=[OracleUntapped(]=])
tw_probe(SERVE_CPP 1 [=[
namespace WidgetHelper {
]=] [=[
std::unique_ptr<twidget::Renderer> OracleUntapped(std::shared_ptr<twidget::SandboxHostedPlatform> platform, const std::string &scheme, const Test::HelperRendererBuild &build);

namespace WidgetHelper {
]=])
file(WRITE "${SERVE_SEAM}/widget_helper_renderer_untapped.cpp.tmp" "${SERVE_CPP}")
configure_file("${SERVE_SEAM}/widget_helper_renderer_untapped.cpp.tmp" "${SERVE_SEAM}/widget_helper_renderer_untapped.cpp" COPYONLY)
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
    "${TW_TDESKTOP_FILES}/_other/widget_helper_renderer.cpp" "${TW_SANDBOX_SRC}/nova_context.cpp" "${TW_SANDBOX_SRC}/sandbox_engine_shared.cpp")

# widget-sandbox's desktop sandbox as its CMakeLists.txt builds it, but for engine_platform.cpp (the oracle defines what the OS answers) and gesture_policy.cpp (oracle_core has it).
add_library(serve_sandbox STATIC
    "${SERVE_SEAM}/nova_context.cpp" "${SERVE_SEAM}/sandbox_engine_shared.cpp"
    "${TW_SANDBOX_SRC}/cpu_budget.cpp" "${TW_SANDBOX_SRC}/bundle_stamp.cpp" "${TW_SANDBOX_SRC}/trusted_validator.cpp" "${TW_SANDBOX_SRC}/sandbox_engine.cpp"
    "${TW_SANDBOX_SRC}/remote_engine.cpp" "${TW_SANDBOX_SRC}/remote_stream_engine.cpp" "${TW_SANDBOX_SRC}/remote_hosted.cpp" "${TW_SANDBOX_SRC}/widget_host_local.cpp"
    "${TW_SANDBOX_SRC}/js_thread.cpp" "${TW_SANDBOX_SRC}/sdk_policy.cpp" "${TW_SANDBOX_SRC}/intl.cpp" "${TW_SANDBOX_SRC}/resources.cpp"
    "${TW_SANDBOX_SRC}/remote_coding.cpp" "${TW_SANDBOX_SRC}/remote_stream.cpp" "${TW_SANDBOX_SRC}/sandbox_engine_client.cpp" "${TW_SANDBOX_SRC}/widget_host.cpp")
target_include_directories(serve_sandbox PUBLIC "${TW_SANDBOX_INCLUDE}" PRIVATE "${TW_SANDBOX_SRC}" "${TW_NOVA}/nova_embed_ffi/include")
if (WIN32)
    target_sources(serve_sandbox PRIVATE "${TW_SANDBOX_SRC}/icu_intl.cpp")
    target_compile_definitions(serve_sandbox PRIVATE NOMINMAX _CRT_SECURE_NO_WARNINGS)
    target_link_libraries(serve_sandbox PUBLIC icu bcrypt)
endif()

add_executable(helper_ffi_oracle helper_ffi_oracle.cpp
    "${TW_TDESKTOP_FILES}/_other/widget_helper_renderer.cpp" "${SERVE_SEAM}/widget_helper_renderer_untapped.cpp" "${HELPER_TAP}/test_widget_helper_tap.cpp"
    "${GLUE_SEAM}/tgw/tgw_hosted_glue.cpp" "${TW_TDESKTOP_FILES}/tgw/tgw_cursor.cpp"
    "${TW_CORE}/renderer_end.cpp" "${TW_CORE}/platform_end.cpp" "${TW_CORE}/remote.cpp" "${TW_CORE}/wire.cpp")
target_include_directories(helper_ffi_oracle PRIVATE
    "${GLUE_SEAM}" "${CMAKE_CURRENT_BINARY_DIR}/helper_tap_seam" "${TW_TDESKTOP_FILES}" "${TW_ROOT}" "${TW_CORE}" "${TW_ROOT}/tests"
    "${TWIDGET_WIDGET_SANDBOX}/packages/desktop" "${TW_ORACLE_IO}" "${TW_RUST}/crates/helper_ffi/include")
target_link_libraries(helper_ffi_oracle PRIVATE oracle_core serve_sandbox "${HELPER_FFI_LIB}")
add_dependencies(helper_ffi_oracle helper_ffi_cargo)
if (MSVC)
    target_compile_definitions(helper_ffi_oracle PRIVATE _CRT_SECURE_NO_WARNINGS NOMINMAX)
    # What `cargo rustc -- --print native-static-libs` lists for the crate.
    target_link_libraries(helper_ffi_oracle PRIVATE kernel32 advapi32 ntdll userenv ws2_32 dbghelp bcrypt psapi)
endif()
target_compile_definitions(helper_ffi_oracle PRIVATE TW_WIDGET_JS="${TWIDGET_WIDGET_SANDBOX}/packages/desktop/WidgetJS")
