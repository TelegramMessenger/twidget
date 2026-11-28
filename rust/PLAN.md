# Widgets without `unsafe`: the helper in safe Rust

Goal: everything Telegram's sandboxed widget helper runs, apart from a small audited layer of OS glue,
is Rust that the compiler checks for memory safety — every crate `#![forbid(unsafe_code)]`, no
dependency with `unsafe` — and it behaves exactly as the C++ it replaces. Nova (TelegramMessenger/nova-safe)
already did this for the JavaScript engine; this plan does it for the rest: the HTML and CSS engine
(the litehtml fork), the renderer (TwidgetCore's renderer end), the sandbox glue around Nova
(widget-sandbox's desktop core) and the tdesktop code compiled into the helper.

"Exactly" is checked, not hoped for: every stage below is compared with the C++ on the same inputs,
output for output, before the next one starts.

## What runs in the helper today

| Part | Where | C++ lines | Runs in the helper |
|---|---|---:|---|
| JavaScript engine | nova-safe (Rust) | — | all platforms (tdesktop `private/nova-safe`); already `forbid(unsafe_code)` |
| Sandbox glue: engine end, in-process engine, CPU budget, SDK policy, JSON, stream wire | widget-sandbox `packages/desktop/sandbox` | ~6.8k | all platforms |
| Hosted runtime, served `WidgetHost`, trusted validator | the same | ~3.0k | macOS today; Windows and Linux in progress |
| Renderer end: shadow tree, container, queries, events, selection, canvas, animations, compositor, paint policy, wire and remote codecs | twidget `core/src` | ~13k | where the helper renders |
| HTML and CSS engine: the litehtml fork | twidget `litehtml/src`, `include` | ~52k (~40k reachable under profile 1) | where the helper renders |
| HTML parser: Gumbo, run on every batch | twidget `litehtml/src/gumbo` | ~10k (+23k entity table) | where the helper renders |
| tdesktop glue: hosted glue, cursor styles, renderer rules, hosted wire | tdesktop `SourceFiles/tgw` | ~2.2k | where the helper renders |
| OS glue: entry, channel adoption, lockdown, XPC service | tdesktop `SourceFiles/_other`, `platform/*` | ~0.5–1.4k per platform | all; **stays** (see "The one `unsafe` layer") |

Roughly 75k lines of C++ to port. The renderer re-parses its own serialised HTML with Gumbo on every
batch (litehtml has no DOM mutation API), so HTML tree construction is part of "behaves the same".

## Rules

Safety:

1. Every crate in the workspace carries `#![forbid(unsafe_code)]`, also set as a workspace lint. The one
   exception is the C ABI crate (below), which `deny(unsafe_op_in_unsafe_fn)` and comments each block's
   precondition.
2. No registry dependency in the helper's build, unless it has no `unsafe` (`tools/unsafe_report.py`
   counts the whole linked tree) and earns its place. Default: std only, as tlottie and nova-safe's
   engine crates do. What std and rustc do is trusted; nothing else is.
3. Hostile input never panics. A panic is still memory-safe, but it stops a widget the C++ would have
   kept; decoders, parsers and every entry point are fuzzed for it. Sizes a widget chooses go through
   `try_reserve` or a cap before they allocate: Rust aborts on allocation failure, and one helper serves
   every widget.

Fidelity:

4. Port, do not redesign. Same algorithms, same order of operations, same float widths per expression
   (`f32` where C++ computes in `float`, `f64` where it promotes), same caps, same error and kill texts,
   same order of backend calls and clock reads. Idiomatic Rust where it cannot change an output.
5. Object graphs (litehtml's `shared_ptr`/`weak_ptr` trees, the renderer's raw `Node*` links, the
   container's back references) become arenas with `u32` ids. Payload sharing that is observable on the
   wire (`Command::path`, `Command::filter`, `CanvasOp::clips`, `CanvasOp::rgba`) stays observable:
   `Arc` and `Arc::ptr_eq` (`Arc`, not `Rc`, so a renderer can move between the helper's threads),
   cloned exactly where C++ copies.
6. Process-wide state of the C++ (litehtml's name table, its declaration caches, the `LH_*` switches)
   becomes an explicit object the helper owns once per process, so its effect on outputs is the same
   and tests can make a fresh one.
7. `litehtml::pixel_t`'s fuzzy comparisons (equality within 1e-4, `std::max`/`min` returning the first
   argument inside the band) are a newtype with the same operators, never `f32::max`.

## Architecture

- `twidget/rust/` (this workspace): the codecs, the renderer end and the litehtml port.
- `widget-sandbox/packages/rust/` (stage S and what widget-sandbox owns: `twidget_json`, `twidget_testkit`): the
  sandbox glue, as another copy of the host glue next to the web Worker, Swift, Kotlin and C++ ones, reading
  the same fixtures. twidget's crates depend on it, as twidget's C++ includes widget-sandbox's headers. It calls `nova_embed` (nova-safe's
  safe embedding API) directly; there is no C layer between the glue and the engine.
- `twidget/rust/crates/helper` (stage H): the helper's serving loop over one byte stream, the tdesktop
  glue ported, linking both of the above.
- **The one `unsafe` layer**, `twidget/rust/crates/helper_ffi`: a C ABI of a handful of functions
  (`tw_helper_serve` with read, write and shutdown callbacks over the stream the C++ OS glue adopted),
  `catch_unwind` around every entry, nothing but byte buffers crossing it. The OS glue that adopts the
  channel and locks the process down stays C++ (it is syscalls, audited, platform-specific, and its
  probes run inside it), so Rust never touches a descriptor or a handle.

The helper keeps the wire it speaks today: widget-sandbox's stream frames, tgw's `0xFF` controls and
`twlite::remote` messages are decoded by C++ in the main process, which does not change. A Rust helper is
a drop-in replacement at the executable's boundary, which is what makes it testable against the C++ one.

## Tests

Each layer has an oracle that is not the code under test.

| Layer | What it compares | Oracle |
|---|---|---|
| T1 ported unit tests | every C++ test, ported by name (`core_tests` ~600 checks, `wire_tests`, `remote_tests`, `ends_tests`, `paint_policy_tests`, `css_keywords_tests`; widget-sandbox's 235 desktop tests) | their expectations |
| T2 shared fixtures | `cpu-budget.json`, `sdk-policy.json`, `gesture-policy.json`, `paint-policy.json` | the files every port already reads |
| T3 golden bytes | every binary form and message as `docs/TWIDGET_CORE.md` and `docs/protocol.md` lay it out | the documented tables |
| T4 differential, per stage | the same input through the C++ and the Rust part: bytes, dumps with float bits, call logs | the C++, built by `oracle/` as small programs on stdin/stdout |
| T5 recordings | `.twrec` sessions (`tests/recordings`, re-recorded deterministically, plus sessions recorded from every corpus widget) replayed through the Rust renderer end: messages, lists, damage, backend calls, clock reads | the C++ renderer that recorded them |
| T6 stream | widget-sandbox's suite and `twidget-run --attacks / --corpus` with `TW_ENGINE_STREAM` pointed at the Rust engine end in a child process | the C++ engine end's per-file rows |
| T7 in the app | tdesktop's attack audit, conformance and stress packs, the tap's mirrored records, the lockdown probes, with the Rust helper, the C++ helper and in-process as arms | the C++ helper |
| T8 safety | `unsafe` count 0 outside `helper_ffi`, no dependency with `unsafe`, no panic and bounded allocation on mutated and generated input | `tools/unsafe_report.py`, seeded mutators |
| T9 speed | the `bench.cpp` scenarios and the stress ladders, Rust against C++ | the C++ timings |

Inputs for T4 and T5: widget-sandbox's corpora (`corpus/compat`, `corpus/attacks`, `generated`,
`htmlbubbles-ported`), the conformance set (68 auto + 21 visual widgets), the stress kit (127 widgets),
twidget's demos, and seeded generators of in-profile HTML, CSS, op streams, canvas commands and hostile
bytes. Every corpus widget is recorded once through the C++ pipeline (sandbox, validator, renderer with a
logging backend), which yields the op streams, the serialised HTML snapshots, the stylesheets and the
canvas command streams the stages below consume.

The gate (`tools/gate.py`) builds the oracle, runs `cargo test` with it, clippy with warnings as errors,
and the `unsafe` report. A stage is done when its gate is green and its differential run over the whole
corpus has no difference.

### Determinism first

Some C++ outputs are not reproducible even by the C++, and those have to be fixed in the C++ before a
port can be compared with it (stage 0.3):

- iteration over `unordered_map`s leaks into outputs: `Animator::update`'s `animationstart` order, the
  running animations and transitions, `~Renderer` and the canvas-font flushes releasing canvases and
  fonts, pointer-capture order over `touches_`. Make them ordered (by id or insertion).
- clang and GCC on arm64 fuse `a*b+c` (`-ffp-contract=on`); Rust never fuses and MSVC on x64 does not:
  build litehtml and the core with `-ffp-contract=off`.
- `snprintf`/`strtod`/`std::to_string` follow `LC_NUMERIC`: pin the C locale or use locale-free code.
- addresses in keys (`element::paint_signature`'s selector pointers, `PatchLog`'s pointer-ordered sets):
  compare through a correspondence, as the replay does, or key by id.
- ties in `std::sort` (selector order, hole order): confirm the keys are unique.

Each such change is its own commit in the C++, reviewed as a behaviour change, and the recordings are
taken again after them.

## Stages

Each stage lands with its tests; a stage starts when the stages it needs are done. Stage S runs in
parallel with stages 3–5: it shares only stage 1.

### 0. Ground

- **0.1** Workspace, workspace lints, `tools/unsafe_report.py`, `tools/gate.py`, this plan.
- **0.2** `oracle/`: a CMake project building the C++ (wire, remote, core, litehtml) for the host, and one
  small program per stage that reads cases on stdin and writes canonical outputs. On Windows x64 (MSVC:
  no FMA contraction, deterministic hash order per build).
- **0.3** Determinism in the C++ (above), then `corpus.twrec` recorded again.
- **0.4** A freeze point for `twlite::remote`: the in-progress "cut the remote widget renderer's round
  trips and bytes" work may change it. Port the version that is current when stage 5 starts, behind
  its version byte.
- **0.5** Corpus recording: every corpus widget through the C++ pipeline into `.twrec` sessions and
  per-stage inputs, kept out of git except a curated set.

### 1. Codecs and value types

- **1.1** `twidget_wire`: the paint values (`Color` … `CanvasOp`, `FontMetrics`) and their binary forms
  (list, damage, batch, metrics). T1 from `wire_tests.cpp`, T3, T4 both ways (Rust encodes, C++ decodes
  and re-encodes; mutated bytes through both decoders), T8.
- **1.2** `twidget_json`: widget-sandbox's `twidget::Json` (strict parse, bounded depth, ordered
  objects, its exact `dump`, its number formatting).
- **1.3** `twidget_remote`: the 25 renderer-channel messages and their codecs (`remote.cpp`).
- **1.4** tgw's hosted wire, widget-sandbox's stream frames (`remote_wire.h`) and coding
  (`remote_coding.cpp`).

### 2. Pure renderer helpers

- **2.1** core `css.cpp`: colours, `light-dark()`, `var()` substitution, transforms, `parse_font`, theme
  and safe-area variables, `format_number`.
- **2.2** paint policy (`paint-policy.json`). The gesture policy is widget-sandbox's and goes with S1.
- **2.3** compositor functions: `summarize`, `damage_between`, `drop_inert_clips`, `paint_digest`,
  `bake_track`, `resume_track`; `base64`.

### 3. Canvas and animations

- **3.1** the canvas interpreter: JSON commands in, `CanvasOp` batches and query answers out, through a
  `CanvasHost` trait. T4 on the canvas command streams of the corpora and generated ones.
- **3.2** the animator: declarations and times in, overrides and events out.

### L. The litehtml port

Every stage is compared through `lhdump`, a C++ program on the fork and its Rust twin: serialised HTML,
the master CSS, a width and a scripted backend in; the stage's dump out, every float as bits. The C++
runs with the `LH_NO_*` switches off by default and on where a stage says so, one process per case.

- **L0** foundations: `Px` (pixel_t), the name table, `t_strtof`, the tokenizer's number conversion as
  written, `web_color`, `css_length`, the string tables.
- **L1** CSS tokenizer — token stream.
- **L2** CSS parser, stylesheets, at-rules, `@layer`, media queries — rule tree, media matrix.
- **L3** HTML tree construction and the element kinds, text split into words and spaces, source spans
  — DOM dump. Gumbo is the reference; twidget's `origin/macos/html5ever-parser` branch (html5ever
  vendored without `unsafe`) is measured against Gumbo on the corpus snapshots first, and the parser that
  matches is the one ported or kept.
- **L4** selectors: parsing, specificity, matching with `:hover`/`:active` — match matrix.
- **L5** cascade and computed style: `var()`, `calc()`, `inherit`, `revert`, layers, style sharing — a
  dump of every property per element, and the core's `cs` strings.
- **L6** render tree: boxes, anonymous and table fix-up boxes, `::before`/`::after`, list markers.
- **L7** block layout, margins, floats, formatting contexts.
- **L8** inline layout: line boxes, the text breaker's calls, white-space, bidi, ellipsis — with the
  order of `text_width` calls.
- **L9** positioned layout, stacking, z-order, sticky, scroll views.
- **L10** flex. **L11** grid. **L12** tables.
- **L13** backgrounds, borders, gradients, shadows, transforms, filters, groups, clips — a log of every
  container paint call.
- **L14** SVG: shapes, paths, `<use>`, text, gradients, bounding boxes, path lengths.
- **L15** images (`data:`) and canvas elements.
- **L16** hit testing and the mouse state.
- **L17** every limit and budget (layout budget, names, depths, the sheet cache), each crossed by a test.
- **L18** the layout cache and incremental relayout, compared with the cache on in both.
- **L19** document patching: templates and holes, structural update, `update_holes`, `update_styles`,
  paint-only patches and their regions — on patch sequences from the recordings.

### 4. The renderer

- **4.1** the shadow tree from ops and its serialiser — `last_html()` on every recorded step.
- **4.2** the container on the Rust litehtml: fonts and the width cache, the text breaker, the display
  list.
- **4.3** build, patch, the layout budget and `frozen`; every query kind.
- **4.4** events, listeners, gestures, focus, the editor, selection.
- **4.5** the paint policy's heavy boxes, layers, damage, the throttle.

### 5. The renderer end

- **5.1** `RendererEnd` and its backend proxy over the channel, report order. T1 from `ends_tests.cpp`.
- **5.2** the replay: every `.twrec` session through the Rust renderer end, step for step (T5). This is
  the renderer's gate.

### S. The sandbox glue (widget-sandbox `packages/rust`)

- **S1** CPU budget, SDK policy, gesture policy, bundle stamp — on the shared fixtures; the stream wire and coding come
  from 1.4.
- **S2** the runtime: one thread per engine with an 8 MiB stack, its task queue and timers, as actors
  that own their state (no shared `this`).
- **S3** the engine context over `nova_embed`: host functions, deadlines through the interrupt, the
  memory limit, promise jobs, WTF-8 at every boundary.
- **S4** the in-process engine: every host call kind, the settle and pump loops, the kill texts; the
  trusted validator.
