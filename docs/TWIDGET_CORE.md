# TwidgetCore

TwidgetCore is a renderer for widget-sandbox's renderer protocol in portable C++. widget-sandbox owns the protocol (`docs/protocol.md` there) and the
sandbox and validator in front of it. TwidgetCore takes the validated messages, lays the widget
out with the litehtml fork in `litehtml/`, and hands a platform display lists and
compositor layers to draw.

The Apple renderer in `apple/` is a separate renderer on the same fork, written in Swift for Telegram
for macOS.

Its sources are in `core/`: `src/` and `include/` below are its folders, and every other path starts at the repository root.

## What it does

| Area | Where |
|---|---|
| The protocol: ops into a shadow tree, queries, events, theme, freeze | `src/renderer.*` |
| SDK v1's theme variables (palette, colour scheme, button and text tokens) and the profile's UA layer | `src/css.*`, `Renderer::apply_theme` |
| CSS animations and transitions, Web Animations records (`anim`), the `anims` query | `src/animations.*` |
| Pointer and hover events, trusted; moves coalesced per frame; the gesture claim, and `dblclick` and `contextmenu` as widget-sandbox's gesture policy decides | `src/renderer.cpp` |
| Canvas 2D: the command interpreter, resolved into `CanvasOp`s a backend draws | `src/canvas.*`, `src/canvas_ops.h` |
| Compositor layers for transform and opacity animations | `src/compositor.*` |
| Heavy boxes (widget-sandbox v0.6.0): the count before every frame, the `limit` message, the stopped paint | `src/paint_policy.*`, `Renderer::count_paint` |
| `light-dark()`, name checks on everything written into the generated HTML | `src/css.*` |
| Binary forms of display lists, damage, canvas batches and font metrics for a painter in another process, with strict decoders | `src/wire.*` |

A batch of ops is serialised to HTML, which the fork patches into the live document where it can
(`update_from_string`), and animation frames patch style attributes in place (`update_styles`).

## What a platform implements

- **`twlite::Backend`** (`src/backend.h`): fonts with metrics, text width, image decoding, and the
  canvas bitmaps (`canvas_run`, `canvas_read`, `canvas_release`). A font comes with the OpenType
  features to apply (`tnum=1,zero=1`), and reports its em box from its OS/2 table when the platform
  can read it. A decoded PNG goes through `png_intact_rows` (`src/png.h`), which says how many rows to
  keep when the stream is damaged, and `image_dimensions` there reads the size a header declares, for a
  backend to refuse an image before decoding it. The container hands a backend only the profile's image
  types (`data:` PNG, JPEG, GIF, WebP): an SVG in any spelling is the placeholder, drawn by the core.
- **A painter** for `DisplayList` (`src/display_list.h`): rectangles, borders, text, images,
  gradients, clips, paths, groups and shadows, in CSS px, and the CSS filters, whose colour matrices
  and whose CPU pass over a painter's own pixels are in `src/filters.h`.
- **A clock.** Call `tick()` while `wants_tick()` holds, and once `next_tick_in()` seconds have passed
  when it does not. A build inside `paint()` or `last_html()` keeps `wants_tick()` until a `tick()` hands
  out its messages.
- **Input.** `touch` and `hover`. `wheel` and `key` remain but do nothing: widget profile 1 gives a
  widget no wheel and no keyboard. For text controls, `editor()` describes the one being edited for
  the platform to cover with its own editor, which reports back through `edit()` and `end_editing()`;
  profile 1 has no text controls, so it only matters to a host that hands the renderer one. `cursor()`
  is the document's CSS cursor where the mouse last was.
