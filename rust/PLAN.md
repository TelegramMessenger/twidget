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
| 0.2 | started: `oracle/` with the wire, remote, css, paint, compositor, base64, canvas, animations, filters, png, encode, ends, computed_style and `shadow_oracle` (the whole core) programs and `lhdump` (L0–L4, Gumbo) here and widget-sandbox's json, stream, cpu budget, SDK, gesture and stamp ones (Windows x64, MSVC); core and litehtml (`lhdump`) still to come |
| 0.3 | done but the recording: the animator's events, transitions, overrides and heavy motions and the renderer's canvas and font releases come in id or creation order; `pair_tokens`' key is length-prefixed (no cross-pair or cross-widget collision); `paint_signature` names selectors by sheet and order (`css_selector::place()`) and `PatchLog` keys by node id; `-ffp-contract=off` on every non-MSVC target (MSVC's `/fp:precise` does not contract); numbers parsed and printed in the C locale (`litehtml/c_locale.h`) at every site that reaches an output, since Telegram sets `LC_ALL` itself; sort ties checked unique. Each its own commit with a test (the same widget under de-DE and C gives the same HTML, display list, answers and canvas calls); the Rust ports and oracles follow. Left: `tests/recordings/corpus.twrec` must be recorded again on macOS (`tools/record` builds only there; on Windows it stops at step 130 on the canvas-font release order, as it did before). tdesktop needs `-ffp-contract=off` in `lib_widget_renderer.cmake` and `lib_litehtml.cmake`'s non-MSVC options too |
| 1.1 | done: `twidget_wire`; `wire_tests.cpp` ported (18 tests: documented bytes, round trips, cuts, count sites, references, enums, masks, flags, forms, defaults, the C++ suite's seeded mutations), and ~165k cases through the C++ and the Rust codec with identical answers (samples and their cuts, mutated samples, generated values, encoders under single caps) |
| 1.2 | done: `twidget_json` (in widget-sandbox's `packages/rust` since stage 1.4); widget-sandbox's JSON tests ported, and ~400k cases through json.h and the port with identical answers (its fuzz texts both surrogate modes, depths around the bound, every escape and surrogate order, `quote`, 160k doubles through both number forms and `asInt`, 30k number texts through `strtod`, 37 real JSON files and their mutations); `twidget_testkit` holds what the tests share |
| 1.3 | done: `twidget_remote` at version 1 as of twidget 5bc9639 (the freeze of 0.4 decides what stage 5 meets); `remote_tests.cpp`'s message checks ported (documented bytes, kinds, pixels, nested caps, value lengths, encoder room, cuts, seeded changes), and ~270k cases through remote.cpp and the port with identical answers (samples and their cuts under default and eight tight caps, their mutations, 20k generated messages and their mutations, encoders under tight caps, `kind_of` of every header, 24k pixel answers against the requests they answer) |
| 2.1 | done: `twidget_css` (core `css.cpp`: colours, `light-dark()`, `var()`, `:where()`, declarations, names, opaque runs, times, `calc()`, transforms, origin, opacity, fonts, theme and safe-area variables) on `twidget_cstd` (C's `strtod` with its whole grammar, hex floats rounded to even, `atof`, `atoi`, `%.*f`); ~645k answers alike with css.cpp and MSVC's C library (140k numbers, 252k generated values through every helper and their changes, 12k themes and safe areas, 241k from the 2088 style texts and 12947 declarations of widget-sandbox's corpus) |
| 2.2 | done: `twidget_paint` (core `paint_policy`: the heavy-box rules and `PaintCounter`, the limits and their messages); the shared `paint-policy.json` passes (220 checks, as `paint_policy_tests.cpp` reads it), and 165k answers alike with paint_policy.cpp (generated shadow lists and their changes, styles, keyframe sequences, scripts of counter steps) |
| 1.4 | started: widget-sandbox's stream frames (`twidget_stream`, ~80k cases alike with remote_wire.h) and coding (`twidget_server::coding`, `remote_coding.cpp`, met by the C++ host end on every stream of T6); tgw's hosted wire waits for S6 |
| 2.3 | done: `twidget_compositor` (layers and tracks, `resume_track`, `paint_digest`, `summarize`, `drop_inert_clips`, `damage_between`, `bake_track`) and `twidget_base64`; core_tests' compositor and damage checks ported (9 tests), ~72k cases alike with compositor.cpp and encode.cpp (12k lists and their edits, 16k damage cases, 6k bakes with their sample times, 12k resumes, 2.4k base64 inputs) |
| 3.1 | done: `twidget_canvas`, the whole interpreter behind a `CanvasHost` trait with every limit, and the answers of `measureText` and `imageData`; 17 tests, ~19k scripts alike with canvas.cpp (2.8M generated commands and 16k queries, 26 command streams from `corpus.twrec` and ~900 changed or spliced, the stress widget's frames, corpus charts, 62 scripts that each cross a limit): every batch in wire form, every answer, the state after each step and every host call in order |
| 3.2 | done: `twidget_animations` (timing functions, keyframes, specs, interpolation, Web Animations records, the `Animator` with plans, signatures, the `anims` query and heavy motions; the C++'s per-thread caches as a shared `Caches`, the per-element maps by element id as 0.3 asks); core_tests' animation checks ported (16 tests, 4 more of the port's own), ~138k cases alike with animations.cpp (24k scripts, 20k timings, 40k interpolations, 8.6k pair-cache cases, 16k keyframe rules and specs, 20k Web Animations records, the corpus's 101 animated sheets with their 4342 declarations and value pairs); 467 of 474 branch points reached, the other 7 unreachable in the C++ too |
| 4.1 | done: `twidget_shadow`: the shadow tree, every op with its checks and limits (with the reordering of a batch whose canvases draw one another), the canvases the ops make, the patch log, the theme on the root, the frozen chrome and the writer behind `last_html()`; the renderer's other effects through a `DocumentHost` trait in the C++'s order. The tree cannot be built alone in the C++, so `shadow_oracle` is the whole renderer driven by its messages; ~19.3k scripts and ~123k batches alike (violations, canvas calls in wire form, `last_html()` byte for byte): corpus.twrec step by step and changed, 14k generated and changed scripts, every limit on its boundary, the 654 corpus widgets as validator-like ops; core_tests' tree checks ported (16 tests, 4 of the port's own); 214 of 221 branch points reached |
| 4.x helpers | done: `twidget_filters` (the painters' CSS filters: colour matrices, reach, `filter_pixels`; 42.5k matrices, 20k reaches, 8.6k filtered images alike, floats as bits), `twidget_png` (png.cpp has no encoder: the intact rows of a damaged PNG and the header sizes of PNG/GIF/JPEG/WebP; 136.5k inputs alike, 28.6k from the corpus's 198 images cut and mutated), `twidget_encode` (the display list, canvas and composition JSON for the Android and web painters; 28k alike byte for byte), `twidget_computed_style` (computed_style.cpp over an `Element` of what it reads of a fork element until L5; 49.6k helper inputs and 1.17M answers on 651 corpus pages and 1000 generated documents laid out by the fork, alike) |
| 4.3 | done: `twidget_renderer`: renderer.cpp's `Renderer` and `Container`'s document life: every host message, coalesced and partial batches, the build (the patch log into the fork's holes, the serialised HTML through `update_from_string`, a new document), paint-only patches, the viewport following the document in up to four passes, viewport units, fonts grown past 4096, the layout budget (100000 + 50 an element) and the name table's bounds that stop a widget with the badge alone, `frozen`, theme, safe area and scheme with the master CSS, animation overrides through the style holes, size reports and resize diffs, canvas fonts, the meters, every query kind and `result`; the tree and the input driven as the C++ drives them, litehtml behind an `Engine` trait and stage 4.5 behind `Layers`. `renderer_oracle` (probes shared with events_oracle via `probe.cmake`/`input_probes.cmake`): 2569 scripts, 92k steps alike, 877k answers replayed (corpus.twrec and 160 changed, 412 corpus widgets, 600 patch scripts, 600 documents with input, 600 theme/animation/resize/frozen, 120 scripted layers, 12 flushes, 60 past the budget or names); 12.6k queries of 13 kinds plus unknown kinds; 8.9k new documents, 19.3k HTML patches, 5.3k in place, 3.5k style-hole frames, 10.5k paint-only; 29 layout, 4 name, 8 paint stops; core_tests' checks ported by name (17, 3 of the port's own). Left: the render-tree helpers the queries read and the container's image table, behind `Engine`; clock-read order unchecked (step clock) |
| 4.4 | done: `twidget_events`: pointers and their pointer/mouse/touch events, coalesced moves and movementX/Y, hover chains, listeners and prevented presses, trusted presses, the gesture claim and widget-sandbox's arbiter (holds, double taps, contextmenu, the bubble's verdicts), pointer capture, the `<details>` toggle, `<img>` load and error, ResizeObserver reports, focus and the platform's editor, text selection with its run cache, carets, long runs, `selectable_text_json` and the tint in documents and layers; the tree is 4.1's `Document`, and what layout, the cascade and the rest of the renderer answer goes through an `EventHost` trait in the C++'s order. `events_oracle`: renderer.cpp copied with probes at the input code's calls (every pattern counted at configure), a tape of the C++ document's answers the port replays question by question; 3975 scripts and 436k steps alike (1.65M answers): corpus.twrec's sessions and 200 with gestures, the gesture, interactive and showcase widgets ×12, the rest of the corpus, 1500 generated documents with listeners, 400 selection, 300 capture, 300 editor scripts, 160 crossing the layout budget from input; core_tests' event, gesture, focus, editor and selection checks ported (17 tests, 2 of the port's own); 247 of 254 blocks reached, 4 unreachable in the C++ too |
| L0 | done: `twidget_litehtml` begun, one module per C++ file: `Px` with its fuzzy comparisons, the name table (`Names`, 383 names at start), `t_strtod`, html.cpp with `t_strtof`, UTF-8 and code points, `num_cvt`, every keyword table and enum, `css_length` with calc, `web_color` (hex, rgb, hsl, names, `darken`) behind a `ResolveColor` trait, `parse_angle`, and `cxx` for what each platform's C++ leaves undefined; `lhdump` (0.2 for litehtml: the fork compiled as twidget builds it, `LH_NO_*` cleared and set per case) with ~470k answers alike (120k number texts, 40k `Px` triples, 60k colour tokens, 20k hsl, 40k clamps, darkens and hex forms, 60k lengths, 22k angles, 30k list markers, 70k UTF-8 and string helpers, the name table) |
| L1 | done: the CSS tokenizer and the parser's normalizing half (`componentize`, `normalize`, `remove_whitespace` both ways, the nesting limit of 128, comma lists and value checks); ~490k answers alike: 147k from the corpus's 1376 style texts and 25507 declarations, 11k from master_css, UA_EXTRA and the profile's UA layer, 180k from 60k generated texts reaching every token kind, 152k colour and length texts |
| L2 | done: the rest of css_parser.cpp (stylesheets from bytes or tokens, rules, at-rules, declarations with `!important`, nested style blocks), `stylesheet` (`@charset`, `@import` with its media, nested `@media`, `@supports`, `@layer` as the fork marks it), all of media_query.cpp with three-valued answers, `encodings` (UTF-8, UTF-16, BOM sniff), unit conversion, `parse_url`, a `DocumentContainer` trait, and the renderer's `rewrite_color_scheme` (how `prefers-color-scheme` reaches litehtml); selector counts and `@supports` property checks wait for L4/L5 behind a `StyleSheetHost` trait. ~165k answers alike, every walk and media answer over a grid of 32 media feature sets: the corpus's 390 sheets and 1376 style attributes, master_css and the UA layers rule by rule, 12k generated sheets, 30k media query lists, 60k declaration blocks, urls, decodes and conversions, 22.7k colour-scheme rewrites |
| L3 | done: Gumbo ported (`twidget_litehtml::gumbo`, one module per C file: the UTF-8 reader, tags, char refs with char_ref.c's Ragel machine and all 2242 actions, errors, the tokenizer's 68 states, tree construction with every insertion mode and its limits; tables generated from the C by `tools/gen_gumbo_tables.py` and checked against it); lhdump `j`: ~265k parses alike field by field, errors included. The DOM: `Document` as `createUpdatableFromString` builds it, an arena of `Element`s by `ElementId` with one `ElementKind` per class (29): `create_node` by tag, words and spaces by `split_text`, comments, CDATA, templates not built, the depth (128) and element (20000) limits, `sanitize_element`, the node map and `build_template`'s literals and holes; each class's `appendChild` and `parse_attributes` (presentational attributes as declarations kept for L5, the form controls' generated boxes and state, `<style>`/`<link>` sheets, the container calls), html_microsyntaxes.cpp, `strtof` and `%g` in `twidget_cstd`; `SelectElements` on the document. lhdump `i`/`I`: 34452 documents and 21018 replays (32328 declarations) alike (corpus pages, snapshots, quirks mode and changes, generated documents of every class and attribute, every control type, number texts, tag soup and serialised trees, the limits); L4's matrices run on the Rust DOM (3455 of 3472 documents; 17 need generated content). Left: `createFromString`'s encoding sniffing, fragments, `::before`/`::after` content (L5/L6) |
| L4 | done: all of css_selector.cpp (every selector form the fork takes, `:nth-*` with `of S`, `:is()`/`:where()`/`:not()`/`:lang()`, pseudo-elements with the renderer's parts, the 128-compound limit, forgiving and strict lists, quirks lowercasing, specificity, `sort_selectors`) and html_tag.cpp's matching over a `SelectElements` trait (the combinators' searches with the descendant pruning, the `:nth-*`/`:only-*` counts, `select_all`/`select_one`; the budget ported, compared with L5); the stylesheet walk parses its own selectors now. Match matrices (every selector × every node, pseudo-classes checked and not, `:hover`/`:active` flipped up the chain) alike with the fork on the fork's own DOM (lhdump `h`): 412 corpus documents with their sheets and the UA layers on 52, 3000 generated documents (a quarter in quirks mode) with generated sheets, master CSS and UA layers against 60 more |
| L5 | done: style blocks (`style.cpp`: `style::add` with the declaration cache as an object the process owns, every property's parser into the fork's typed values, shorthands, CSS-wide keywords, `!important`, the `-lh-raw-` twins, `var()` and folded math), gradients and images, `css_effects.cpp`'s value parsers; and the cascade: `finalize_from_external_root` (master and user sheets as trusted, `:root`, attributes, document sheets under their media, `media_changed`), `apply_stylesheet` with the selector budget (the sibling index's charges, the `LH_NO_*` switches as `Switches`), `::before`/`::after` with their content (quotes, `attr()`, counters, images, the generated-content limits), counters, `refresh_styles`, `find_styles_changes`, `compute_styles` with style sharing, `get_property` with `inherit`/`initial`/`unset`/`revert`, `var()` on elements, every field of `css_properties::compute` (fonts through the container), the form controls' look, SVG paints; `computed_style::Element::of_cascade`. lhdump `l`/`f`/`y`: 107.5k declaration answers and 2940 cascaded documents alike field by field (the corpus's 412 pages light and dark at two viewports with media changes and `:hover`/`:active` flips, 255.9k elements; every tag under the master sheet and UA layers; 1500 generated documents and sheets; the budget crossed; 604 documents under the four switches), the container's calls alike but for the render tree's; 146.4k elements' computed-style records equal computed_style_oracle's, with 1.17M `cs` answers alike. What the render tree writes into the cascade (`list_index`, column widths, anonymous boxes) is L6's |
| L6 | done: the render tree (`render_item.rs`, `render_block.rs`, `render_flex.rs`, `render_table.rs`, `table.rs`, document.cpp's tree building): an arena of render items by slot-and-use id with the C++'s ownership counted (children, the root, the tabular list and a table's grid own; parents and `m_renders` are weak), every class, `create_render_item` of each element class, each `init()` (`list_index`, `split_inlines`, anonymous blocks and flex items, the grid with spans, `limit_columns` and the 200000-cell budget, column widths written into cells), `fix_tables_layout`, `html_tag`'s anonymous constructor running the cascade (slots reused); built by `finalize_from_external_root` and by `media_changed` as the C++ does. lhdump `o` with `r`: 3414 documents (12.6k dumps, 1.07M items) alike item by item with their calls (the corpus light and dark at two viewports with media changes, every tag, 41 written shapes, 2500 generated documents); `l` and computed_style_oracle without their accommodations, alike. Layout starts at L7 on these items |
| L14 | started: the geometry half. `el_svg.rs` over an `SvgElements` trait (the DOM's tree and attributes; the cascade's own `transform`/`transform-origin`, computed transform and `transform-box`; layout's placement: L5–L9 implement it on the document): numbers and flags, path data with SVG 2's error handling (the fork has no cap on commands), arcs as cubics, the shapes' outlines, `transform` attributes, CSS transforms with fill-box, `svg_ctm`, `view_box`, `get_content_size`, `view_group`, a `<use>`'s placement with its caps (depth 8, ids of 1–128 bytes, 10000 a pass, 16 a lookup inside a `<use>`), `svg_parent` through `<use>` hosts, gradient coordinates over a path's box, `svg_path_length` and the point at a length; `TransformMatrix`, `PaintGroup`, `VectorPath`, `SvgGradient`, `Position`, `Size`. `svg_oracle` (el_svg.cpp included whole): 134k record answers and 3520 documents alike (the corpus's 85 SVG pages and its 207 `<svg>`s, 828 changes, 2400 generated: 98.9k elements, 10.3k `<use>` placements, 96.4k parent chains, 24k gradients), NaNs as one. Left: the painter's walk, `svg_property` and the paints, text runs, `el_svg::draw` and its caches; hit testing with L16 |
| 5.1 | done: `twidget_ends`: `RendererEnd` over a `Renderer` trait (renderer.h's calls) with its `Output`, the backend `Proxy`, `Backend`/`Clock`/`Channel` traits, and `PlatformEnd` with a `Host` trait for the tests; ends_tests.cpp's 11 tests ported by name against a scripted renderer; `ends_oracle` (renderer_end.cpp beside a scripted C++ renderer, since the C++ end constructs the renderer itself) alike on 48 written and 4000 generated sessions (97.8k steps: wire traffic, renderer calls, Backend calls, host reports, every stop reason and the platform end's refusals). The real renderer's own answers (pixels, the throttle, clocks) come with stage 4 |
| S1 | done: `twidget_cpu_budget`, `twidget_sdk_policy`, `twidget_gesture`, `twidget_bundle_stamp` (with version.h and `SandboxResources`); the three shared fixtures and the C++ tests ported by name (13 tests), ~1.14M inputs alike with the C++ (246k budget steps, 590k SDK checks and gate steps, 203k gesture claims and steps, 100k stamps and 300 bundle directories) |
| S2 | done: `twidget_js::JsThread` and `Poster` (8 MiB stack, tasks, timers in deadline order, the sweep); a panic on the thread aborts, as an exception leaving a JSThread task terminates |
| S3 | done: `twidget_js::NovaContext` on `nova_embed`; test_main.cpp's 8 context tests ported (recursion depth, timers, a string past the limit, host function failures, results that cannot be copied, bundles compiled once, arguments as the C interface gave them, deadlines) |
| S4 | done: `twidget_engine` (sandbox_engine.cpp: boot, every host call kind, the quota and the main-thread window, deferrals, timers, frames, scripts, the bridge, storage, requests and the wallet, the query guard, stats, the kill texts) and `TrustedValidator`; 4 engine and 6 validator tests in process on the shipped bundles, the rest through T6 |
| S5 | done: `twidget_server` (`SandboxRemoteServer`, the serving loop with its Output, SyncSlot, EngineSink and routes) and `twidget_engine_ffi` (the C ABI, 29 `unsafe`; `nova-c-api` adds Nova's C interface for a C++ program that drives Nova). T6 in process through the C ABI rather than in a child: widget-sandbox `-DTW_RUST_ENGINE_END=ON`, `TW_ENGINE_STREAM=1 TW_ENGINE_END=rust`: 230 tests, 13728 checks, 0 failures, 128 of 128 fixture engines over the stream, per-test times alike (116.4 s, the C++ end 116.5 s); 133 attacks, 0 leaks, 7 killed, compat 19/19 and generated 20/20 with every per-file row the C++ end's; the end dropping diagnostics failed 34 of 36 `sdk_` tests |
| S6 | done: `twidget_host` (widget-sandbox): `SandboxHostedRuntime`, `HostedPlatform` with the glue's wait accounting, the served `WidgetHost` and its local glue behind a `Renderer` trait, `serve_hosting` (the hosting opening, its kinds, the platform slot); `twidget_engine_ffi` gains `twidget_engine_serve_hosting` with a C renderer factory and renderer table (79 `unsafe` there in all, each with its SAFETY note). Under `TW_ENGINE_HOSTED=stream` both ends give 230 tests and 0 failures, 133 attacks with 0 leaks and identical per-file rows, compat 19/19 and generated 20/20 alike; `hosted_` 38 and `remote_stream_hosted_` 16 tests pass on the Rust end; a planted glue-wait bug failed its test. The renderer is the C++ harness's through the C ABI until stage 5. On nova-safe's tdesktop-perf line (32-bit capable) the engine-only suite passes on Win32 too, both ends, row for row |
| H3 | started (Windows, 2026-11-20): TelegramWidgetHelper links `twidget_engine_ffi` as its one static Rust library (cargo in the tdesktop build, `x86_64-win7-windows-msvc` with `build-std` and a static CRT as `tdesktop_rust`); it built and linked for Win64 Debug and Release, its imports ADVAPI32, KERNEL32 and ntdll with bcrypt delay-loaded as before, plus `/DELAYLOAD:userenv.dll` for Rust std's `env::home_dir` (never called; not a KnownDLL). Not committed: the owner moves the whole widget runtime into the helper (no Nova in Telegram, no DLLs) and fixes nova-safe's build that way; the recipe is reapplied on top of it. Nova supports 64-bit targets only (its `Value` is one 64-bit word), so no Win32 build runs widgets on nova-safe |

## Findings

What the comparisons found about the C++, kept for the stages that meet it again.

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