- **S5** the engine end over a stream (Windows and Linux helpers today) — T6.
- **S6** the hosted runtime and the served `WidgetHost`, with the Rust renderer as its renderer.

### H. The helper

- **H1** tdesktop's hosted glue, renderer rules, hosted wire, the Debug tap, and cursor styles (on the
  Rust litehtml's hit test and computed style).
- **H2** `helper_ffi`, the C ABI, and its audit.
- **H3** the build: cargo per platform (Windows MSVC with a static CRT and the import-table check against
  the LPAC KnownDLL rule; Linux in the Docker image with a toolchain that stays; macOS per architecture
  and `lipo`), a `TDESKTOP_WIDGET_HELPER_RUST` option, both helpers side by side in Debug with a launch
  switch for A/B runs. The helper starts no thread before lockdown (Linux) and opens no descriptor after
  it (macOS).
- **H4** T7 on all three platforms.
- **H5** the default flips per platform; the C++ leaves the helper.

## Decisions taken

- The workspace lives in twidget (`rust/`), the sandbox glue in widget-sandbox (`packages/rust/`): each
  repo ports its own code (agreed 2026-11-20).
- The determinism fixes of stage 0.3 go into the shipped C++, each its own commit, although they change
  what it does in small ways (agreed 2026-11-20).
- The freeze point of `twlite::remote` (0.4) is agreed with the owner of the round-trip work before stage
  5 starts.
- The OS glue stays C++: it is already small and audited, and its probes run inside it.
- No registry crates so far; nova-safe's own tree is the only Rust dependency stage S adds.
- The base is tdesktop's `nova-safe` branch: widget-sandbox at its `nova-safe` tip (2871bfc), whose stream lost
  the bytecode fields, and nova-safe 05d4344c (agreed 2026-11-20).
- The toolchain is the caller's (`C:\Telegram\ThirdParty\rust`, 1.96.1 on Windows), as nova-safe's build
  in tdesktop does; no `rust-toolchain.toml` pins it here.

## Open questions

- Gumbo or html5ever (L3): decided, Gumbo. html5ever gave a tree alike with Gumbo's on what litehtml reads in 18.5% of
  141k parses (no source positions for 246 of 412 corpus pages, other text spans, control characters kept, CDATA as
  text, no stop at 400 open elements); Gumbo is ported.
- Whether to replace the serialise-and-reparse round trip by building the DOM from the shadow tree
  directly — only after L19 matches, and only if a tree-construction normaliser reproduces Gumbo's
  results on every recorded step.
- Out-of-profile values reaching litehtml through `var()` (`Container::sanitize_declaration` blocks only
  a few): tighten the C++ to an allow-list first, or port the paths those values take. Decide before L5.
- Integer overflow: the tests run with overflow checks (debug), so an unintended overflow is a failure
  there and C++'s intended wrapping is written `wrapping_*`. Whether the shipped helper keeps the checks
  (a panic that stops one widget instead of a wrapped value) is decided at H3.
- Allocation: the C++ wire tests count every allocation a decode makes with a replaced global
  allocator, which Rust can only do with `unsafe`; the Rust tests bound what a decoded value holds
  instead, and the comparison with the C++ shows both decoders refuse the same inputs.

## Progress

