# TwidgetCore

TwidgetCore is the renderer of the twidget protocol in portable C++: what `TwidgetRenderer` does in
Swift, for every client. widget-sandbox owns the protocol (`docs/protocol.md` there) and the
sandbox and validator in front of it. TwidgetCore takes the validated messages, lays the widget
out with the litehtml fork in `Sources/LiteHTML/litehtml`, and hands a platform display lists and
compositor layers to draw.

The plan is one renderer everywhere: TwidgetCore becomes the renderer on Apple platforms too, and
the Swift code shrinks to the Apple backend (CoreGraphics or Metal drawing, Core Animation layers,
magic move). Until then both exist side by side.

## What it does

| Area | Where |
|---|---|
| The protocol: ops into a shadow tree, queries, events, theme, freeze | `src/renderer.*` |
| CSS animations and transitions, Web Animations records (`anim`), the `anims` query | `src/animations.*` |
| Pointer, hover, wheel and key events; moves coalesced per frame | `src/renderer.cpp` |
| Canvas 2D: the command interpreter, resolved into `CanvasOp`s a backend draws | `src/canvas.*`, `src/canvas_ops.h` |
| Compositor layers for transform and opacity animations | `src/compositor.*` |
| `light-dark()`, name checks on everything written into the generated HTML | `src/css.*` |

A batch of ops is serialised to HTML, which the fork patches into the live document where it can
(`update_from_string`), and animation frames patch style attributes in place (`update_styles`).

## What a platform implements

- **`twlite::Backend`** (`src/backend.h`): fonts with metrics, text width, image decoding, and the
  canvas bitmaps (`canvas_run`, `canvas_read`, `canvas_release`).
- **A painter** for `DisplayList` (`src/display_list.h`): rectangles, borders, text, images,
  gradients, clips, paths, groups and shadows, in CSS px.
- **A clock.** Call `tick()` while `wants_tick()` holds, and once `next_tick_in()` seconds have passed
  when it does not.
- **Input.** `touch`, `hover`, `wheel` and `key`; for text controls, `editor()` describes the one being
  edited for the platform to cover with its own editor, which reports back through `edit()` and `end_editing()`.

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

Each platform's backend is in `platform/`. Apple, Android and the web reach the core through the C
ABI in `include/twidget_litehtml.h` (input, frames, layers and paint), Windows through the C++ API.
Every platform draws the same display list commands and runs canvases:

| Platform | Backend | Painter | What the build produces |
|---|---|---|---|
| iOS, macOS | `apple/`: CoreText, CoreGraphics, ImageIO | `CGBackend::paint`, canvases as bitmap contexts | `libtwidget_litehtml.a`, merged into one archive by `tools/build-apple.sh` |
| Android | `android/jni.cpp` over `android/kotlin/` | `DisplayListPainter`, `CanvasBitmaps` on `android.graphics` | `libtwidget_litehtml.so`, plus the Kotlin sources for the host's Gradle build |
| Web | `web/wasm.cpp` over `web/painter.ts` | `DisplayListPainter`, `CanvasBitmaps` on canvas 2D | `twidget_litehtml.js` and `.wasm`, plus the TypeScript for the host's bundle |
| Windows | `windows/`: DirectWrite, WIC and Direct2D | `D2DBackend::paint` | `twidget::d2d`, which a host paints with |
| Qt (any desktop) | `qt/`: QFont, QImage and QPainter, canvas included | `QtBackend::paint` into a QImage | `twidget::qt` with `-DTWIDGET_QT=ON` |

Android and the web paint from JSON (`to_json` of the display list, the canvas ops and the
composition, in `src/encode.cpp`); the keys are documented next to each `to_json`.

Where a platform API falls short, the platform does the nearest thing:
- Android before API 29 draws the blend modes beyond Porter-Duff, multiply, screen, overlay, darken
  and lighten as source-over; its radial gradients have no focus.