- **Who gets a press** (widget-sandbox v0.1.2, `docs/sdk-v1.md` §5.5). Every primary pointerdown sends a
  `gesture` message with the claim (`x`, `y`, `hold`) from `touch-action`. The host drives its own recognizers
  over the widget (the message menu, swipe-to-reply, the list scroll, back) with widget-sandbox's
  `twidget::GestureArbiter` on that claim, and when the arbiter gives a press to the bubble it ends it for
  the widget with `touch` action 3. The renderer runs the same policy for what the widget gets: `dblclick`
  after the second tap's click, and `contextmenu` for a hold or a secondary-button press (`button` 2) that
  the claim gives the widget. A hold needs the clock: its deadline is in `next_tick_in()`. A host with no
  recognizers of its own sends `arbitrate` with `bubble: true`, and the renderer then takes those presses
  itself and reports each in a `bubble` message.
- **Text selection.** A mouse drag over `user-select: text` selects (widget-sandbox v0.1.1, §5.10) and the
  next paint tints it; the host copies `selected_text()` with its own command and calls `clear_selection()`
  for a press the renderer does not get. For touch, a host selects with its own UI (the message menu and
  its handles): `selectable_text_json()` lists the selectable runs with their fonts and baselines, and the
  host lays its selection over them; the Telegram Android client does so. Each run carries the matrix it
  shows under (CSS transforms as last painted, its layer's track as of the call), so a host asks again
  after a paint or while a layer moves.
- **The paint limit** (widget-sandbox v0.6.0, `paint-policy.ts`). Before every frame the renderer counts the
  boxes whose computed style blurs a shadow, filters, has a backdrop-filter or opacity below 1, or runs an
  animation of one; past 16 (2 with backdrop-filter) it sends `limit` with the code and the count, once, and
  paints from then on without blur, filters, backdrop or transparency, its animations stopped. A host passes
  that to `twidget::Renderer::limitExceeded`; widget-sandbox's `WidgetHost` does so and stops the widget.
  `{"type":"policy","policy":"compat"}` turns the count off.
- **Tools that lay the document out elsewhere** read `master_css()` (litehtml's, the renderer's own rules
  and the UA layer with the theme's values) and `theme_variables()`.

With `set_resize_tracking(true)`, each height change comes with `resize_diff()` (and `resize` in the
size message): how much of the top and of the bottom stayed put, where the element pressed last went,
and which side to keep still, as the Swift renderer's `WidgetResizeDiff` decides it. Which edge of the
bubble stays put is the widget's choice since widget-sandbox v0.1.3 (`setResizeAnchor`, reported by the host
as `WidgetDiagnostics.resizeAnchor`, `top` by default): a host keeps that edge still and shows the frame of
the new height in the same frame as the bubble's new size. The diff's side is a guess from the content, for
a host that has no anchor to follow.

With `set_compositing(true)`, the platform also plays `composition()`: a list of layers, each an
element's own display list with a track of linear keys (a 2D matrix and an opacity per key) that the
core has already baked from the CSS timing. Easing, `steps()`, direction, iterations and Web
Animations timing all become linear keys, so any compositor with linear keyframes can play them.
`resume_track()` helps one that cannot start an animation part way through. An element a script
moves through transform or opacity three batches running becomes a layer too, holding one key; a
batch that only moves such layers changes their keys without a build, and the next `paint()` hands
out the last list with empty damage. A shimmer (a gradient tile whose background-position
animates) is its tile sliding under the element's clip, and an SVG a script keeps changing is a layer
that repaints alone. Without compositing, the core paints every frame.

The renderer is not thread-safe: one widget's calls must not overlap.

## Platforms

Each platform's backend is in `platforms/`. Apple, Android and the web reach the core through the C
ABI in `include/twidget_litehtml.h` (input, frames, layers and paint), Windows and Qt through the C++ API.
The C ABI, the wasm exports and JNI make the same calls, camelCased in Kotlin and Java; only what one
platform alone needs differs (`twl_render`, Apple's CPU raster; `twl_backend_id`, `twl_alloc` and
`twl_free` on the web; `filterPixels` on Android). What the `Renderer` gives a host goes into all three.
Every platform draws the same display list commands and runs canvases:

| Platform | Backend | Painter | What the build produces |
|---|---|---|---|
| iOS, macOS | `platforms/apple/`: CoreText, CoreGraphics, ImageIO | `CGBackend::paint`, canvases as bitmap contexts | `libtwidget_litehtml.a`, merged into one archive by `tools/build-apple.sh` |
| Android | `platforms/android/jni.cpp` over `platforms/android/kotlin/` | `DisplayListPainter`, `CanvasBitmaps` on `android.graphics` | `libtwidget_litehtml.so`, plus the Kotlin sources for the host's Gradle build |
| Web | `platforms/web/wasm.cpp` over `platforms/web/painter.ts` | `DisplayListPainter`, `CanvasBitmaps` on canvas 2D | `twidget_litehtml.js` and `.wasm`, plus the TypeScript for the host's bundle |
| Windows | `platforms/windows/`: DirectWrite, WIC and Direct2D | `D2DBackend::paint` | `twidget::d2d`, which a host paints with |
| Qt (any desktop) | `platforms/qt/`: QFont, QImage and QPainter, canvas included | `QtBackend::paint` into a QImage | `twidget::qt` with `-DTWIDGET_QT=ON` |

Android and the web paint from JSON (`to_json` of the display list, the canvas ops and the
composition, in `src/encode.cpp`); the keys are documented next to each `to_json`.

Where a platform API falls short, the platform does the nearest thing:
- Android before API 29 draws the blend modes beyond Porter-Duff, multiply, screen, overlay, darken
  and lighten as source-over; its radial gradients have no focus.
- Direct2D draws every canvas blend mode beyond Porter-Duff as source-over and has no conic brush on
  canvas.

The adapters that plug a renderer into a host's own interface stay with the host: widget-sandbox's
desktop core, its demos, and Telegram's clients.

## Binary forms

`src/wire.h` (namespace `twlite::wire`) gives four paint values a binary form, for a renderer and a
painter in two processes: a display list, damage, a batch of canvas ops and `FontMetrics`. The forms
are lossless: every member comes back, `Command::key` and the members a kind does not paint included,
each float with its bits (NaN payloads, signalling NaNs, -0, infinities), and a payload the value
shares is shared again. They are versioned, and their decoders take hostile bytes. They build as
`twidget::wire`, which is `src/wire.cpp` alone: it includes only the value headers (`src/backend.h`,
`src/canvas_ops.h`, `src/display_list.h`) and the standard library and links nothing, so a program
that uses the forms links no litehtml, no renderer, no Qt and no platform API. Configuring twidget
still needs widget-sandbox for its other targets (see Building); `twidget::wire` includes and links
nothing from it. The JSON forms above are separate and stay as they are: they round px values and
leave `Command::key` out. Compositor layers (`Composition`, and the `Affine` only they hold) have no
binary form.

- `encode(value, caps, out)` appends the value to `out`. It returns false, `out` cut back to its size
  on entry, for what the decoder would refuse under the same caps: a count or a length past its cap
  or past `UINT32_MAX`, or an enum value version 1 does not know (`Cmd`, `CanvasOp::Kind`,
  `Composite`, `CanvasPaint::Kind`, `Filter::Kind`, `Path::Op`), in a member its kind paints or not.
  Font metrics have nothing to refuse.
- `decode(bytes, caps, out)` takes the one value all of `bytes` hold. It returns false and leaves
  `out` untouched for anything else: bytes cut short or left over, another form or version, a count,
  length, reference, enum value, flag or mask bit out of bounds. It replaces `out` only once the last
  byte has passed.
- An allocation the caps allow may throw `std::bad_alloc`, as elsewhere in TwidgetCore. A decoder's
  `out` is then still untouched; an encoder's may keep part of the value.
- The decoders check structure, not what a painter makes of it: whether floats are finite, text is
  UTF-8, ids name anything, a path's points match its ops, or what a list costs to paint. A host
  that paints for another process checks those itself.

**Fields.** Little-endian, assembled byte by byte:

| Field | Bytes |
|---|---|
| `u8`, `u32`, `u64` | an unsigned integer of 1, 4 or 8 bytes |
| `i32` | an int's two's-complement bits as a `u32` |
| `f32` | a float's IEEE-754 bits as a `u32` |
| `f32[N]` | N `f32` in index order |
| `flag` | a `u8`, 0 or 1; anything else is refused |
| `color` | `r`, `g`, `b`, `a` as four `u8` |
| `rect` | `x`, `y`, `w`, `h` as four `f32` |
| `str` | a `u32` length (cap `text`), then that many bytes as they are |
| `count` | a `u32` n of items of at least so many bytes each |
| `stops` | `count` (cap `stops`, 8 bytes) × (`f32` offset, `color`) |
| `floats` | `count` (cap `floats`, 4 bytes) × `f32` |
| `pathops` | `count` (cap `path_ops`, 1 byte) × `u8`, each at most 4 (`Path::Close`) |
| `geometry` | `pathops` (the ops), then `floats` (the points) |
| `ref` | a `u32`: 0 = a new payload follows, k = the k-th payload of its kind so far |

A decoder refuses a count past its cap or when the bytes left cannot hold that many of its items at
their smallest, and a length past its cap or past the bytes left.

**Form and version.** Every value starts with a `u8` form (`wire::Form`: 1 display list, 2 damage, 3
canvas batch, 4 font metrics) and a `u8` version, 1 for every form (`wire::VERSION`). A decoder
refuses any other pair before it makes room for anything. A changed layout takes a new version or a
new form, and version 1's bytes keep their meaning.

**Masks and defaults.** A `Command`, a `Path`, a `CanvasOp` and a `CanvasPaint` are their kind, which
is always written (a Path has none), a mask, and then, in bit order, the members whose bits the mask
sets. A member is written when it differs from its version-1
default, floats by their bits, whether its kind paints it or not; a bool is its bit alone, set for the
value that is not its default. An absent member decodes to its default. A decoder also takes a member
written at its default value and a list written empty. Mask bits past the last member are refused.
The defaults are fixed for version 1, not taken from what the structs' initializers say later:

- numbers 0, bools false, strings and lists empty, payloads null, colours `{0, 0, 0, 255}`, a
  geometry with no ops and no points;
- `Command::opacity` 1, `Command::matrix` `{1, 0, 0, 1, 0, 0}`;
- `Path::stroke_width` 1, `Path::miter_limit` 4;
- `CanvasOp::line_width`, `alpha` and `scale_x` 1, `miter_limit` 10, `sw` and `sh` -1, `smoothing`
  true, `composite` SourceOver, `shadow_color` `{0, 0, 0, 0}`, `matrix` `{1, 0, 0, 1, 0, 0}`, and the
  paint Solid with the default colour, zero args and no stops.

**Shared payloads.** `Command::path`, `Command::filter`, `CanvasOp::clips` and `CanvasOp::rgba` are
`ref`s. A value keeps a table of each kind's payloads in the order they first appear: paths and filter
lists in a list, clip lists and pixels in a batch. 0 writes a new payload inline and adds it to the
table; k from 1 names the k-th entry, refused past the entries so far. A table belongs to one value,
so a value cannot name another's payloads. The encoder tells payloads apart by their address: a
payload several commands or ops share is written once and decoded into one shared object, and two
equal payloads stay two. Null is an absent member and an empty payload a present one, so the two
stay apart. References need no cap: a table gains at most one entry per command or op. What a decoder
returns owns its payloads, strings and pixels; nothing in it points into `bytes`.

**Display list (form 1).** `count` (cap `commands`, 5 bytes) × Command. A Command is a `u8` kind (at
most 17, `Cmd::Backdrop`), a `u32` mask (bits 24-31 must be 0), then:

| Bit | Member | Bytes |
|---|---|---|
| 0 | rect | `rect` |
| 1 | radii | `f32[8]`: tlx, tly, trx, try_, brx, bry, blx, bly |
| 2 | clip | `rect` |
| 3 | has_clip | none: set = true |
| 4 | color | `color` |
| 5 | width | `f32` |
| 6 | font | `i32` |
| 7 | text | `str` |
| 8, 9 | x, y | `f32` each |
| 10, 11 | image, canvas | `i32` each |
| 12-15 | ax, ay, bx, by | `f32` each |
| 16 | stops | `stops` |
| 17 | path | `ref`; at 0 a Path |
| 18 | filter | `ref`; at 0 a filter list |
| 19 | opacity | `f32` |
| 20 | matrix | `f32[6]` |
| 21 | hidden | none: set = true |
| 22 | inset | none: set = true |
| 23 | key | `u64` |

A Path is a `u32` mask (bits 25-31 must be 0), then:

| Bit | Member | Bytes |
|---|---|---|
| 0 | ops | `pathops` |
| 1 | points | `floats` |
| 2 | fill | none: set = true |
| 3 | evenodd | none: set = true |
| 4 | fill_color | `color` |
| 5 | gradient | `i32` |
| 6-12 | x1, y1, x2, y2, r, fx, fy | `f32` each |
| 13 | stops | `stops` |
| 14 | stroke | none: set = true |
| 15 | stroke_color | `color` |
| 16 | stroke_width | `f32` |
| 17, 18 | line_cap, line_join | `i32` each |
| 19 | miter_limit | `f32` |
| 20 | dashes | `floats` |
| 21 | dash_offset | `f32` |
| 22 | stroke_gradient | `i32` |
| 23 | sg | `f32[7]` |
| 24 | stroke_stops | `stops` |

A filter list is `count` (cap `filters`, 17 bytes) × (`u8` kind, at most 9 `Filter::DropShadow`;
`f32` amount, `f32` x, `f32` y, `color`): every filter has all its fields.

**Damage (form 2).** A `flag`: 0 = the whole frame is to be painted (`nullopt`: `Renderer::damage`
returned false) and nothing follows; 1 = `count` (cap `rects`, 16 bytes) × `rect`, none when nothing
moved.

**Canvas batch (form 3).** `count` (cap `canvas_ops`, 9 bytes) × CanvasOp. A CanvasOp is a `u8` kind
(at most 6, `CanvasOp::PutImage`), a `u64` mask (bits 38-63 must be 0), then:

| Bit | Member | Bytes |
|---|---|---|
| 0 | path | `geometry` |
| 1 | evenodd | none: set = true |
| 2 | paint | a `u8` kind (at most 3, `CanvasPaint::Conic`), a `u8` mask (bits 3-7 must be 0), then bit 0 color `color`, bit 1 args `f32[6]`, bit 2 stops `stops` |
| 3 | matrix | `f32[6]` |
| 4 | line_width | `f32` |
| 5, 6 | line_cap, line_join | `i32` each |
| 7 | miter_limit | `f32` |
| 8 | dashes | `floats` |
| 9 | dash_offset | `f32` |
| 10 | alpha | `f32` |
| 11 | composite | `u8`, at most 25 (`Composite::Luminosity`) |
| 12 | shadow_color | `color` |
| 13-15 | shadow_blur, shadow_x, shadow_y | `f32` each |
| 16 | clips | `ref`; at 0 `count` (cap `clips`, 9 bytes) × (`geometry`, `flag` evenodd) |
| 17 | font | `i32` |
| 18 | text | `str` |
| 19-22 | x, y, scale_x, text_dx | `f32` each |
| 23 | stroke_text | none: set = true |
| 24, 25 | image, canvas | `i32` each |
| 26-33 | sx, sy, sw, sh, dx, dy, dw, dh | `f32` each |
| 34 | smoothing | none: set = false |
| 35 | rgba | `ref`; at 0 a `u32` length (cap `pixels`) and the bytes |
| 36, 37 | w, h | `i32` each |

The paint is there when its kind, colour, args or stops differ from the default paint, and inside it
the same rule sets its own mask. The length of the pixels is not checked against `w` × `h`.

**Font metrics (form 4).** Seven `f32`: ascent, descent, height, x_height, zero_width, em_ascent,
em_descent; 30 bytes in all.

**Caps.** `wire::Caps` holds the caller's bounds. Each bounds one counted field of a value, not a sum
of them or what the value costs to paint: the length of the bytes bounds the total, and the channel
that carries them bounds that. A count is a `u32` whatever its cap. A decoder makes room for a list
only once its count has passed both checks, so what it allocates follows the bytes it is given, not
its caps. The defaults are generous bounds, not measured maxima; a caller picks its own.

| Cap | Bounds | Default |
|---|---|---|
| `commands` | the commands of one display list | 2^20 |
| `canvas_ops` | the ops of one batch | 2^20 |
| `path_ops` | the ops of one Path or geometry | 2^22 |
| `floats` | the points of one Path or geometry; the dashes of one Path or CanvasOp | 2^22, above what the canvas lets one path hold |
| `stops` | the stops of one gradient: a Command's, a Path's fill or stroke, a CanvasPaint's | 2^16 |
| `filters` | the functions of one filter list | 2^12 |
| `clips` | the clips of one clip list | 2^16 |
| `text` | the bytes of one `str` | 4 MiB, as much text as the renderer holds |
| `pixels` | the bytes of one rgba payload | 64 MiB, the RGBA of the largest canvas bitmap |
| `rects` | the rectangles of one damage | 2^12; the compositor merges damage past 32 |

`twidget_wire_tests` (ctest `twidget_wire`) links `twidget::wire` alone. Samples that set every kind
and member come back bit for bit and shared as they were, and encode to the bytes above. The decoders
refuse every cut of every sample, seeded random changes they cannot take, and counts, lengths,
references, enum values, flags and mask bits past their bounds, leaving the output as it was; no
decode asks the allocator for more than a bound linear in its bytes.

## Building

```sh
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
ctest --test-dir build
```

widget-sandbox has to be checked out next to twidget (`../widget-sandbox`), or its path given in
`TWIDGET_WIDGET_SANDBOX`, as for `Package.swift`: TwidgetCore reads the protocol's JSON helper
(`twidget/json.h`) and the gesture policy (`twidget/gesture_policy.h`, its source compiled into
`twidget::core`) from it. TwidgetCore follows release `v0.6.0`: check out that tag, never a branch,
and its QuickJS submodule with `git submodule update --init` (widget-sandbox's `docs/integration.md`). The
build reads the profile's UA layer from its `packages/core/profile.json` into a generated header.

The build produces `twidget::litehtml` (the fork and its gumbo), `twidget::core`, `twidget::wire`
(the binary forms, on the standard library alone) and the platform's backend; Android and Emscripten
builds go through their toolchain files. With `TWIDGET_BUILD_TESTS`
(on when twidget is the top-level project) it also builds the checks in `tests/`,
and on macOS the CoreGraphics tests and `twidget-core-run`, which turns a message log into a PNG.
TwidgetCore is C++20. Apple's SwiftPM build does not use TwidgetCore.

`-DTWIDGET_LITEHTML_NO_CORE_FOUNDATION=ON` builds `twidget::litehtml` without CoreFoundation, for a host
that may link only libSystem and the C++ runtime. It is off by default, and then litehtml asks
CoreFoundation for the locale's date pattern and links the framework. When on, `el_form.cpp` leaves out
the framework's header and that lookup, the target does not link the framework, and a date input uses
the fixed `MM/dd/yyyy`: a valid value is shown in that order, and an empty or invalid one shows the hint
`mm/dd/yyyy`. A build that compiles litehtml's sources itself defines `TW_LITEHTML_NO_CORE_FOUNDATION` on
them, a preprocessor guard that C++17 builds take too, and drops its own CoreFoundation link if nothing
else needs it. The switch changes only litehtml: the CoreGraphics backend `twidget_litehtml` and
`Package.swift` still link CoreFoundation. `twidget_core_tests` checks the hint an empty date input
paints, and on macOS the `twidget_litehtml_core_foundation` test checks the library's CoreFoundation
imports and the link of a program on litehtml alone against the switch.

`-DTWIDGET_QT=ON` adds the QPainter backend and, with the tests, `twidget_qt_tests`, which checks its
pixels on Qt's offscreen platform. `demos/qt` is a desktop demo on it: widget-sandbox's corpus
in a feed, each widget a `WidgetHost` from widget-sandbox's desktop core over TwidgetCore.

```sh
cmake -S demos/qt -B build-qt -G Ninja -DCMAKE_PREFIX_PATH=<Qt 6>/<compiler>
cmake --build build-qt
build-qt/twidget-qt-demo [corpus folder or .html file]
```

`demos/macos` is the same demo for macOS on `CGBackend`, with the compositor's layers on Core
Animation. Its README records the build and visual checks.

`demos/android` is the same demo for Android, set up as Telegram for Android is: Java with no
Kotlin or AndroidX, views built in code, and one native library built by CMake, holding the core with
`platforms/android/jni.cpp` and widget-sandbox's C++ WidgetHost behind a small JNI bridge. Its Java
copies the Kotlin backend and painter under `platforms/android`. Not yet built: see its README.

`tools/conformance` runs widget-sandbox's conformance set (`packages/conformance/CLIENTS.md` there) on
TwidgetCore, Windows only: each widget of the automatic set through the desktop host, drawn off screen
by the Direct2D backend at DPR 2, its scripted input sent through the renderer. It writes the run file
that the set's `tools/report.mjs` turns into the per-feature report. `docs/CONFORMANCE.md` tracks the results.
Its widgets get Intl from the system's ICU through widget-sandbox's `IcuIntl`, and it paints the default
palette's `bg_color` under the widget, as a client draws its bubble.

```sh
cmake -S tools/conformance -B build-conformance -G Ninja && cmake --build build-conformance
build-conformance/twidget-conformance <out> [--filter auto/03] [--width 360]
node ../widget-sandbox/packages/conformance/tools/report.mjs --out <out>/report <out>/run.json
```

`tools/stress` runs widget-sandbox's stress kit (`packages/stress/CLIENTS.md` there) the same way, painting
every frame with Direct2D on the GPU on a Windows client's frame clock. `--pass measure` is the kit's
measuring run under `compat`, `--pass production` the strict one that shows where each load stops.

```sh
cmake -S tools/stress -B build-stress -G Ninja -DCMAKE_BUILD_TYPE=Release && cmake --build build-stress
build-stress/twidget-stress <out> --pass measure|production [--filter <load>]
node ../widget-sandbox/packages/stress/tools/report.mjs --out <out>/report <out>/*.run.json
```

## Portability check, 2026-10-14

Checked on Apple Silicon with Xcode's AppleClang and Emscripten 6.0.12:

| Target | Result |
|---|---|
| macOS Release core and CoreGraphics backend | Built; both CTest suites passed |
| macOS demo | Built and launched with the canvas-market stress widget |
| iOS arm64, deployment target 16.3 | Static libraries built; no device execution |
| WebAssembly | JavaScript module and Wasm binary built; 11 web painter checks passed against recording fakes, not a browser pixel test |
| Android, Windows, Qt | Not built or run in this check |

The Mac build needed CoreFoundation linked by the litehtml target: its date input implementation
uses that framework. The demo now defaults to macOS 13.3, matching the floating-point
`std::to_chars` requirement already recorded in `tools/build-apple.sh`.

Of the three stress fixtures Telegram for macOS checks its renderer with, the canvas-market replay
and the live Mac demo show a flat fill in place of the chart content.

## Not yet here

- **A composition player for Windows.** A Windows host plays `composition()` on
  `Windows.UI.Composition` in its own adapter; none is in this repository.
- **Platform validation is incomplete.** Windows runs the conformance set and the stress kit
  (`tools/conformance`, `tools/stress`). The Qt backend and its demo were previously built against
  Qt 6.8 on Windows. Apple and Emscripten build results are recorded above. Android has only
  type checks: the JNI bridges against `jni.h`, the Kotlin and the demo's Java against `android.jar`;
  nothing has been built with the NDK or run on a device.
- **`TW_LITEHTML_TWIDGET`** keeps the code that needs the fork apart from code that also builds
  against upstream litehtml. With the fork the only litehtml, it can go.