| Stage | State |
|---|---|
| 0.1 | done: workspace, workspace lints, `tools/unsafe_report.py`, `tools/gate.py` |
| 0.2 | done: `oracle/`, one CMake project building twidget's C++ (wire, remote, core, litehtml with Gumbo) for the host (Windows x64, MSVC, no FMA contraction, ordered iteration where 0.3 made it so), and a program per stage reading cases on stdin and writing canonical answers: `wire_oracle`, `remote_oracle`, `css_oracle`, `paint_oracle`, `compositor_oracle`, `base64_oracle`, `canvas_oracle`, `animations_oracle`, `filters_oracle`, `png_oracle`, `encode_oracle`, `ends_oracle`, `computed_style_oracle`, `shadow_oracle`, `svg_oracle`, `lhdump` (every L stage's records: Gumbo, DOM, cascade, render tree, layout with grid passes, paint, patch, hit), and renderer.cpp copied with probes (`probe.cmake`, `input_probes.cmake`) as `events_oracle` and `renderer_oracle` (the tape); widget-sandbox's json, stream, cpu budget, SDK, gesture and stamp oracles in its own workspace. `tools/gate.py` builds it first; it follows the C++ at the freeze (0.4) |
| 0.3 | done but the recording: the animator's events, transitions, overrides and heavy motions and the renderer's canvas and font releases come in id or creation order; `pair_tokens`' key is length-prefixed (no cross-pair or cross-widget collision); `paint_signature` names selectors by sheet and order (`css_selector::place()`) and `PatchLog` keys by node id; `-ffp-contract=off` on every non-MSVC target (MSVC's `/fp:precise` does not contract); numbers parsed and printed in the C locale (`litehtml/c_locale.h`) at every site that reaches an output, since Telegram sets `LC_ALL` itself; sort ties checked unique. Each its own commit with a test (the same widget under de-DE and C gives the same HTML, display list, answers and canvas calls); the Rust ports and oracles follow. Left: `tests/recordings/corpus.twrec` must be recorded again on macOS with CoreGraphics (the owner's): on Windows both replays stop at step 132, canvas fonts released 6, 5 in libc++'s hash order where 0.3 releases 5, 6, and past it step 343 is one ulp off (arm64 contracted `c = y - m*x` before `-ffp-contract=off`); nothing else differs, and its test asserts that mismatch until the origin names a re-record. `tools/record` builds everywhere since 0.5 (the stub Backend off macOS). tdesktop needs `-ffp-contract=off` in `lib_widget_renderer.cmake` and `lib_litehtml.cmake`'s non-MSVC options too |
| 0.4 | done: the freeze is twidget f0a070d, which the owner's `private/js-sandbox` pins (with widget-sandbox cac5a6c and nova-safe 906ebcf); `twlite::remote` at version 1 with 28 kinds (WidthsRequest 100, WidthsAnswer 36, FrameDelta 73) and the caps `widths`, `widths_text`, `frame` and `delta_base`. Both branches were rebased onto the pins and the port caught up: per-thread name counts (c6a85f0); texts that owe their widths and the Container's pre-layout batch behind `Engine::measure_words` (d533840; the Container's half with 4.2); the break predicates, wire's command starts, the remote's widths messages and frame deltas, the ends' batching, deltas and close (d533840, f9cf512, f0a070d); a5375fd is build-only. Three C++ 0.3 fixes: the delta index hashes with a written-out FNV-1a, the container's images are kept in source order, the selectable key takes a document serial, not the document's address. Every comparison was rerun against f0a070d. widget-sandbox: nova-safe 906ebcf (35 commits of interpreter work) leaves `nova_embed`'s API and `nova_embed_ffi` as they were; cac5a6c (a hosting stream ends when its renderer leaves a message unanswered 3 s: `SandboxHostedAnswers`, `lookAtAnswers`, the channel's deaths and `localStop`) is the host's side alone (`remote_stream.cpp`, `sandbox_engine_client.cpp`; no wire rule or engine-process source changed), so nothing to port; `-DTW_RUST_ENGINE_END=ON` keeps one Nova (`TW_NOVA_ROOT` may name only the nova-safe `../rust` depends on, `TW_NOVA_LIBRARY` is refused). Windows x64: 244 tests, 0 failures in process, `TW_ENGINE_STREAM=1`, `TW_ENGINE_HOSTED=stream` and `=loopback` with either end, every result alike; 133 attacks, 0 leaks, 7 killed, compat 19/19, generated 20/20 twice, per-file rows alike; Win32 engine stream 244/0 on both ends. Moved on 2026-11-21 to js-sandbox 33fd4d91cd's nova-safe c6d4896 (Nova's speedups; twidget and widget-sandbox pins unchanged): twidget's gate (Debug and release tests, the 412 recorded corpus sessions) and the helper oracles on a js-sandbox checkout pass unchanged, as do widget-sandbox's gate and both ends' suites, x64 (244 tests, 0 failures in each mode; 133 attacks, 0 leaks) and Win32 (244/0, 133/0). The pins moved again to js-sandbox 866160d990 (twidget 7cfa295, widget-sandbox 3be502c, nova-safe unchanged): 7cfa295's `delta_scan` (a FrameDelta past every check `decode` makes, making nothing) came to `twidget_remote` with remote_tests' new checks, and 502,712 cases alike with remote.cpp's (generated deltas with no, empty, small and large damage, their changes and cuts, a byte after, another header or version, tight caps and message and frame caps around each delta's and its Frame's size, remote_tests' cases; each also against `decode`, which takes all the scan takes but the 121 whose damage does not decode); a planted slip, the message cap left out, differed in 154. widget-sandbox 3be502c (`Withdrawal::own`, `decidedView`) is `SandboxRemoteHostEnd`'s alone, so nothing to port. twidget's gate (Debug and release tests, the 412 recorded corpus sessions, the helper oracles on a js-sandbox checkout) and widget-sandbox's pass, as do both ends' suites after clean rebuilds, x64 (247 tests, 0 failures in each mode, the three new hosted tests on the scripted engine end either way; 133 attacks, 0 leaks) and Win32 (247/0, 133/0) |
| 0.5 | done: `tools/record` builds on any desktop: `TW_RECORD_BACKEND` is `coregraphics` (macOS, unchanged) or `stub` (elsewhere: renderer_oracle's stub fonts at 8 px a byte, `data:` images sized by their header, canvas pixels a hash of what was drawn; named in the origin with the compiler, standard library and system); `--keep` keeps a troubled widget's session; `record_corpus.py` / `record-corpus` records each widget in its own process and file under `build-record/recordings`, out of git. Windows x64: 412 widgets, 412 recordings, 30,290 steps, 40.8 MB (attacks 133, htmlbubbles-ported 116, generated 40, common 32, showcase 27, compat 19 and the rest); 30 kept with the trouble they are made for (7 die loading, 7 paint nothing, 8 throw, 8 die at the paint limit). Committed `corpus-stub.twrec`: 7 widgets (canvas, gestures, theme, the image, a long article, two attacks), 217,252 bytes, `twidget_parity_corpus_stub` on every platform, capped at 271,565 bytes (`TW_CORPUS_STUB_BYTES`). The C++ replay (Debug) replays every recording, `core.twrec` (285 sessions, 30,472 steps) and `bench.twrec` (7, 260) |
| 1.1 | done: `twidget_wire`; `wire_tests.cpp` ported (18 tests: documented bytes, round trips, cuts, count sites, references, enums, masks, flags, forms, defaults, the C++ suite's seeded mutations), and ~165k cases through the C++ and the Rust codec with identical answers (samples and their cuts, mutated samples, generated values, encoders under single caps); since 0.4 `wire_tests.cpp`'s `command_starts` too (19 tests), ~228k cases alike with 63k command-start cases (samples and 1,500 generated lists with their mutations, under three caps, at seven byte limits) |
| 1.2 | done: `twidget_json` (in widget-sandbox's `packages/rust` since stage 1.4); widget-sandbox's JSON tests ported, and ~400k cases through json.h and the port with identical answers (its fuzz texts both surrogate modes, depths around the bound, every escape and surrogate order, `quote`, 160k doubles through both number forms and `asInt`, 30k number texts through `strtod`, 37 real JSON files and their mutations); `twidget_testkit` holds what the tests share |
| 1.3 | done: `twidget_remote` at version 1 as frozen at twidget f0a070d (stage 0.4), 28 kinds; `remote_tests.cpp`'s message checks ported with the widths messages, `widths_in_request`, deltas rebuilt with their bytes and bounds, the frame place and view and `delta_ids`; ~642k cases alike (207k samples, cuts and mutations, 65k generated, 24k pixel answers, 136k widths cases, 209k frame-delta cases) |
| 2.1 | done: `twidget_css` (core `css.cpp`: colours, `light-dark()`, `var()`, `:where()`, declarations, names, opaque runs, times, `calc()`, transforms, origin, opacity, fonts, theme and safe-area variables) on `twidget_cstd` (C's `strtod` with its whole grammar, hex floats rounded to even, `atof`, `atoi`, `%.*f`); ~645k answers alike with css.cpp and MSVC's C library (140k numbers, 252k generated values through every helper and their changes, 12k themes and safe areas, 241k from the 2088 style texts and 12947 declarations of widget-sandbox's corpus) |
| 2.2 | done: `twidget_paint` (core `paint_policy`: the heavy-box rules and `PaintCounter`, the limits and their messages); the shared `paint-policy.json` passes (220 checks, as `paint_policy_tests.cpp` reads it), and 165k answers alike with paint_policy.cpp (generated shadow lists and their changes, styles, keyframe sequences, scripts of counter steps) |
| 1.4 | done: widget-sandbox's stream frames (`twidget_stream`, ~80k cases alike with remote_wire.h) and coding (`twidget_server::coding`, remote_coding.cpp, met by the C++ host end on every stream of T6); tgw's hosted wire in `twidget_helper::wire` as tdesktop `private/js-sandbox` has it, compared with tgw_hosted_wire.h from a tdesktop checkout: 123.6k cases alike (samples, cuts and mutations, 60k generated controls and work records, 30.3k encoder calls, constants and caps, every length limit on and past its boundary up to a 32 MiB mirrored message); a planted off-by-one failed |
| 2.3 | done: `twidget_compositor` (layers and tracks, `resume_track`, `paint_digest`, `summarize`, `drop_inert_clips`, `damage_between`, `bake_track`) and `twidget_base64`; core_tests' compositor and damage checks ported (9 tests), ~72k cases alike with compositor.cpp and encode.cpp (12k lists and their edits, 16k damage cases, 6k bakes with their sample times, 12k resumes, 2.4k base64 inputs) |
| 3.1 | done: `twidget_canvas`, the whole interpreter behind a `CanvasHost` trait with every limit, and the answers of `measureText` and `imageData`; 17 tests, ~19k scripts alike with canvas.cpp (2.8M generated commands and 16k queries, 26 command streams from `corpus.twrec` and ~900 changed or spliced, the stress widget's frames, corpus charts, 62 scripts that each cross a limit): every batch in wire form, every answer, the state after each step and every host call in order |
| 3.2 | done: `twidget_animations` (timing functions, keyframes, specs, interpolation, Web Animations records, the `Animator` with plans, signatures, the `anims` query and heavy motions; the C++'s per-thread caches as a shared `Caches`, the per-element maps by element id as 0.3 asks); core_tests' animation checks ported (16 tests, 4 more of the port's own), ~138k cases alike with animations.cpp (24k scripts, 20k timings, 40k interpolations, 8.6k pair-cache cases, 16k keyframe rules and specs, 20k Web Animations records, the corpus's 101 animated sheets with their 4342 declarations and value pairs); 467 of 474 branch points reached, the other 7 unreachable in the C++ too |
| 4.1 | done: `twidget_shadow`: the shadow tree, every op with its checks and limits (with the reordering of a batch whose canvases draw one another), the canvases the ops make, the patch log, the theme on the root, the frozen chrome and the writer behind `last_html()`; the renderer's other effects through a `DocumentHost` trait in the C++'s order. The tree cannot be built alone in the C++, so `shadow_oracle` is the whole renderer driven by its messages; ~19.3k scripts and ~123k batches alike (violations, canvas calls in wire form, `last_html()` byte for byte): corpus.twrec step by step and changed, 14k generated and changed scripts, every limit on its boundary, the 654 corpus widgets as validator-like ops; core_tests' tree checks ported (16 tests, 4 of the port's own); 214 of 221 branch points reached |
| 4.x helpers | done: `twidget_filters` (the painters' CSS filters: colour matrices, reach, `filter_pixels`; 42.5k matrices, 20k reaches, 8.6k filtered images alike, floats as bits), `twidget_png` (png.cpp has no encoder: the intact rows of a damaged PNG and the header sizes of PNG/GIF/JPEG/WebP; 136.5k inputs alike, 28.6k from the corpus's 198 images cut and mutated), `twidget_encode` (the display list, canvas and composition JSON for the Android and web painters; 28k alike byte for byte), `twidget_computed_style` (computed_style.cpp over an `Element` of what it reads of a fork element until L5; 49.6k helper inputs and 1.17M answers on 651 corpus pages and 1000 generated documents laid out by the fork, alike) |
| 4.2 | done: `twidget_document`, a crate of its own (the renderer stays generic over `Engine` and its tape comparisons build without litehtml): renderer.cpp's `Container` at twidget f0a070d as litehtml's `DocumentContainer` (fonts through the backend, one face per description shared by every document, faces no document holds evicted past 256, the width cache per face with its 4096-entry, 256-byte-key and shared 1 MiB bounds; texts measured later and a layout's words in bulk, text_widths.cpp's `LayoutWidths`: the walk's owed words and the units its shown spaced runs are painted in, in one `Backend::text_widths` call within each face's room and the shared bytes, then at the first character a breaking text's face lacks the break points of every shown text the layout may break, in one more; font handles with their metrics and spacings, the text breaker between user-perceived characters, `data:` images decoded by the backend with late sizes, the SVG placeholder, `tw-canvas:` sources checked against the renderer's canvases, the image table's 4096 sources and 32 MiB with their violations, media features and the viewport from the renderer's width, scale and viewport height, `sanitize_declaration` under trusted styles, every paint call into the display list), and `DocumentEngine`, `Engine` on a `twidget_litehtml::Document` with that container and renderer.cpp's free helpers (`boxes.rs`: the boxes, SVG boxes from sampled outlines, offsets, scroll extents, textarea content, the element index, declaration scans by style epoch, `measure_words`, the names the calling thread added, text runs, selectable text keyed by the document's serial, hits through `hittable`, the mouse state, the editor's style, heavy boxes, layer candidates, clips, shimmer tiles, the paint trace with `covered`/`obscurers`, the draws, `paint_key`); `Process` holds the name table, the declaration cache and the switches once per process. `Engine` gains `viewport`, `viewport_height` and `canvases`; `editor_style` takes the editor's clip, which the C++ leaves in place; `twidget_ends::Proxy` is `Clone`, shared by the renderer and its container. On renderer_oracle's tape, its stub's `text_widths` batches written where they are made, every 4.3 and 4.5 set plus written and generated container widgets and written batching cases (a `B` step grows the stub's widths per call, so every measure and width-cache hit shows): 5012 scripts, 2.26M answers of 64 methods and 12,051 batches alike with each engine asked every question, and 5012 transcripts alike end to end on the port's engine alone; 548 of 593 container blocks reached at the first count (34 of the rest unreachable in the C++ too, 11 the port's plumbing) and 28 of the batching's 33 branch points (one unreachable in the C++ too, three the port's plumbing). ends_tests' ten batching sessions plus bounded_batches and batch_failures run on the real renderer through both ends (warm_tabs' heap blocks compared as calls), and core_tests' two renderers on two threads are each charged their own names. The tape engine stays for the renderer's own comparisons |
| 4.3 | done: `twidget_renderer`: renderer.cpp's `Renderer` and `Container`'s document life: every host message, coalesced and partial batches, the build (the patch log into the fork's holes, the serialised HTML through `update_from_string`, a new document), paint-only patches, the viewport following the document in up to four passes, viewport units, fonts grown past 4096, the layout budget (100000 + 50 an element) and the name table's bounds that stop a widget with the badge alone, `frozen`, theme, safe area and scheme with the master CSS, animation overrides through the style holes, size reports and resize diffs, canvas fonts, the meters, every query kind and `result`; the tree and the input driven as the C++ drives them, litehtml behind an `Engine` trait and stage 4.5 behind `Layers`. `renderer_oracle` (probes shared with events_oracle via `probe.cmake`/`input_probes.cmake`): 2569 scripts, 92k steps alike, 877k answers replayed (corpus.twrec and 160 changed, 412 corpus widgets, 600 patch scripts, 600 documents with input, 600 theme/animation/resize/frozen, 120 scripted layers, 12 flushes, 60 past the budget or names); 12.6k queries of 13 kinds plus unknown kinds; 8.9k new documents, 19.3k HTML patches, 5.3k in place, 3.5k style-hole frames, 10.5k paint-only; 29 layout, 4 name, 8 paint stops; core_tests' checks ported by name (17, 3 of the port's own). The render-tree helpers and the container's image table came with 4.2. The clock-read order is compared since 5.2 (each read written down as `K`); alike again on f0a070d with the re-recorded `corpus.twrec` (59 ops batches), every layout's `measure_words` answered from the tape and the name meter charging its own thread's names |
| 4.5 | done: `twidget_renderer`'s `layers.rs`: renderer.cpp's stage 4.5 members as `Layers` and its code as `Core` methods over `Engine` (the `Layers` trait and `NoLayers` gone): heavy boxes (the document's box walk, recounts with a scripted layer's opacity, rule 5 through `sync_motion`, `count_paint` with paint reports and the paint/backdrop stops, `stop_paint`, `policy`), `update_composition` (its key, movers and nested layers, shimmers, scripted, SVG and canvas layers, still layers over a wanted one, clips, coverage rounds under 256 layers and 4096² px, orphans), `strip_layers`, scripted layers moved without a build, the paint (the list handed out again, layer content kept or redrawn with the selection's tint, `drop_inert_clips`, summary and `damage_between`, `damage_json`, `composition()`), `layer_shown` and `widget_id_at` through layers; litehtml's part (box walk and facts, `layer_candidate`, `clip_of`, `background_tile`, the paint trace, `covered`, `obscurers`, the draws, `paint_key`, hits inside a layer) behind `Engine`. `renderer_oracle` probes those calls (lists in wire form), paints, and dumps the stage's whole state every step: 4.3's 2569 scripts again plus 1809 alike, 1.66M answers (400 layer pages, 400 heavy-box pages, the paint-policy fixture's 20 cases × 3 policies × compositing, 200 damage, 150 throttle, 412 corpus + 127 stress widgets under both policies); 22.3k paints, 90.6k animated / 7.9k still / 3.6k shimmer / 1.2k canvas / 282 scripted / 190 SVG layers; 309 paint stops; core_tests' layer, damage, coverage and heavy-box checks by name (14). litehtml's half came with 4.2. Left: motion names (dangling views in the C++) not compared |
| 4.4 | done: `twidget_events`: pointers and their pointer/mouse/touch events, coalesced moves and movementX/Y, hover chains, listeners and prevented presses, trusted presses, the gesture claim and widget-sandbox's arbiter (holds, double taps, contextmenu, the bubble's verdicts), pointer capture, the `<details>` toggle, `<img>` load and error, ResizeObserver reports, focus and the platform's editor, text selection with its run cache, carets, long runs, `selectable_text_json` and the tint in documents and layers; the tree is 4.1's `Document`, and what layout, the cascade and the rest of the renderer answer goes through an `EventHost` trait in the C++'s order. `events_oracle`: renderer.cpp copied with probes at the input code's calls (every pattern counted at configure), a tape of the C++ document's answers the port replays question by question; 3975 scripts and 436k steps alike (1.65M answers): corpus.twrec's sessions and 200 with gestures, the gesture, interactive and showcase widgets ×12, the rest of the corpus, 1500 generated documents with listeners, 400 selection, 300 capture, 300 editor scripts, 160 crossing the layout budget from input; core_tests' event, gesture, focus, editor and selection checks ported (17 tests, 2 of the port's own); 247 of 254 blocks reached, 4 unreachable in the C++ too |
| L0 | done: `twidget_litehtml` begun, one module per C++ file: `Px` with its fuzzy comparisons, the name table (`Names`, 383 names at start), `t_strtod`, html.cpp with `t_strtof`, UTF-8 and code points, `num_cvt`, every keyword table and enum, `css_length` with calc, `web_color` (hex, rgb, hsl, names, `darken`) behind a `ResolveColor` trait, `parse_angle`, and `cxx` for what each platform's C++ leaves undefined; `lhdump` (0.2 for litehtml: the fork compiled as twidget builds it, `LH_NO_*` cleared and set per case) with ~470k answers alike (120k number texts, 40k `Px` triples, 60k colour tokens, 20k hsl, 40k clamps, darkens and hex forms, 60k lengths, 22k angles, 30k list markers, 70k UTF-8 and string helpers, the name table); since 0.4 the names each thread added (`_id_thread_usage`, `Names::thread_usage`) |
| L1 | done: the CSS tokenizer and the parser's normalizing half (`componentize`, `normalize`, `remove_whitespace` both ways, the nesting limit of 128, comma lists and value checks); ~490k answers alike: 147k from the corpus's 1376 style texts and 25507 declarations, 11k from master_css, UA_EXTRA and the profile's UA layer, 180k from 60k generated texts reaching every token kind, 152k colour and length texts |
| L2 | done: the rest of css_parser.cpp (stylesheets from bytes or tokens, rules, at-rules, declarations with `!important`, nested style blocks), `stylesheet` (`@charset`, `@import` with its media, nested `@media`, `@supports`, `@layer` as the fork marks it), all of media_query.cpp with three-valued answers, `encodings` (UTF-8, UTF-16, BOM sniff), unit conversion, `parse_url`, a `DocumentContainer` trait, and the renderer's `rewrite_color_scheme` (how `prefers-color-scheme` reaches litehtml); selector counts and `@supports` property checks wait for L4/L5 behind a `StyleSheetHost` trait. ~165k answers alike, every walk and media answer over a grid of 32 media feature sets: the corpus's 390 sheets and 1376 style attributes, master_css and the UA layers rule by rule, 12k generated sheets, 30k media query lists, 60k declaration blocks, urls, decodes and conversions, 22.7k colour-scheme rewrites |
| L3 | done: Gumbo ported (`twidget_litehtml::gumbo`, one module per C file: the UTF-8 reader, tags, char refs with char_ref.c's Ragel machine and all 2242 actions, errors, the tokenizer's 68 states, tree construction with every insertion mode and its limits; tables generated from the C by `tools/gen_gumbo_tables.py` and checked against it); lhdump `j`: ~265k parses alike field by field, errors included. The DOM: `Document` as `createUpdatableFromString` builds it, an arena of `Element`s by `ElementId` with one `ElementKind` per class (29): `create_node` by tag, words and spaces by `split_text`, comments, CDATA, templates not built, the depth (128) and element (20000) limits, `sanitize_element`, the node map and `build_template`'s literals and holes; each class's `appendChild` and `parse_attributes` (presentational attributes as declarations kept for L5, the form controls' generated boxes and state, `<style>`/`<link>` sheets, the container calls), html_microsyntaxes.cpp, `strtof` and `%g` in `twidget_cstd`; `SelectElements` on the document. lhdump `i`/`I`: 34452 documents and 21018 replays (32328 declarations) alike (corpus pages, snapshots, quirks mode and changes, generated documents of every class and attribute, every control type, number texts, tag soup and serialised trees, the limits); L4's matrices run on the Rust DOM (3455 of 3472 documents; 17 need generated content). Left: `createFromString`'s encoding sniffing, fragments, `::before`/`::after` content (L5/L6) |
| L4 | done: all of css_selector.cpp (every selector form the fork takes, `:nth-*` with `of S`, `:is()`/`:where()`/`:not()`/`:lang()`, pseudo-elements with the renderer's parts, the 128-compound limit, forgiving and strict lists, quirks lowercasing, specificity, `sort_selectors`) and html_tag.cpp's matching over a `SelectElements` trait (the combinators' searches with the descendant pruning, the `:nth-*`/`:only-*` counts, `select_all`/`select_one`; the budget ported, compared with L5); the stylesheet walk parses its own selectors now. Match matrices (every selector × every node, pseudo-classes checked and not, `:hover`/`:active` flipped up the chain) alike with the fork on the fork's own DOM (lhdump `h`): 412 corpus documents with their sheets and the UA layers on 52, 3000 generated documents (a quarter in quirks mode) with generated sheets, master CSS and UA layers against 60 more |
| L5 | done: style blocks (`style.cpp`: `style::add` with the declaration cache as an object the process owns, every property's parser into the fork's typed values, shorthands, CSS-wide keywords, `!important`, the `-lh-raw-` twins, `var()` and folded math), gradients and images, `css_effects.cpp`'s value parsers; and the cascade: `finalize_from_external_root` (master and user sheets as trusted, `:root`, attributes, document sheets under their media, `media_changed`), `apply_stylesheet` with the selector budget (the sibling index's charges, the `LH_NO_*` switches as `Switches`), `::before`/`::after` with their content (quotes, `attr()`, counters, images, the generated-content limits), counters, `refresh_styles`, `find_styles_changes`, `compute_styles` with style sharing, `get_property` with `inherit`/`initial`/`unset`/`revert`, `var()` on elements, every field of `css_properties::compute` (fonts through the container), the form controls' look, SVG paints; `computed_style::Element::of_cascade`. lhdump `l`/`f`/`y`: 107.5k declaration answers and 2940 cascaded documents alike field by field (the corpus's 412 pages light and dark at two viewports with media changes and `:hover`/`:active` flips, 255.9k elements; every tag under the master sheet and UA layers; 1500 generated documents and sheets; the budget crossed; 604 documents under the four switches), the container's calls alike but for the render tree's; 146.4k elements' computed-style records equal computed_style_oracle's, with 1.17M `cs` answers alike. What the render tree writes into the cascade (`list_index`, column widths, anonymous boxes) is L6's |
| L6 | done: the render tree (`render_item.rs`, `render_block.rs`, `render_flex.rs`, `render_table.rs`, `table.rs`, document.cpp's tree building): an arena of render items by slot-and-use id with the C++'s ownership counted (children, the root, the tabular list and a table's grid own; parents and `m_renders` are weak), every class, `create_render_item` of each element class, each `init()` (`list_index`, `split_inlines`, anonymous blocks and flex items, the grid with spans, `limit_columns` and the 200000-cell budget, column widths written into cells), `fix_tables_layout`, `html_tag`'s anonymous constructor running the cascade (slots reused); built by `finalize_from_external_root` and by `media_changed` as the C++ does. lhdump `o` with `r`: 3414 documents (12.6k dumps, 1.07M items) alike item by item with their calls (the corpus light and dark at two viewports with media changes, every tag, 41 written shapes, 2500 generated documents); `l` and computed_style_oracle without their accommodations, alike. Layout starts at L7 on these items |
| L7 | done, with L8, which the C++ does not separate: render_item.cpp's layout (`layout.rs`: `render` with the layout cache, its measures and marks, relayout boundaries, the viewport's epoch, `LH_LAYOUT_CACHE_CHECK`; `measure`, outlines, auto margins, relative shifts, the containing block, `fetch_positioned`, the out-of-flow pass, baselines, `y_shift`, sticky offsets and rendering boxes, the document's size and scroll views, `document::render` with the renderer's budget), render_block's `_render` and `place_float`, the block context (margins collapsing, clearance, boxes below floats, auto margins, rtl), formatting_context.cpp, render_image.cpp, each class's content size; layout state on `RenderItem`; the container's viewport, image sizes and a handle-based text breaker; the four layout switches. lhdump `Y`: 4773 cases, 22.1k dumps, 2.94M items alike with the container's calls (the corpus light and dark, 382 of 824 cases, the rest reaching L9–L12; every tag; 45 written shapes; 3000 generated documents; the four switches; the budget crossed); 14.8k cache hits, 21k floats, 11k clearances, 24.6k collapsed margins. L9–L12 came after; `DocLayout::unported` is gone |
| L8 | done with L7: render_inline_context.cpp with iterators.cpp's walk (skipped spaces, trailing room, inline boxes, texts cut into fragments by the container's breaker, floats and `fix_line_width`, `text-overflow: ellipsis`, margins through lines, measure's shortcut), line_box.cpp (alignment and justification, baselines, `vertical-align`, strut and line heights, quirks mode, `reorder_bidi`, relatively positioned inline ancestors, inline boxes, `new_width`); compared in the same `Y` cases: 653k lines, 57k fragments of 18.4k texts, 1483 ellipses, 41.7k right-to-left lines, every white-space mode, text-align and vertical-align keyword, `text_width` and breaker calls in order; since 0.4 texts that owe their widths until read (`measures_text_later`) and `breaks_in_line`/`breaks_at_narrowest` as the C++ asks them, compared with lhdump's containers measuring later (1,844 cases, 7,788 dumps, 802,869 items, container calls in order) |
| L9 | done: positioned layout (`layout.rs`): `render_positioned` with every case of CSS 2.2 10.6.4 and 10.3.7 as the fork writes them (auto margins zeroed or solved, min and max against the containing block in the box's sizing, a replaced box's leftover split between its auto margins, flex boxes measured in the room the offsets leave), `element_static_offset`, the holders walk, each positioned list stable-sorted by `z-index`, the fixed boxes' redraw boxes (`get_redraw_box`, `m_fixed_boxes`, `get_fixed_boxes`), `render_fixed_only`/`render_no_fixed`, `get_placement`/`calc_placement` with sticky offsets; `DocLayout::unported` keeps only tables and grid. Alike: the corpus light and dark at four widths each run (702 of 824, 614 before; the rest reach grid or tables), 41 written positioned shapes per branch and 4 sticky ones (180 of 180), 2500 generated positioned documents (13.5k dumps, 1.91M items, every one of the 16 offset cases), the five layout switches (758 of 758 each); 124 positioned levels lay out on 320 KiB optimized, 512 KiB unoptimized |
| L10 | done: flex layout (`render_flex.rs`, `flex_line.rs`, `flex_item.rs`): lines and wrapping with gaps, `order`, flexible lengths (grow and shrink, frozen items, factors under one, min and max violations), cross sizes and first and last baselines, `align-content` (stretch, wrap-reverse), `justify-content` with every fallback and the split's remainder, main and cross `auto` margins, every `align-self` with stretch laid out again, the container's baselines; the lines own their items in the arena; `LH_NO_COLUMN_REUSE` in `Switches`. lhdump `Y` dumps a flex's baselines and lines with every item. Alike: the corpus light and dark at ten widths (614 of 824 per run, 382 before; the other 210 reach grid, absolute boxes or tables), every tag with text fields and selects, 68 written flex shapes ×4 (268 of 272, a range's thumb is absolute), 2500 generated nested flex documents (90k flex items), the five layout switches; 242 of 250 blocks reached, the other 8 unreachable in the C++ too. 124 nested flex levels lay out on 384 KiB optimized, 640 KiB unoptimized |
| L11 | done: grid layout (`render_grid.rs`): render_grid.cpp's `_render_content` with every helper: `make_tracks` (one `auto-fill`/`auto-fit` repeat counted from the definite sizes, room and gap, at most 1000 tracks), lines and spans resolved (negative lines from the explicit end, clamped), `place_cells` (both lines, row only, then sparse row-major auto-placement; past the 200000-cell budget items stack in rows of their own), columns grown to the areas and auto-fit's empty repeats collapsed, items measured, `size_columns` (single-column items by track type, spanning items, growth to limits in 8 rounds, `fr` shares with frozen tracks in 16, `auto` tracks stretched), the reported widths, `justify-content`, rows from the template's plain tracks and `grid-auto-rows`, `align-content` and `align-self`; absolute and fixed children's in-flow pass. With L12 nothing is unported: `DocLayout::unported` and the tests' skip path are gone. lhdump `Y` dumps every grid pass (render_grid.cpp copied with a probe, `lhdump_grid.h`; the port through `DocLayout::grid_trace`). Alike: the corpus light and dark at ten widths (824 of 824 per run, 750 before; 74 reach grid), 74 written grid shapes ×4 (296 of 296), 2500 generated nested grid documents (512k grid passes of 861k items), the five layout switches (1182 of 1182 each); 184 of 185 blocks reached, the other unreachable in the C++ too. 124 nested grid levels lay out on 288 KiB optimized, 512 KiB unoptimized |
| L12 | done: table layout (`render_table.rs`, `table.rs`): `render_item_table::_render` with the grid taken from the table while its cells lay out, `resolve_collapsed_borders` (each cell's `collapse_skip`), the spacing of separate and collapsed borders, the cells' minimum and maximum widths (fixed tables from the first row's CSS widths, one-column tables, measures at no content width and at the table's width or the column's CSS width), `distribute_min_width`/`distribute_max_width` for spans, `calc_table_width` (auto, fixed and % columns in floats as written, 1/64 px rounding) and `calc_fixed_table_width`, the columns' and rows' places, the cells laid out at their columns' width as a second pass, rows from their cells and rowspans, `calc_rows_height` with the table's height and min-height, cells placed and vertically aligned, captions above and below; `DocLayout::unported` keeps only grid. lhdump `Y` dumps every item's `collapse_skip` and each table's grid as laid out. Alike: the corpus light and dark at four widths each run (750 of 824, 702 before: all 48 table cases; the rest reach grid), every tag with its tables (8 of 8), 49 written table shapes ×4 (196 of 196), 2000 generated table documents (10k dumps, 2.58M items, 63.6k tables, 235k cells), the five layout switches (958 of 958 each); 203 of 206 blocks reached, the other 3 unreachable in the C++ too; 31 nested tables (126 DOM levels) lay out on 160 KiB optimized, 384 KiB unoptimized |
| L13 | done: painting (`paint.rs`, `background.rs`, `borders.rs`, each class's draw in its module): every paint call of `document_container.h` on `DocumentContainer` in the C++'s order (the pure ones without bodies), `document::draw` with culling and the root's group, `draw_stacking_context`'s passes, `draw_children` with overflow clips and radii, the renderer's skips, culling by each item's drawn ink, fixed boxes escaping clips, `draw_group` (opacity, transforms with the culling clip mapped back, filters, backdrop filters, clip paths, sticky), the ink cache by paint generation, `get_visual_transform`, backgrounds of every layer kind and the three gradients, `html_tag`'s shadows, borders, outlines and list markers, texts and ellipses, form controls, tables' rows, cells and collapsed borders; an `<svg>`'s (L14) and an `<img>`'s (L15) own drawing as seams (`paint_unported`). lhdump `Q` (`lhdump_paint.h`): paint steps with the renderer's settings, every call logged with its arguments, the ink, visual transforms and looks dumped. Alike: the corpus light and dark at four widths in two runs, each case painted six ways (824 of 824 each, 8k paints, 192k calls), every tag (8 of 8), 60 written shapes ×6 (360 of 360), 2500 generated documents (18k paints, 346k calls), `LH_NO_TRANSFORMED_CULLING` and the five layout switches (210 of 210 each); 6.5k image and 6.4k SVG seams stand for 57k C++ calls; 463 of 481 blocks reached, the other 18 unreachable in the C++ too but one the port's own; 124 levels lay out and paint on 259 KiB optimized, 450 KiB unoptimized. Not yet: element scroll positions (`on_scroll`, L16), `draw_cached`/`draw_isolated` (reached only in SVG painting); since 0.4 the paint switch run measuring later alike (210 cases, 1,380 paints) |
| L14 | done: SVG (`el_svg.rs`, `svg_paint.rs`). The geometry half over `SvgElements` (numbers and flags, path data with SVG 2's error handling, arcs, outlines, `transform` attributes and CSS transforms with fill-box, `svg_ctm`, view boxes, intrinsic sizes, `view_group`, a `<use>`'s placement with its caps (depth 8, ids of 1–128 bytes, 10000 a pass, 16 a lookup inside a `<use>`), `svg_parent`, gradient coordinates, path length), and painting: `svg_property` through the `<use>` hosts and the paints, numbers, keywords, opacities and gradient stops read through it; the painter's drawing (groups with opacity and transforms, visibility and display as attributes and style, shapes filled and stroked with colours, `currentColor`, `var()`, gradients, dashes, caps, joins and the fill rule, texts and `<tspan>`s through the container's widths, `<use>`, nested `<svg>`s as groups, a top-level `<g>` through `draw_cached` under its key), `svg_text_runs`/`svg_text_box`, `paint_signature`, `el_svg::draw` (the `<use>` budget per paint generation, `cached_paint_key`/`cacheable_paint_key`, `draw_cached` or `draw_isolated`) and `paint_shapes`; `LH_NO_SVG_GROUP_CACHE` and `LH_SVG_LATE_CACHE` in `Switches`. svg_oracle: 134k answers, 3520 documents alike (geometry). lhdump `Q` compares SVG whole (no seam; `draw_cached` answering as a cache; paint signatures, kept state, text boxes and the `<use>` budget dumped; every NaN as one): L13's runs now compare their 6.4k SVG paints (50k calls); the corpus's 85 SVG pages and 207 `<svg>`s light and dark at four widths with `:hover` and media changes (584 of 584, 6.1k SVG paints, 95.5k calls), 34 written SVG documents ×6 (204 of 204, 3.5k SVG paints, 799k calls), 2500 generated (20.6k SVG paints, 263k calls), eight switches (180 of 180 each); 189 of 194 blocks reached, the rest unreachable in the C++ too but one the port's own; 124 SVG levels on 71 KiB optimized, 263 KiB unoptimized. Not yet: hit testing (`element_at`, the painter's hit mode) with L16 |
| L15 | done: el_image.cpp's drawing (`el_image.rs`): `draw` (the content box offset and rounded at the snap scale, the layer clipped to it, the border box with its radii, no repeat, no base url, an empty source drawn too) and `fitted` (object-fit and object-position from their raw declarations, the container's aspect without a fit, contain/cover/scale-down, keywords, percentages, lengths); `<canvas>` and generated images are `el_image`s; `DocumentContainer::get_image_aspect`; `paint_unported` gone. lhdump `Q` compares images whole (logged `N`): the L13/L14/L16 runs now compare the 6501 image draws formerly behind the seam (13002 C++ calls); 11 written image documents ×4 (44 of 44) and 1200 generated (31.1k draws); 36.5k draws, 82.8k image calls alike; 32 of 32 probe points reached. Left for 4.2: decoding `data:` images, `tw-canvas:` sources, `draw_image`'s commands, the image limits |
| L16 | done: hit testing and the mouse state (`hit.rs`): render_item.cpp's `get_element_by_point` in painting's z-order passes (fixed boxes at their client point, sticky shifts, clip paths through `point_in_clip_path`) and `get_child_by_point` (a clipping box's content box, scroll positions, transforms inverted with the 1e-12 determinant cap, a check passed by one of the child's render items), each class's `is_point_inside`; scrolling (`ScrollView`, the item's and the element's through its first render item, `scroll_box`), `element::get_placement`; document.cpp's `on_mouse_over`, `on_lbutton_down`/`up`, `on_button_cancel`, `on_mouse_leave`, `on_scroll` (`doc_on_*`, `ScrollValues`) with the hovered and active elements, cursor, mouse events, clicks and redraw boxes; `el_svg::element_at` through the painter's hit mode; `update_structure`'s hover remap wired (hovered and active anonymous elements kept alive as the C++'s `shared_ptr`s do; an anonymous element's anonymous parent is weak). lhdump `W` (`lhdump_hit.h`): patches, layouts and paints between pointer steps, every answer, check, redraw box, scroll value and container call. Alike: 5318 cases, 626k steps (the corpus light and dark and at four more widths, its SVGs, 54 written, 2500 generated with patches between, six switches ×137, two deep documents): 439k hit tests, 182k checks, 75k pointer events, 4.4k scrolls, 20k SVG points; 107 of 112 blocks reached, the other 5 unreachable or undefined in the C++; 124 levels on 68 KiB optimized, 94 KiB unoptimized. The renderer's calls (`element_at`, `element_at_within`, `mouse`, `leave`, `widget_id`) came with 4.2 |
| L17 | done: `crates/litehtml/LIMITS.md`: 72 limits, budgets, caps and clamps of the fork and Gumbo, each with its C++ place, value and comparison, its Rust place and the test that crosses it; the 27 `getenv` switches with their tests. 43 crossed by earlier stages (21 now at their boundary too), 23 newly, 6 not comparable (the charset prescan and fragments not ported, the sheet caches whose hit equals a parse, three unreachable guards). `tests/limits.rs`: 25 tests, 418 cases alike under, at and past each limit (`Q`, `j`, `b`, `W`); a planted off-by-one in `max_generated_boxes` failed |
| L18 | done with L19: the layout cache compared with it on in both after every patch: `m_layout_marked` and `forget_positions`, `m_layout_origins`, `begin_layout_patch`'s snapshot, `layout_dirty_regions` after each relayout (3430 with regions, 11.3k dirtying nothing), every item's cache and marks in the `b` dumps; each relayout after a patch against a fresh document of the same HTML in both languages: 24953 alike, 194 not, all the C++'s (see Findings) |
| L19 | done: `patch.rs`: document.cpp's patching half (`update_from_string` through the template or `update_structure`'s diff by order or `data-lh-id`, `update_styles`, `update_holes`, `apply_template_values`, `restyle_after_patch`, `replace_text_renderers`, `selector_scope`, `retag_allowed`, `apply_retag`, `template_html`, paint-only patches and their regions) and html_tag's flips, memos and retags; `Switches::patch` (15 switches); generated content given back when the C++ box would die. lhdump `b`: 6529 cases and 161.9k steps alike (412 corpus pages edited, 400 shadow-tree sessions from 4.1's serialiser and patch log, 70 written cases, 3000 generated, 18 switches ×147, 124 levels on 320 KiB optimized and 512 KiB unoptimized); 12962 HTML, 5169 hole and 663 style patches, 9295 refusals, 5001 paint-only; 45 cases with tied holes left out (L3's `std::sort` finding); 576 of 600 blocks reached, 15 of the rest unreachable, 5 waiting for L16's hover, 4 for an inline root. `update_structure`'s hover remap came with L16, the renderer on this document with 4.2; since 0.4 147 patch scripts (3,121 steps) measuring later alike |
| 5.1 | done: `twidget_ends`: `RendererEnd` over a `Renderer` trait (renderer.h's calls) with its `Output`, the backend `Proxy`, `Backend`/`Clock`/`Channel` traits, and `PlatformEnd` with a `Host` trait for the tests; ends_tests.cpp's 11 tests ported by name against a scripted renderer; `ends_oracle` (renderer_end.cpp beside a scripted C++ renderer, since the C++ end constructs the renderer itself) alike on 48 written and 4000 generated sessions (97.8k steps: wire traffic, renderer calls, Backend calls, host reports, every stop reason and the platform end's refusals). The real renderer's own answers (pixels, the throttle, clocks) come with stage 4; since 0.4 widths batched within the caps, painted frames sent as deltas on the last painted list, the platform end checking a widths request whole, rebuilding deltas and closing: 21 ends tests by name, `ends_oracle` alike on 52 written, 4,000 generated and 1,500 delta sessions (4,241 delta frames, 6,561 whole, 20,084 widths requests) |
| 5.2 | done: `twidget_replay` (`tests/recording.h`'s reader and writer with its version check, `wire_compare.h`, `replay.cpp` on `RendererEnd`/`PlatformEnd` with the renderer on `twidget_document`; Backend answers and clock reads from the recording, compared in order). Alike step for step: `corpus-stub.twrec` 438 steps, its list, Backend-call and key perturbations caught at their leaves; the corpus 412 sessions, 30,290 steps, 6,391 frames (5,817 painted, 574 unchanged), 4,098 renderer messages, 44,948 Backend calls, 180,783 clock reads, 4,951 keyed commands (`gate.py --corpus`, 51 s unoptimized); `core.twrec` and `bench.twrec` after one port fix (a refused scripted move read the clock once more than `apply_scripted` does, 84abf8b; renderer_oracle now writes every clock read down, so 4.5's comparisons would have caught it); `corpus.twrec` up to its step 132 (0.3's note). A planted width bug failed each recording at its `SetWidth(240)` |
| S1 | done: `twidget_cpu_budget`, `twidget_sdk_policy`, `twidget_gesture`, `twidget_bundle_stamp` (with version.h and `SandboxResources`); the three shared fixtures and the C++ tests ported by name (13 tests), ~1.14M inputs alike with the C++ (246k budget steps, 590k SDK checks and gate steps, 203k gesture claims and steps, 100k stamps and 300 bundle directories) |
| S2 | done: `twidget_js::JsThread` and `Poster` (8 MiB stack, tasks, timers in deadline order, the sweep); a panic on the thread aborts, as an exception leaving a JSThread task terminates |
| S3 | done: `twidget_js::NovaContext` on `nova_embed`; test_main.cpp's 8 context tests ported (recursion depth, timers, a string past the limit, host function failures, results that cannot be copied, bundles compiled once, arguments as the C interface gave them, deadlines) |
| S4 | done: `twidget_engine` (sandbox_engine.cpp: boot, every host call kind, the quota and the main-thread window, deferrals, timers, frames, scripts, the bridge, storage, requests and the wallet, the query guard, stats, the kill texts) and `TrustedValidator`; 4 engine and 6 validator tests in process on the shipped bundles, the rest through T6 |
| S5 | done: `twidget_server` (`SandboxRemoteServer`, the serving loop with its Output, SyncSlot, EngineSink and routes) and `twidget_engine_ffi` (the C ABI, 29 `unsafe`; `nova-c-api` adds Nova's C interface for a C++ program that drives Nova). T6 in process through the C ABI rather than in a child: widget-sandbox `-DTW_RUST_ENGINE_END=ON`, `TW_ENGINE_STREAM=1 TW_ENGINE_END=rust`: at cac5a6c and nova-safe 906ebcf 244 tests, 14157 checks, 0 failures, 128 of 128 fixture engines over the stream (155 channels on 8 streams), every result and count the C++ end's (x64 and Win32); 133 attacks, 0 leaks, 7 killed, compat 19/19 and generated 20/20 with every per-file row the C++ end's; the end dropping diagnostics failed 34 of 36 `sdk_` tests |
| S6 | done: `twidget_host` (widget-sandbox): `SandboxHostedRuntime`, `HostedPlatform` with the glue's wait accounting, the served `WidgetHost` and its local glue behind a `Renderer` trait, `serve_hosting` (the hosting opening, its kinds, the platform slot); `twidget_engine_ffi` gains `twidget_engine_serve_hosting` with a C renderer factory and renderer table (79 `unsafe` there in all, each with its SAFETY note). Under `TW_ENGINE_HOSTED=stream` both ends give 244 tests (14158 checks) and 0 failures at cac5a6c, the same frame tallies, 133 attacks with 0 leaks and identical per-file rows, compat 19/19 and generated 20/20 alike; `hosted_` 52 and `remote_stream_hosted_` 30 (Windows) pass on the Rust end; cac5a6c's answer times are host-side, nothing to port; a planted glue-wait bug and (2026-11-21) an emptied platform answer each failed their tests (6 of 52 `hosted_` and the crate's own). The renderer is the C++ harness's through the C ABI until stage 5. On nova-safe's tdesktop-perf line (32-bit capable) the engine-only suite passes on Win32 too, both ends, row for row |
| H1 | done: `twidget_helper` (tdesktop `private/js-sandbox`'s helper glue, 05a27c6625): the hosted wire, the renderer rules, the Debug tap with its forwarding renderer and work records, the cursor styles on the Rust litehtml (`twidget_document::Process`), the hosted glue (a renderer end on `DocumentEngine`, `make_hosted_renderer`) and `serve` (`twidget_host`'s serve_hosting with the glue, tapped in Debug, on the OS glue's clock), behind `Renderer` and `Platform` seams the tap sits between; the renderer's clock and canvas font hook are `Send`; the workspace repeats nova-safe's `[patch.crates-io]` (59 crates, 0 `unsafe`). Oracles compile the C++ from a tdesktop checkout (`TW_TDESKTOP_SOURCE`, skipped without one): rules 154.6k cases; tap 3000 generated and written sessions, 73.5k steps byte for byte; cursor 1612 documents, 22.6k pointer calls, 730k element cursors; the glue end to end over the platform stream, 1212 sessions (412 corpus, 800 generated), 70.1k steps byte for byte (69.7k platform messages, 102k requests and answers, 30.6k host messages, 778 diagnostics); `ExerciseHelperWork`'s decoder verdicts ported; a planted bug failed in each. The OS glue keeps the entry, `Adopt`, `Lockdown`, `Exit`, the XPC service and the OS answers. Left: widget-sandbox's desktop suite in hosted mode on the glue needs a `twlite::remote` platform end in its harness |
| H2 | done: `twidget_helper_ffi` (static library and rlib, `include/twidget_helper.h`): `tw_helper_serve(stream, platform, build, build_size, lockdown, tapped, reason_size)` and `tw_helper_free`; the stream (read, write, shutdown), the OS's answers (thread CPU seconds, steady seconds, the tap's steady micros, time zone, CSPRNG bytes) and the lockdown as C tables of callbacks with a context, only bytes and numbers crossing; a guard that serving closes before it returns (no callback after it), `catch_unwind` and an abort around both entries; `twidget_helper::serve` takes the tap's clock from the OS glue. 30 `unsafe` in the crate's sources (`AUDIT.md`: each with its invariant, the header's contract and its tests; threads, lifetimes, broken contracts); 0 outside it of 61 crates. From C++ (`oracle/helper_ffi_oracle`: tdesktop's `WidgetHelper::Serve` on widget-sandbox's C++ engine end beside `tw_helper_serve`, one Nova, one step clock, every frame compared by runtime and kind, `Stats`' engine heap masked): 412 corpus sessions tapped, 20,047 steps alike, and 800 generated untapped, 46,498 steps alike; 240 mutated streams to the same reason, peak commit 56 MiB; NULL tables and callbacks, ended, cut and write-failing streams, a failed lockdown, empty and 64 MiB builds and no lockdown alike in both; 12 streams ended under load with no callback after return; a panicking Rust callback aborts. Imports, x86_64-win7 and i686-win7, helper-like (`mainCRTStartup`, static CRT, build-std, CFG): KERNEL32, ADVAPI32 (`SystemFunction036`) and ntdll alone, no delay-load needed. Left: Linux and macOS builds of the library with their import and syscall checks (H3). Since (2026-11-22, nova-safe `rust-safe` 9d0e4423, widget-sandbox 32bd364): the helper's global allocator is nova-safe's metering allocator, `nova_meter` (moved out of `nova_embed_ffi` into a crate of its own; `nova-c-api` leaves it to `nova_embed_ffi`), and `twidget_js` gives each engine an account, entered where nova_embed_ffi's C interface enters it, the host's part of a host function call on the uncounted account and results copied off the engine's blocks, so the memory limit and `Stats`' engine heap count what the C++ helper's do. `TW_HELPER_ORACLE_MEMLOG` logs that heap for both arms: two differences found and removed (the boxed host, 16 bytes against `CHost`'s 40, now five words too: 24 bytes on every engine; host function names interned on the engine that registered them first, now on none: 50 bytes on each run's first); then 1434 of the corpus's 2505 `Stats` alike to the byte and 1882 of the generated 6313, the rest by multiples of 32 bytes mostly, where Nova's random hash keys decide when a table grows, as much as C++ against C++ (1433 of 2505) and Rust against Rust (1425); with the keys pinned (a probe build) 2504 of 2505 and 6313 of 6313, the one left a session's end that varies C++ against C++ too. The heap stays masked in the comparison. `unsafe`: 15 in `nova_meter` (audited in nova-safe, its tests and Miri), allowed beside this crate's 30; 0 in the other 61 crates |
| H3 | Windows and Linux done (2026-11-21, tdesktop `js-sandbox-rust` on `private/js-sandbox` 33fd4d91cd; twidget e65a8db, widget-sandbox ef3617e, nova-safe c6d4896): `TDESKTOP_WIDGET_HELPER_RUST`, OFF by default (the helper built as before); ON, `build_widget_helper_rust.cmake` builds `twidget_helper_ffi` as a staticlib in the tdesktop build (always run, `--locked`, `copy_if_different`) the way Nova is built: on Windows for Nova's target per architecture with `build-std`, `+crt-static`, CFG, fat LTO and release overflow checks, on Linux for the host with the prebuilt std, LTO and overflow checks; TelegramWidgetHelper is the OS glue, its probes, `_other/widget_helper_rust.cpp` (`tw_helper_serve`, tapped in Debug) and widget-sandbox's `engine_platform.cpp` (no Nova) on that one Rust library. Debug builds the C++ helper beside it (`TelegramWidgetHelperCpp`), its digest in the Debug identity header alone (`kAlternateDigests`), launched with `TDESKTOP_WIDGET_HELPER=cpp`. Windows: imports alike with the C++ helper's but for a few KERNEL32 functions (x86 static ADVAPI32, KERNEL32, ntdll and the pc std's API set, delay bcrypt and bcryptprimitives; x64 static KERNEL32 and ntdll, delay ADVAPI32 and bcrypt; WS2_32 delayed in Debug); in the app (Win32 and x64 Debug, Test account) both helpers, launched by Telegram, served a conformance widget with the same 139 rows (48 ops self-check), the same widget checks and run file but timings, and the same 53 probe lines; on x64 the threads started after the lockdown drew their keys through the delay-load hook. Linux (`tdesktop:centos_env` with js-sandbox's rustup 1.96.1, the `linux` target, Debug and Release): the C++ helper's NEEDED, glibc at most 2.28, PIE, RELRO, BIND_NOW, a non-executable stack, 11.8 MB stripped against 13.2 MB; driven over its channel in Docker (default seccomp, no_new_privs, user, pid, net and mount namespaces) both helpers have one thread and descriptors 0-3 at the lockdown, serve a widget on Nova in threads made after it, each drawing its HashMap keys with `getrandom(GRND_INSECURE)`, which the filter allows, and have no call refused. Not run: ARM64, Telegram on Linux (H4). Left: macOS (per architecture and `lipo`, `getentropy`, no descriptor after the lockdown, signing), which needs the owner's Mac |
| H4 | Windows and Linux done (2026-11-21, tdesktop `js-sandbox-rust` on `private/js-sandbox` 866160d990; twidget 53dc8e2, widget-sandbox 9dc139e, nova-safe c6d4896): T7 in Debug, the C++ helper (`TDESKTOP_WIDGET_HELPER=cpp`) against the Rust one, through a disposable overlay that dumps every tap record by host. x64: conformance (181 runs), the attack audit (133 files), stress (the full ladder of 127 levels on 33fd4d91cd, a 26-level subset on 866160d990), the probe; Win32 (the win7 target since 866160d990): conformance and the probe on both bases, the attack audit on 33fd4d91cd. Rows and verdicts alike in every pack; widget checks alike but where the clock decides; stage crops byte-identical (74/74 x64; 73/74 Win32, the odd one a crop mid-transition that also varies C++ against C++); tap records alike (`b`, `t`, the `w` kinds and sequences everywhere; `h`, `r`, validated batches and platform messages outside clock-driven widgets, real-input positions and Telegram's request timing), with the same remainder as an A/A run of the C++ helper; the 53 probe lines alike on both architectures; stress levels alike in loaded, dead and measured (path-work counts at the paint limit vary run to run on either helper). One stable difference until the Rust helper metered its engines on `nova_meter` (twidget bd84c8c, nova-safe 9d0e4423): `auto/33-stop-batch` stopped at the 8 MiB memory limit with the x64 C++ end and at the ops limit with the Rust end; since then (tdesktop 4f9d0710fd, x64 and Win32 Debug) both helpers stop it alike (x64 the memory limit at 5236-5238 KB, x86 the ops limit at 6083 KB), the `mem-*` attacks die alike (`mem-burst-alloc` 6847 KB in both), and engine memory across 181 conformance runs per architecture and 126 attack runs differs by a median of 0 KB, its outliers clock-driven widgets that vary as much C++ against C++. Linux: a Debug Telegram built with `TDESKTOP_WIDGET_HELPER_RUST=ON` as the `linux` target ran in `tdesktop:centos_env` with Xvfb at a device pixel ratio of 2 (`QT_SCALE_FACTOR=2`, the crops need 2x), with only `seccomp=unconfined` granted (the launcher's namespaced `clone`), and the Test account through the same overlay; the Rust helper and the C++ one, each in its own user, pid, net and mount namespaces under seccomp, Landlock ABI 3 and no_new_privs, gave the same rows and verdicts in the probe (154/0/2, identical probe lines), the conformance pack (162/0/2, 92 of 92 crops and cards pixel-equal) and the attack audit (102/1/17, the same `render-nested-*` click-not-observed FAIL as Windows), tap records equal but clock-driven ones; `auto/33-stop-batch` differed as on Windows before the meter. Not run: the stress renderer timing (its timing overlay does not exist), network controls, Release in the app, ARM64. Left: macOS, which needs the owner's Mac |
| H5 | Windows, Linux and macOS verified (2026-11-22, tdesktop `js-sandbox-rust` ed818b080c, 546550cb2c, 63f00295c3, 97a1936fee and 2a656a512e on 4f9d0710fd; twidget 1d5dc5b, widget-sandbox 32bd364, nova-safe 9d0e4423): `TelegramWidgetHelper` is the OS glue and `_other/widget_helper_rust.cpp` on `twidget_helper_ffi` on every platform: no `TDESKTOP_WIDGET_HELPER_RUST` option, no Debug `TelegramWidgetHelperCpp`, no `TDESKTOP_WIDGET_HELPER=cpp` switch on Windows or Linux, and one digest table in the identity header again (no `kAlternateDigests`). The C++ serving left tdesktop: `_other/widget_helper_renderer.cpp`, `tgw/tgw_hosted_glue.*`, `tgw/tgw_cursor.*`, `test/test_widget_helper_tap.*`, the forwarding renderer of `test/test_widget_renderer_forward.h` (the helper work record Telegram's tap decodes stays), the `lib_widget_sandbox` engine library, `lib_widget_renderer` with its UA stylesheet, `lib_litehtml.cmake`, the `*_hardened` variants, and `lib_nova_engine.cmake` with `build_nova_engine.cmake`; Telegram keeps `lib_widget_sandbox_host`, `lib_widget_renderer_platform`, `tgw_hosted_wire.h` and `tgw_renderer_rules.h`, and links no Nova. `lib_widget_helper_rust.cmake` finds cargo itself (`TDESKTOP_CARGO`, the prepared toolchain's rustup and cargo homes, Linux's build-tree `CARGO_HOME`, now `widget_helper_rust/cargo`) and picks the Windows target; the helper's C++ hardening now covers its OS glue and `lib_widget_sandbox_platform`. Windows (x64 and Win32; Debug and Release helpers, Debug Telegram): the import tables of H4 (Debug: static KERNEL32 159 on x64 and 158 on x86, ntdll 2, delay ADVAPI32 23, bcrypt 1, WS2_32 19; Release: KERNEL32 115, ntdll 2, delay ADVAPI32 12, bcrypt 1); the probe's 53 lines as in H4 on both; x64 conformance against H4's last Rust run: rows and verdicts alike (162/0/2), the 24 deaths alike (`auto/33-stop-batch` at the memory limit, 5238 KB), engine memory a median 0 KB apart, 74/74 crops and 18/18 cards byte-identical, widget checks alike but where the clock decides (the time corner's width follows the hour). Audit: the helper links one Rust static library, `twidget_helper_ffi` (neither `nova_embed_ffi` nor `tdesktop_rust`). `cargo metadata --filter-platform` for `x86_64-win7-windows-msvc` and `i686-win7-windows-msvc`, the normal dependencies of `twidget_helper_ffi` without proc-macros and what only they use, gives the same 58 crates on both, their linked sources counted as `tools/unsafe_report.py` counts: `unsafe` in `twidget_helper_ffi` (30; 17 more in its tests) and `nova_meter` (15) alone, 0 in twidget's other 17 (animations, base64, canvas, compositor, computed_style, css, cstd, document, ends, events, helper, litehtml, paint, remote, renderer, shadow, wire), widget-sandbox's 10 (bundle_stamp, cpu_budget, engine, gesture, host, js, json, sdk_policy, server, stream), nova-safe's other 25 (bitflags, cow-utils, nova_embed, nova_vm, num-bigint, num-traits, oxc_allocator, oxc_ast, oxc_ast_visit, oxc_data_structures, oxc_diagnostics, oxc_ecmascript, oxc_estree, oxc_index, oxc_parser, oxc_regular_expression, oxc_semantic, oxc_span, oxc_str, oxc_syntax, regress, small_string, unicode-id-start, unicode-normalization, wtf8) and crates.io's 4 (num-integer, rustc-hash, tinyvec, xsum); at compile time alone: nova_vm_derive, oxc_ast_macros and seq-macro with proc-macro2, quote, syn, unicode-ident, phf, phf_shared and siphasher; std, core and alloc are the toolchain's (`-Z build-std`). The Windows run covered x64. Linux (2026-11-22): the Rust-only TelegramWidgetHelper (tdesktop ea75040b58, twidget 727a17e, nova-safe 9d0e4423 with `nova_meter`) built fresh as the `linux` target in `tdesktop:centos_env` and has the same NEEDED, glibc ceiling 2.28 and PIE, RELRO and BIND_NOW as before (13.2 MB stripped); it holds one thread and descriptors 0-3 at the lockdown, draws `getrandom` after it with nothing refused, and in a Debug Telegram under Xvfb it passed its pinned identity and gave the probe (154/0/2, identical lines) and the conformance pack (162/0/2, 92 of 92 crops pixel-equal to H4) with `auto/33-stop-batch` stopping at the 8 MB memory limit, as on Windows x64. macOS (2026-11-22): the Debug Desktop app ran eight widget fixtures using its signed direct helper. The direct helper and the signed Store XPC service both rendered a widget over their native transports. Cargo built the Intel and Apple Silicon slices, and the Store service passed signature and entitlements checks on both. |
| T9 | first measurement (2026-11-21, on twidget d79f4a4): `crates/bench` is `tests/bench.cpp` on the Rust renderer, the same scenarios sent as the same message strings, the frame loop, the stub backend (8 px a byte, nothing rastered) and the report. bench.cpp's steady clock lets the time decide part of the work: the render throttle holds back what follows a pass over 16 ms (the C++ built 0.16–0.89 times a frame for 1000 CSS animations, the Rust 1.00; at swarm 2400 and css 3000 both are held back nearly every frame) and the animations' progress per tick follows each frame's cost. So both also run on a step clock (`--step`: 1000 ms, a 60th of a second a tick), the C++ as `oracle/bench_oracle --step` (bench.cpp unchanged, its renderer wrapped; as fast as `twidget_core_bench` where the clock decides nothing, 0.97–1.04), and the work is shown the same: `--trace` writes every call of the loop as a line of hashes (the message, the answers, every Backend call with its arguments and answers, the encoded list, the damage, the composition with each layer's content, the counters), and the C++'s lines and the Rust's are identical in all seven modes at their defaults and at 3000 (swarm 2400), 605 to 2,104 lines a run; the crate's test compares eight small runs in the gate (a planted text width is caught at the first batch). Timed on the step clock (below) no mode is over 2x: the Rust renderer takes 0.30–1.06 of the C++'s time with the workspace's release profile (codegen-units 1, no LTO, overflow checks off) and 0.29–1.09 as tdesktop builds it (`build_widget_helper_rust.cmake`: fat LTO and overflow checks on that profile; `panic = "abort"`, which it does not set, changes them by 3% at most). It trails in swarm (receive 1.19x, the tick's coalesced build 1.18x at 2400, while its paint takes 0.6x), in css's paint (1.42x at 1000, 1.25x at 3000) and in css's setup (the first build and paint of the animated document, not in the per-frame numbers: 1.44x at 1000, 2.17x at 3000, superlinear in Rust: a sampling profile of the 3000 setup has 89% in the first paint's `update_composition`, 40% in `DocumentEngine::element_set`, which turns the movers' widget ids (and `covered`'s ignored ones) into a set of elements again for every `layer_candidate` and `covered` call where renderer.cpp builds `mover_elements` and `others` once, so quadratic in the animated elements (13% of the 1000 setup's samples, 35% of the 3000's), and 37% in baking the layers' tracks (`Baker::split` sampling the animations, `twidget_css` parsing their transforms); the heap's allocations and frees are 35% of all samples); it is fastest on canvas (0.30, all in receive: the batch's JSON and the interpreter) and on the large documents (0.55–0.63 at 3000). On bench.cpp's own steady clock the modes whose work it leaves alone give the same ratios (0.29–1.10 at the defaults, 0.31–0.69 at 3000). The helper's metering allocator (bd84c8c, `--features metered`) costs 4–18% a frame (11% at the median, most where receive builds: swarm +18% and +15%, transform +15% at 1000; least in drag-doc, +4–6%), which takes the shipped Rust to 0.30–1.22 of the C++'s time (swarm 800 1.22, swarm 2400 1.17, every other mode below 1.07); its traces stay alike with the C++'s. Optimized (2026-11-22, twidget a41ec73 to 0450178 on faa6853, nova-safe 9d0e4423, widget-sandbox 32bd364), each step with its traces alike with the C++'s in all seven modes at the defaults and at 3000 and twidget's gate passing after it (`--release --corpus`, the helper oracles built, the recorded corpus replayed; one debug run of 0450178 lost `twidget_remote`'s oracle pipe to Windows error 1450, insufficient system resources, in a crate none of the steps touch, and the gate run again passed): the layer pass makes each set of elements once where renderer.cpp makes `mover_elements`, `others`, `ignore_all` and `ignore` (`Engine::Elements`, `DocumentEngine`'s a bit an element; a41ec73, css setup 2.16x to 1.21x at 3000); a property patch's restyle moves its values as document.cpp moves them (the patches kept by reference, `replace_properties` taking the scratch style's values, a transform patch's tokens moved into it; 7fc9644, swarm 800 1.31 to 1.01, 2400 1.17 to 0.97); a transform's function arguments are read where the token holds them (copied only to fold a `calc()`) and its names compared without lowered copies (9c7bca6, swarm 800 1.06 to 0.96); a layer track's samples read the cached interpolation tokens, the running animation and the base values where the animator keeps them (4e732b3, css setup 1.22 and 1.21 to 0.97); and the hole patch takes each new value by reference and assigns it in place, as std::string does, finds substrings from their first byte and marks the holes it has seen instead of searching the list of them, quadratic in the changes in the C++ too (0450178, swarm 2400 0.88 to 0.83). With the metering allocator the Rust renderer now takes 0.29–0.89 of the C++'s time a frame (swarm 0.83 and 0.84, transform 0.83, css 0.89 and 0.86, mixed 0.80) and 1.00 and 0.95 of its css setup (the table after this one); css's paint alone stays over (1.35x and 1.20x), outweighed by its tick (0.82x and 0.81x) |

T9 on the step clock: µs a frame (receive, tick and paint), medians of 5 runs alternated with the C++ on one P-core of an i9-14900K at high priority (spread within 17% of the median), Windows x64, MSVC 14.51 `/O2 /Ob2` without LTCG, rustc 1.96.1; Rust/C++ in brackets. n is the count (the bench's defaults are 1000 and 800 for swarm); "shipped" is the release profile with `--config profile.release.lto=true --config profile.release.overflow-checks=true`, as tdesktop builds the helper (without its `+crt-static`, control-flow guard and `build-std`). "metered" is shipped with `--features metered`, nova-safe's metering allocator (`nova_meter::Metered`, a 16-byte header and an atomic add on every allocation and free) as the global allocator, as the helper installs it since twidget bd84c8c, everything on account 0 as the renderer's allocations are; timed on 2026-11-22 (twidget bd84c8c, nova-safe 9d0e4423) in a second set of runs, C++, shipped and metered alternated, whose shipped/C++ ratios are within 0.07 of the first set's: its cost is metered against shipped, and the ratio is against that set's C++.

| mode | n | C++ | Rust release | Rust shipped | shipped, `panic = "abort"` | shipped, metered: its cost (and /C++) |
|---|---:|---:|---:|---:|---:|---:|
| mixed | 1000 | 5812 | 5235 (0.90) | 5083 (0.87) | 5134 (0.88) | +11% (0.98) |
| transform | 1000 | 4381 | 4186 (0.96) | 4144 (0.95) | 4016 (0.92) | +15% (1.06) |
| swarm | 800 | 8240 | 8746 (1.06) | 9005 (1.09) | 8724 (1.06) | +18% (1.22) |
| css | 1000 | 16509 | 14829 (0.90) | 14254 (0.86) | 14185 (0.86) | +13% (0.97) |
| drag | 1000 | 39 | 33 (0.85) | 32 (0.82) | 31 (0.81) | +10% (0.89) |
| drag-doc | 1000 | 1166 | 758 (0.65) | 750 (0.64) | 747 (0.64) | +4% (0.65) |
| canvas | 1000 | 726 | 214 (0.30) | 211 (0.29) | 209 (0.29) | +13% (0.30) |
| mixed | 3000 | 18144 | 11123 (0.61) | 11258 (0.62) | 11061 (0.61) | +10% (0.61) |
| transform | 3000 | 13356 | 8464 (0.63) | 8488 (0.64) | 8352 (0.63) | +8% (0.67) |
| swarm | 2400 | 26008 | 27460 (1.06) | 26684 (1.03) | 26219 (1.01) | +15% (1.17) |
| css | 3000 | 67815 | 61101 (0.90) | 58601 (0.86) | 58077 (0.86) | +11% (0.98) |
| drag | 3000 | 82 | 50 (0.61) | 50 (0.61) | 49 (0.60) | +12% (0.67) |
| drag-doc | 3000 | 4074 | 2242 (0.55) | 2196 (0.54) | 2178 (0.53) | +6% (0.52) |
| canvas | 3000 | 2162 | 663 (0.31) | 649 (0.30) | 643 (0.30) | +11% (0.33) |

T9 after the optimization (2026-11-22, twidget 0450178): the step clock, core and priority as above; medians of 5 runs of the C++, the metered shipped Rust before it (twidget faa6853) and after it, alternated (spread within 12% of the median but for canvas 3000 before, 15%, and drag 3000's C++, 18%), µs a frame with Rust/C++ in brackets, and the setup (the first build and paint, which the frame leaves out) in ms.

| mode | n | C++ | metered, before | metered, after | setup: C++, before, after |
|---|---:|---:|---:|---:|---:|
| mixed | 1000 | 5977 | 5711 (0.96) | 4794 (0.80) | 26, 22 (0.85), 22 (0.85) |
| transform | 1000 | 4402 | 4633 (1.05) | 3649 (0.83) | 26, 22 (0.85), 22 (0.85) |
| swarm | 800 | 8134 | 9907 (1.22) | 6746 (0.83) | 22, 20 (0.91), 20 (0.91) |
| css | 1000 | 17411 | 16620 (0.95) | 15493 (0.89) | 160, 240 (1.50), 160 (1.00) |
| drag | 1000 | 40 | 35 (0.88) | 36 (0.89) | 29, 25 (0.86), 25 (0.86) |
| drag-doc | 1000 | 1206 | 797 (0.66) | 782 (0.65) | 29, 26 (0.90), 25 (0.86) |
| canvas | 1000 | 826 | 240 (0.29) | 242 (0.29) | 0, 2, 2 |
| mixed | 3000 | 19830 | 12827 (0.65) | 11514 (0.58) | 113, 75 (0.66), 74 (0.65) |
| transform | 3000 | 15008 | 9832 (0.66) | 8600 (0.57) | 118, 74 (0.63), 75 (0.64) |
| swarm | 2400 | 27175 | 31487 (1.16) | 22894 (0.84) | 57, 49 (0.86), 50 (0.88) |
| css | 3000 | 68390 | 65088 (0.95) | 58764 (0.86) | 485, 1045 (2.15), 463 (0.95) |
| drag | 3000 | 90 | 56 (0.63) | 57 (0.64) | 113, 74 (0.65), 74 (0.65) |
| drag-doc | 3000 | 5014 | 2475 (0.49) | 2285 (0.46) | 115, 75 (0.65), 74 (0.64) |
| canvas | 3000 | 2270 | 732 (0.32) | 726 (0.32) | 0, 2, 2 |

## Findings

What the comparisons found about the C++, kept for the stages that meet it again.

- 0.3's id-keyed `PatchLog` (198b1c3) made its comparator dereference nodes, and a `<textarea>` `value` inserted into
  `texts` without checking `valid`: a `data` op, an `rm` of the text's parent and a textarea `value` in one batch read
  the freed parent (a use-after-free on the rust-safe branch only: the product's pointer-keyed sets compare addresses
  and never dereference). Fixed on rust-safe (the insert checks `valid`, as `note_children`/`note_attribute` do), the
  Rust tree alike. Take 0.3's 198b1c3 only with that fix.
- renderer.cpp's `motions_` holds `std::string_view` names into the animator that dangle between frames (read only right
  after refilling, so never freed memory); the composition key hashes the width with `std::hash<double>`, whose value
  depends on the standard library (only equality is used).

- The recordings: `twidget_core_tests` and `twidget_core_record` die of a stack overflow in a Windows Debug build
  on the default 1 MiB main stack (they pass on 2 MiB), so ctest `twidget_core` and `twidget_parity_core` fail
  there; no corpus widget reads canvas pixels or selects under the recorder's script (only `core.twrec` does: 1 pixel
  read, 27 selections) and the corpus has 4 images; the recorder's outcomes depend on wall-clock time (a widget may
  paint within the 300 ms settle or not, `api-insertadjacenthtml` did once).

- tdesktop's Rust helper (H3): on x64 the main thread, impersonating the initial token, gets one
  `BCryptGenRandom` answer and 0xC0070005 for each later one; the Rust helper draws its std's HashMap keys there
  before the lockdown, so WarmUp's call became the second and failed ("the widget helper is not sandboxed"): the
  delay-load hook now answers `SystemFunction036` with `ProcessPrng` where the system has it and with
  `BCryptGenRandom` as before where it has not (Windows 7: untested whether the one-call limit holds there), on
  x86 and x64 alike since both helpers are built for Windows 7 (tdesktop 6ef075cd4d, which the C++ helper
  compiles too). Without LTO the Debug Rust helper imported USERENV.dll (std's `home_dir`, no KnownDLL). On Linux,
  a kernel older than 5.6 still gathering entropy after boot makes std open `/dev/urandom`, which the filter
  refuses, so the helper aborts. Found on the way, not ours: in `tdesktop:centos_env` CMake picks Python 3.12, whose
  pyexpat needs a newer expat than the image's 2.2.5, so `widget_helper_identity.py` (plistlib) fails for every
  pinned Linux build; a non-production Linux configure installs one unpinned helper that Debug and Release overwrite.

- The helper's C ABI: both engine ends enforce the 8 MiB widget memory limit on the bytes the engine's allocations
  hold, counted by nova-safe's metering allocator (`nova_meter`: the Rust helper's global allocator, and
  nova_embed_ffi's in the C++ helper), which counts the same bytes for the same work in both; the count varies from
  run to run of either end with Nova's random hash keys, so `helper_ffi_oracle` still leaves `Stats`' engine heap out
  of its comparison (H2). `auto/33-stop-batch` (H4) has not been run in the app since. Rust std draws its `HashMap`
  keys from `SystemFunction036` once per new thread, after the lockdown and outside the platform table, so H3 binds
  cryptbase.dll before an LPAC lockdown, allows `getrandom` in Linux seccomp and `getentropy` on macOS; a layout query
  sent after the glue stopped waits out its 3 s on the platform clock in Rust and on `std::chrono` in C++ (alike in
  the app, not under a stepped clock); the renderer clock's milliseconds are `duration<double, milli>` in C++ and the
  glue's seconds times 1000 in Rust, which can differ in the last bit on a real clock.

- tdesktop's helper glue (js-sandbox): a `frozen` message stops the glue's forwarding but never reaches the
  renderer, which keeps painting the frames Telegram asks for; host messages before Open are dropped (unreachable
  through the served host); a load ack is read with `asInt`, so qid -1.5 counts as the post-load query; the stop
  reason and the cursor text are cut by bytes and can end inside a UTF-8 sequence; the cursor's document shares the
  process-wide declaration cache with the renderer's while its container sanitizes nothing (the cache keys only on
  whether there is a container: latent cross-talk); `last_html()` inside the cursor builds a waiting batch outside any
  renderer-end call, so its State reaches Telegram with the next call; `sync()` calls `root()` unchecked and
  `hitAt()` converts a double to int64 unchecked (UB in principle, unreachable with the renderer's answers). The
  tap's stamps are on the OS steady clock Telegram reads, so `serve` takes the OS glue's clock.

- f0a070d's protocol: a `FrameDelta` past the message cap passes `delta_ids` and the due check first, so it is
  refused as "did not rebuild a frame" rather than "did not decode"; after a failed batch the proxy's remaining widths
  are 0 (the end has stopped); the Container measures owed widths of texts under `display:none` too. `runs_for` keyed
  its index with `std::hash`, which changes the deltas only past four colliding commands but depends on the standard
  library: fixed in 0.4 as a 0.3 fix. MSBuild in a Temp build tree misses header changes after a rebase (MSB8029:
  twidget's oracle, widget-sandbox's `build-rust`): build with `--clean-first` once.

- renderer.cpp's container: 344 of 13,559 documents were allocated at the previous one's address, and
  `selectable_key_` is (address, style epoch), so a new document whose epoch matches reads the old `selectable_`
  answer (the port gives each document its own identity and differs exactly there); `image_ready` walks an
  `unordered_map` for the first image of an id, and id 0 repeats (failed, refused, placeholder and canvas sources), so
  its answer depends on the standard library (the port walks the sources in order: a 0.3 candidate).
- `editor_style` leaves the editor's old clip in place when no ancestor clips, and writes the inner clip with
  `has_clip` false when an outer one cuts it, while `refresh_editor`'s "moved" check compares those bytes anyway.
  `marker_text`'s alphabetic, roman and leading-zero branches are dead (litehtml draws those markers as text; the
  container gets armenian, CJK, georgian, hebrew and kana markers and draws them as decimals); `draw_borders`' square
  strips and `draw_cached`'s tracing are dead too. `MAX_FACES` bounds only faces no document holds; one face can fill
  the shared 1 MiB width pool; a decoration on a 0-wide box is measured past the width cache and spacing.

- litehtml's images: an aspect's alignment is matched case-sensitively against `xmin`/`xmax`/`ymin`/`ymax` (SVG's
  `xMinYMid` centres; the renderer has no `get_image_aspect`, so it is always empty in the helper);
  `object-position: top` reads as `top top` (left) and `bottom` as right; only its first two tokens are read; lengths
  go through `strtof` (units dropped, `.5em` is 0.5px, `calc()` split at its spaces); `object-fit`'s CSS-wide keywords
  and invalid values act as `none`, not `fill`; an image without a source still gets a `draw_image`; the border box
  handed over is not rounded with the content box.
- The fork's limits: a `var()` chain deeper than 32 returns without marking the budget invalid, so the partly
  substituted value stays; `sanitizer_limit("depth")` and `("elements")` fire per refused element, `"table"` and
  `"content"` once; a refused cell's `take_table_cells(1)` answer is ignored; past 256 matched selectors an element
  stops testing the rest, `::before`/`::after` rules included, and can lose its generated content; counter increments
  are clamped to int, resets not; `clamp_length` bounds `%` and `calc()` at 2^25 px but not plain px; Gumbo's numeric
  references wrap past 2^32 (`&#4294967361;` is `A`); two rules giving one `::before` its content charge the
  generated-content budget twice while the first render tree lives.

- litehtml's hit testing: positioned boxes that a transformed or contained static box holds are almost never hit
  (only `get_element_by_point` reads positioned lists, static blocks are reached through `get_child_by_point`), so
  points over them fall through although painting draws them; a clipping box passes points to its children only in
  its content box, not the padding box where its scrolled content shows, and ignores its radius.
- A hovered or active anonymous element survives a render-tree rebuild while its anonymous parents (weak `m_parent`)
  die: `on_mouse_leave` stops at it and its ancestors keep `:hover`/`:active` (a product bug). `on_lbutton_up` never
  clears `m_active_element`, so a second release over it clicks again. `on_scroll` dereferences `m_root_render`
  unchecked (UB without a render tree), is `const` while it scrolls, and the element-level scrolls move the first
  render item, not the one checked; the renderer calls neither `on_scroll` nor `scroll_to`, so element scroll
  positions stay 0 in the helper. `el_svg::element_at` hits outlines' point boxes (control points and stroke slop)
  and has no caller in the helper; `position::is_point_inside` is fuzzy to 1e-4 px.

- litehtml's patching and the layout cache: a collapsible space the inline layout skips keeps its place from an
  earlier layout (`forget_positions` resets marked items only; 124 relayouts differ from a fresh document, none with
  the cache off); a patch that leaves no root render item makes `render()` return before resetting `m_size`, so the
  old height is reported (34); positioned lists survive a patch that changes no `position`, so a transform added or
  removed in place leaves an absolute box placed against its old holder; a legend restyled alone loses the margins
  `el_fieldset::compute_styles` gave it; an option's text changed in place does not resize its `<select>`.
- A refused `update_structure` still bumps the match and style epochs and takes a layout snapshot; `retag_allowed`
  interns every id and class it compares, refused or not; a child list that changes length without `data-lh-id`
  keys (the renderer writes none) rebuilds every child and drops their class flips and match memos; a removed
  `::before`/`::after` gives its generated-content budget back only when the old render tree dies;
  `m_layout_origins` keeps raw element pointers, guarded by the snapshot flag (no use-after-free reached).
- litehtml's SVG painting: `draw_use`'s depth over 8 counts every group level, so a `<use>` inside nine nested
  groups is never drawn; an inherited `visibility` attribute wins over a child's CSS `visible`; an empty presentation
  attribute stops inheritance and paints black, `fill="inherit"` or a word that is no colour paints nothing; the
  `opacity` attribute goes through `strtof` (`"50%"` opaque, `"abc"` invisible); `<text>` takes no gradient fill,
  only the first `x`/`y` and no `text-transform`; stops' `stop-color`/`stop-opacity` are not inherited,
  `spreadMethod`, `gradientTransform` and `xlink:href` templates are ignored.
- Paint keys mix float bits, so NaN geometry gives compiler-dependent keys; `cacheable_paint_key` matches `url(`
  case-sensitively while `svg_group_key` lowercases; `m_stable_draws` is an `int` that overflows (UB) after 2^31
  draws. Where a +NaN and a −NaN meet (a NaN-y box's rotated transform) MSVC keeps the first operand and LLVM the
  last, so lhdump `Q` writes every NaN as one. The corpus snapshots name SVG shapes `x-unknown` (Gumbo's tag
  table), so their paint runs drew no path: SVG is compared on the raw pages.

- litehtml's painting: a table draws its captions, rows and cells itself and never through `draw_group`, so
  opacity, transforms, filters, backdrop filters, clip paths and sticky on a cell, row or caption are ignored, and a
  positioned cell paints in the blocks' pass rather than in its z-order (a product bug; a C++ fix candidate).
- A box's own clip check grows it by its outer shadows' blur once while the ink counts the blur twice, so a box just
  outside the clip loses a shadow that would reach into it. Linear and radial gradients never hand the painter their
  colour space or hue interpolation (`linear-gradient(in oklab, …)` is parsed, then dropped); conic ones do.
- `border_radiuses::fix_values(w, h)` gives 0/0 for a box 0 wide with a vertical radius past half its height, and the
  NaN radii reach `draw_borders` and the layers. A fieldset with a legend skips `html_tag::draw` (no shadow, outline or
  marker; skipped background images drawn). Inset shadows ignore `bare_shadows`; a collapsed cell's given-away sides
  reach the container with style none but their width kept.

- litehtml's grid: `repeat(auto-fill|auto-fit, …)` without a definite size counts repeats by the gap alone
  (`repeat(auto-fill, minmax(min-content, 1fr))` with `gap: 10px` is 38 columns in 375 px, one in CSS);
  `justify-content: flex-start`, the fork's initial value, stretches `auto` columns as `normal` does;
  `grid-template-rows` takes plain tracks only (a repeat there is dropped, auto-fit collapses columns only); a % row
  without a definite height is a fixed 0 px row, and a definite height of 0 counts as none; a spanning item taller
  than all-fixed rows grows the last of them past its length.
- The widths a grid reports (shrink-to-fit, inline-grid, floats, absolute grids) count only items in one column, so a
  grid of spanning items reports its gaps and fixed tracks, and `fit-content(x)` reports x. Negative lines past the
  explicit grid clamp to line 1. No baseline alignment (`baseline` is start, unstretched; a grid's baselines are its
  border box's bottom); `safe` is ignored. `grid_track::infinite` is never read. The fork has no named lines or
  areas, `grid-auto-flow`, `grid-auto-columns`, `grid-area` or subgrid: the parser drops them.
- Each grid lays an item out two or three times at different containing blocks, which the layout cache cannot
  answer: nested grids cost 2–3 to the power of their depth, bounded only by the renderer's layout budget (the
  `render-nested-grid` attack crosses it, 124 levels after 9.7k passes).

- litehtml's tables: the captions' pass moves every row down by the top captions' height with `+=`, and rows are never
  laid out, so a captioned table's rows (and a `<tr>`'s placement, getBoundingClientRect) drift down a caption's height
  on every layout that is not a cache hit. A product bug; a C++ fix candidate (set the rows' y, or reset it first).
- The grid takes the rows' CSS heights, the columns' CSS widths and the cells' border widths when it is built: a
  `:hover` restyle that changes them reaches the table only when a media change builds the tree again. Cells are laid
  out as a second pass, so a cell's min-/max-width or keyword width never lays its content out again (the table then
  gives it its column's width). A collapsed table overlaps neighbouring borders by the thinner one while painting draws
  the winner, so a wider collapsed border reaches into the next cell.
- A top caption exactly as high as the table's top border moves nothing, and `top_captions_height` is set only when the
  captions move the cells, so it keeps an earlier layout's value; `bottom_captions_height` is never set. A fixed table
  reads its first row only; the table's minimum width and `calc_rows_height`'s spacing are computed and never read; the
  three-step `distribute_width` and `table_grid::clear()` are never called. No undefined behaviour met in the table code.

- litehtml's positioned layout: `m_fixed_boxes` is cleared only when `render_positioned` runs, which needs an
  absolute or fixed box, so after a media change or a restyle leaves none `get_fixed_boxes` keeps answering the last
  placement's boxes, and the renderer reads them (a product bug). `get_redraw_box` grows a box's right edge by its left
  padding and border, so a fixed box's redraw box and a paint group's bounds are off by the difference. With `left`
  and `right` set and `width: auto`, a % max-width is resolved against the containing block's height.
- The render types apply to the first level only (a positioned box's own list is placed with `render_all`; the
  renderer uses neither type). Positioned lists are sorted only when an absolute or fixed box exists, which painting
  and hit testing never see. A replaced box between `top` and `bottom` gives its auto margins the leftover height only
  when it is positive, the leftover width whenever it is not zero. An absolute box inside a relatively positioned
  inline is taken out by `split_inlines`, so its containing block is the next positioned block or the viewport.
  `element_static_offset` reads the document's root render item unchecked for a fixed box.

- litehtml's flex: a row item's cross `auto` margins place it from the container's content top, not its line's start (a
  wrapped line's item lands on the first line); a column item's cross `auto` margins never end its alignment, so
  `margin: 0 auto` does not centre in a column; a column's `flex-basis: content` is 0; a line's cross size is capped by
  the larger of the container's height and max-height, padding and borders included for border-box.
- `justify-content`'s remainder was written for integer pixels: a float residue past 1e-4 shifts an item and all after it
  by a whole pixel. A gap is its length's raw number (a `calc()` gap loses its %, a % row-gap in a row is 0). An empty
  flex with `space-around` divides by zero lines (UB, result unused); a column's min-height check is always true.

- `render_item_image::_render` writes the image's outer height into its element's computed `line-height`, before the CSS
  width, height and max apply (so the intrinsic height). `line_box::new_width` starts at the second item, so the first is
  neither moved nor counted, and on an empty line it steps past `end()` (UB). When a line moves, start and continuation
  markers move their element's boxes but not themselves; end markers do not move.
- The second breaker `fit` (room minus trailing punctuation) is unchecked: a breaker answering at or before `start` would
  take the rest and step backwards. `<textarea rows>` past INT_MAX gives 2 rows with a 64-bit `long`, INT_MAX on Windows.
  Items `fetch_positioned` never reaches are never cached (text pieces, a media change's leftover tabular items); the
  cache's hit counters are process-wide and `LH_NO_FIXED_LAYOUT_REUSE` is read with `getenv` on every flex box's lookup.

- The renderer decides the name-limit stop in `receive()` and `tick()` after flushing the messages, so its violation and
  the badge come one message or frame late; the layout budget counts every word and space of text as an element.
- The renderer's `matchMedia` is a substring match: `not`, `and`, commas and units are ignored (`(min-width: 20em)` is
  read as 20px, `not (hover: none)` matches).
- A NaN or huge document height goes through `int(ceil(...))` out of range (UB; x86 gives INT_MIN, as the port), then
  `px_height()` turns the negative value into a `uint32_t`; the `anims` query's 100000-id cap cannot be reached under
  the 50000-node limit.

- `media_changed` rebuilds the render tree without `fix_tables_layout` and without clearing `m_tabular_elements`: after a
  resize across a breakpoint a loose `display: table-cell` loses its anonymous table boxes, the previous tree's tabular
  items stay alive (about 8 more per resize on a two-table page), and a table's elements answer `get_render_item` and
  `get_placement` with the old tree's items, never laid out again: table content reports stale boxes after a resize.
  A product bug; a C++ fix candidate.
- Some render items are `init()`ed twice (a flex child that is neither text nor a block box; the cells of a row group
  inside a row, whose nested tables charge the cell budget twice); a block initialised twice drops its first formatting
  context, so `get_render_item` answers nothing for it. `split_inlines` recurses into any child with children, pulling a
  block out of an inline-block or an inline table's cell and cloning the boxes around it.
- A caption among a table's loose children leaves the later children listed by both the table and the anonymous row
  group; whitespace between blocks gets no render item, a flex drops leading collapsible and all trailing spaces;
  `list_index` reads `start` from any parent, not only `<ol>`.

- mousemove and touchmove carry no `pointerId`, so moves of different pointers in one frame share one coalescing key
  (`mousemove:0`) and their movement adds up; every mouse pointer is pointer id 1, so one capture call takes all of
  them. `pointer + 2` overflows `int` for a host pointer near INT_MAX (UB; the port wraps); a run past 4096 bytes under
  a NaN pointer rounds NaN with `lround` (unspecified; the port gives 0).
- `refresh_editor` restores the colour before comparing it, so a colour change never renews the editor's generation; a
  restyle that freezes the renderer inside `hover()` leaves `hover_` set after `freeze()` cleared it; a second down on a
  live pointer replaces its touch with no pointercancel or lostpointercapture.

- `refresh_styles` never re-adds the declarations HTML attributes make: after a media change or a `:hover` restyle,
  `<img width>`, `<td bgcolor>`, `<font color>` and the SVG font attributes are lost. A product bug; a C++ fix candidate.
- Text in a relatively positioned inline copies its offsets but is always `position: static`; an anchor with `href`
  gains `:link` once per sheet applied, so it never shares a style; list items numbered by the render tree's
  `list_index` stop sharing styles in later restyles; the table pass writes column widths into cells' computed widths
  and its anonymous boxes run the cascade too.
- `font-size: 0` creates no font and keeps the previous metrics and `normal` line height; `text-decoration-style` is not
  reset when the line is `none`; `revert` lookups count against the selector budget during a hover restyle; the
  process-wide caches of parsed master and document sheets are keyed on the text, not the container.
- For H3: the cascade recurses per level; 128 levels must fit the helper's thread stack, so its per-level frames stay
  small (a 120-deep document runs in 512 KiB optimized, 1 MiB unoptimized).

- SVG percentages in `svg_geometry` (getBBox, getTotalLength, SVG client rects) come from the nearest `<svg>`'s viewBox,
  else the first number of its `width`/`height` attributes with units and `%` dropped, while painting uses the laid-out
  view: in `<svg width="50%">` a `cx="50%"` is measured at 25 and painted at about 90; in an `<svg>` sized by CSS alone a
  `width="100%"` rect has no outline; a shape outside any `<svg>` is measured against 0 × 0.
- An `<svg>` laid out 0 wide or high without a viewBox makes `view_group` divide 0 by 0: every CTM inside is NaN. SVG
  references go through a CSS `#id` selector, so an id that is no CSS identifier (`#1a`) never resolves.
- `parse_svg_transform` keeps lists browsers drop (unknown functions and wrong argument counts as the identity, names in
  any case, `rotate(a, x)` about the origin); `numbers()` skips units and `%` in viewBox and points; fill-box and a
  gradient's box use the control points' box, and a NaN first point poisons it while later NaNs are skipped.

- `<style media="print">` applies to the screen: `el_style` is not an `html_tag`, keeps no attributes, so its sheet's
  media is always empty. A `<style>` inside a `<table>` never applies: `el_table::appendChild` keeps only
  tbody/thead/tfoot/caption, and the rest Gumbo puts there (`<colgroup>`, `<col>`, scripts, text) is built without a
  parent. `<template>` is never built.
- `build_template` ties: a formatting element Gumbo clones shares its attributes' source, so two holes start at one
  offset; with a non-empty value the template is refused (`<p><b class=x><div>` never gets the fast patch path), with
  empty values both holes stay in `std::sort`'s library order (which clone a patch reaches depends on the STL). A 0.3
  fix: break ties by node order, or refuse a template whose spans tie.
- The render tree writes `list_index` into ordered list items' attributes, visible to selectors and `get_attr`;
  `<style>` text is split into word and space elements and joined back by `get_text`; `width=4294967297` and
  `<font size=99999999999>` differ between Windows and elsewhere (`strtol`/`atoi` past `int`); on Apple a date field's
  order comes from the user's locale unless `TW_LITEHTML_NO_CORE_FOUNDATION` is set.

- litehtml's declaration cache keys on whether there is a container, not which one: one container's
  `sanitize_declaration` verdict (and the colour names it resolves) is replayed for another. Latent in the helper (its
  containers sanitize alike; the master CSS takes the token path). `style::add` interns every property name it meets.
- `color-mix()` refuses opaque black (it compares equal to `currentcolor`: colour equality ignores the flag); a keyword
  read into a length that held a value keeps that value's unit (`flex: content` gives a keyword basis in px);
  `font: 12px` is taken with an empty family; `text-decoration: auto` sets the colour, never the thickness; a malformed
  `rotate3d()` is skipped instead of refusing the transform; past `-lh-anim` a style text drops every raw-named
  declaration, a sheet's block only the twins.
- Folded math is computed in doubles and narrowed to float unchecked: past float's range (`calc(1e30px * 1e30)`) that is
  undefined; x86 gives infinity, as the port does.

- The platform end parses a report with a JSON depth of 4 and refuses for good one nested deeper, while the renderer
  end sends any report of a platform type whatever its depth.
- `filter_pixels` reads and writes past a buffer shorter than its rows (the port leaves such a buffer alone).
  png.cpp's inflater is more lenient than zlib (no stored-block NLEN check, over-subscribed or incomplete Huffman codes
  decoded, Adler-32 ignored), so a browser may keep fewer rows than it counts.
- encode.cpp writes a canvas command's non-UTF-8 text into the JSON raw, which a strict parser refuses; a custom
  property's computed text loses the spaces inside functions (Chromium keeps what was written).
- The three `render-nested-*` attacks lay out in exponential time in the bare fork, which has no layout budget of its own.

- Gumbo compares the doctype quirks prefixes as whole identifiers (`-//IETF//DTD HTML//EN` is not quirks; one prefix has
  a typo, `::)extensions`). `<image>` never gets `FROM_IMAGE`, and the `xmlns` checks look at the token after the
  element took its attributes, so they never fire.
- Gumbo flushes a body's last text at the end of input, so its span runs over `</body></html>` and litehtml's
  `build_template` refuses the document's holes (10 of 412 serialised corpus pages): a C++ fix candidate.
- Every Gumbo parse error copies the whole open-element stack, which litehtml never reads; with `max_errors` reached a
  duplicate attribute's name runs into the next (`<a b b c=1>` gives `bc`). A NUL inside `<svg>`/`<math>` becomes a
  U+FFFD whitespace node, which litehtml splits into one `el_space` per byte. `</name >` in SVG keeps the space in the
  end tag's name and can close an unrelated custom element (the renderer never writes it).
- Gumbo reads one byte past the input for a named reference ending both an attribute and the text (defined only because
  litehtml passes NUL-terminated text; the port reads 0); `tab_stop` 0 divides by zero; the clone limit counts
  `sizeof(GumboNode)`, which depends on the pointer size.

- litehtml's specificity is not Selectors 4's: `:is()`, `:not()` and `:nth-child(… of S)` count as one class whatever
  they hold, and a pseudo-element counts as a class.
- `:lang()` falls back to the document's language, which the renderer never sets (`lang_changed` is never called):
  `:lang(en)` needs a `lang` attribute, and an empty `:lang()` matches every element without one.
- `An+B` reads its numbers with `strtol` into an `int`: past `int` a Windows build saturates (32-bit `long`), others
  wrap. `[attr~=word]` splits with `split_string`, which keeps a quoted run as one word. `||` is not implemented.
- The renderer's tree-byte limit checks its per-op bound (a quarter of the limit) only once the running count reaches
  the limit, so a first op of 16 MiB + 1 is taken. Adopted roots keep attribute names an `attr` op refuses, which the
  writer then leaves out. The patch log was keyed by node pointer; since 0.3 both key it by node id.

- WidgetHost matches a renderer's `result` to the pending query and resolves it in two locked steps: a query that
  times out between them and the next query that starts meanwhile gets the old answer.
- A hosted widget's death can reach the stream before or after its host's `Close` (a `Died` frame more or less in a
  run), on either end: a race in the C++ that the port keeps.

- litehtml's resolution media features are inverted: dpi, dpcm, dppx and x are refused and any other unit is read as
  dpi, so `@media (min-resolution: 2dppx)` never applies.
- `!important` counts only with `!` right before `important`; with whitespace between them both stay in the value.
- Media query lengths are fixed when the sheet is parsed: vw and vh use the document's media at that moment (all
  zeros for the master CSS), `ch` is 0, and none follow a resize. `@import url( "x" )` with spaces is not a url.
- Every at-rule name and media-feature keyword a widget writes is interned for the life of the process, as units are.

- litehtml's CSS tokenizer decides with decoded code points but reads a number's bytes raw: a token that starts with
  an overlong digit (`C0 B0`, or `.` then `E0 80 B0`) is a number that consumes nothing, and `tokenize()` makes empty
  tokens until memory runs out (`lhdump` never returns on `.à°`). `parse_stylesheet` and Gumbo decode UTF-8
  first, `normalize(std::string)` does not. The port loops alike; the comparison keeps such bytes out. A 0.3 fix.
- `hsl()` with a NaN or infinite hue reaches `std::min({…})` over NaNs, undefined: MSVC's STL skips the NaNs (black),
  libc++ and libstdc++ do not (white). The port does each platform's; a written-out min in web_color.cpp makes them agree.
- Out-of-range float→int casts give INT_MIN on x86 and saturate on arm64, float→byte keeps the int's low byte, MSVC
  prints a NaN as `-nan(ind)`: `twidget_litehtml::cxx` reproduces each platform's.
- `parse_angle` interns every unit it reads, so `hsl(1foo …)` grows the process-wide name table for good.
- `remove_whitespace` decides against the tokens kept so far up to 50 tokens and against the originals past 50, so a
  media query reads `< /**/ /**/ =` as `< =` in a short block and `<=` in a long one.
- A NUL, or a sequence that decodes to 0 (`C0 80`), ends tokenizing, but a comment's search for `*/` goes past NULs;
  `url(` followed by a quote keeps all but the last whitespace in the function token's text.

- `Animator::declares_motion` is always false: `update()` keeps only the watched properties that are not animation
  or transition ones in `bases_`, the only ones it looks at, so renderer.cpp's scripted-layer filter never excludes
  an element for what it declares.
- `pair_tokens` keyed its per-thread cache `a + '' + b`, so two pairs that joined alike shared an entry and one
  widget could change another's interpolations; fixed in 0.3 (length-prefixed key, C++ and Rust).
- The animator's unordered maps made the order of events across elements, and of `heavy_motion`'s list, an accident
  of hashing; since 0.3 both order by element id.

- sandbox_engine.cpp's `wake()` puts the deferred tasks after the one that failed in front of what that failed
  `dispatch` deferred again, which is that same task: a deferred frame or timer then waits in the queue twice.
  The port keeps it; a fix belongs in the C++ first (0.3).
- `CpuBudget::slot` converts a bucket to `int` before indexing, undefined for a time that is not finite (a
  `record` at NaN or infinity); the port takes slot 0 there, the comparison keeps such times out.
- NaN payloads and signs differ between compilers where two NaNs meet or LLVM folds `-w / 2` into `w / -2`:
  MSVC, clang and Rust debug and release give different bits. The canvas comparison treats computed NaNs as one;
  reproducible NaN bits on the wire would be a 0.3 fix in the C++.
- `paint_digest` hashes filter lists, path ops, points and dashes without their lengths, so no filters and an
  empty filter list digest alike; `bake_track` keeps one key more than the two ends of a final run of equal
  keys. Both harmless, both kept.

- `std::to_chars(double)` (json.h's numbers off Apple) is not "shortest digits": of the fewest characters
  that read back, it writes the ones nearest the value, a tie to the even digit. A large integer in fixed
  form gets its exact digits (`…016`, not `…020`), and `X.25` between 2^50 and 2^51 is `X.2`. Rust's `{:e}`
  breaks such ties upward; `twidget_json::to_chars_shortest` rounds again with `{:.N$e}`, which is exact
  and ties to even.
- json.h writes numbers differently on Apple, whose deployment targets have no floating `to_chars`: the
  shortest of 15, 16 and 17 `%g` digits, so `0.0001` is `0.0001` there and `1e-04` elsewhere. The helper's
  JSON already differs between macOS and the other platforms; the port keeps both forms, chosen as the
  C++ chooses them.
- core `format_number` prints into a 64-byte buffer: a number past 63 characters in `%.4f` (a huge
  percentage, a hex float) is cut there, and the port cuts it the same way.
- The Windows C runtime's `strtod("nan")` sets every payload bit (`0x7fffffffffffffff`), not the default
  quiet NaN; `twidget_cstd` gives each platform's. No ported caller lets a NaN through so far.
- The CSS helpers read numbers with `strtod`, so `0x10`, `1e3` and `5` are numbers to them: the
  validator, not this code, keeps such text out of widgets.