- Direct2D draws every canvas blend mode beyond Porter-Duff as source-over and has no conic brush on
  canvas.

The adapters that plug a renderer into a host's own interface stay with the host: widget-sandbox's
desktop core, its demos, and Telegram's clients.

## Building

```sh
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
ctest --test-dir build
```

widget-sandbox has to be checked out next to twidget (`../widget-sandbox`), or its path given in
`TWIDGET_WIDGET_SANDBOX`, as for `Package.swift`: TwidgetCore reads the protocol's JSON helper
(`twidget/json.h`) from it.

The build produces `twidget::litehtml` (the fork and its gumbo), `twidget::core` and the platform's
backend; Android and Emscripten builds go through their toolchain files. With `TWIDGET_BUILD_TESTS`
(on when twidget is the top-level project) it also builds the checks in `Tests/TwidgetCoreTests`,
and on macOS the CoreGraphics tests and `twidget-core-run`, which turns a message log into a PNG.
TwidgetCore is C++20. Apple's SwiftPM build does not use TwidgetCore yet.

`-DTWIDGET_QT=ON` adds the QPainter backend and, with the tests, `twidget_qt_tests`, which checks its
pixels on Qt's offscreen platform. `tools/qt-demo` is a desktop demo on it: widget-sandbox's corpus
in a feed, each widget a `WidgetHost` from widget-sandbox's desktop core over TwidgetCore.

```sh
cmake -S tools/qt-demo -B build-qt -G Ninja -DCMAKE_PREFIX_PATH=<Qt 6>/<compiler>
cmake --build build-qt
build-qt/twidget-qt-demo [corpus folder or .html file]
```

`tools/macos-demo` is the same demo for macOS on `CGBackend`, with the compositor's layers on Core
Animation. Its README records the build and visual checks.

`tools/android-demo` is the same demo for Android, set up as Telegram for Android is: Java with no
Kotlin or AndroidX, views built in code, and one native library built by CMake, holding the core with
`platform/android/jni.cpp` and widget-sandbox's C++ WidgetHost behind a small JNI bridge. Its Java
copies the Kotlin backend and painter under `platform/android`. Not yet built: see its README.

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

This is not yet a replacement for Telegram's Swift renderer. The three fixtures in Telegram's
`tools/perflab/widget-stress` expose gaps beyond the existing unit coverage:

- The canvas-market replay and the live Mac demo show a flat fill in place of the chart content.

`docs/PERFORMANCE_PARITY.md` tracks the rest against the Swift renderer, item by item.

The path to one implementation is to move renderer decisions into this core while retaining
platform painting, text, input, and compositor playback adapters. Port and verify the existing
fast paths before switching production clients. Compare the shared core and Swift renderer on
the same Mac first, using deterministic pixels, interactions, CPU, memory, and idle wakeups;
then run the same fixtures on actual target devices. A build or a passing protocol test does not
establish visual or performance parity. Shared algorithms can avoid duplicated work, but equal
CPU percentages across different hardware and drawing backends are not a meaningful guarantee.

## Not yet here

- **The Composition player for Windows** (layers on `Windows.UI.Composition`) lives in a Windows
  spike with its host adapter.
- **Platform validation is incomplete.** The Qt backend and its demo were previously built against
  Qt 6.8 on Windows. Apple and Emscripten build results are recorded above. Android has only
  type checks: the JNI bridges against `jni.h`, the Kotlin and the demo's Java against `android.jar`;
  nothing has been built with the NDK or run on a device.
- **The hosts still use widget-sandbox's renderer copy.** Its Android and web demos build their own
  renderer against upstream litehtml; moving them onto this tree means building `libtwidget_litehtml`
  from here and replacing their painters with the ones in `platform/`.
- **`TW_LITEHTML_TWIDGET`** keeps the code that needs the fork apart from code that also builds
  against upstream litehtml. With the fork the only litehtml, it can go.
