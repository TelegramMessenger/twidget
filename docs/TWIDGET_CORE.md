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
- **Input.** `touch`, `hover`, `wheel` and `key`.

With `set_compositing(true)`, the platform also plays `composition()`: a list of layers, each an
element's own display list with a track of linear keys (a 2D matrix and an opacity per key) that the
core has already baked from the CSS timing. Easing, `steps()`, direction, iterations and Web
Animations timing all become linear keys, so any compositor with linear keyframes can play them.
`resume_track()` helps one that cannot start an animation part way through. Without compositing,
the core paints every frame.

The renderer is not thread-safe: one widget's calls must not overlap.

## Building

```sh
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
ctest --test-dir build
```

The build produces `twidget_litehtml` (the fork and its gumbo) and `twidget::core`, and with
`TWIDGET_BUILD_TESTS` (on when twidget is the top-level project) the checks in
`Tests/TwidgetCoreTests`. TwidgetCore is C++20. Apple builds through `Package.swift` and does not
use TwidgetCore yet.

## Not yet here

- **The platform glue** still lives in widget-sandbox: the CoreGraphics backend and C ABI for Apple,
  the JNI backend for Android, the wasm bindings for the web. The Direct2D backend and the Windows
  Composition player live in a Windows spike.
- **`TW_LITEHTML_TWIDGET`** keeps the code that needs the fork apart from code that also builds
  against upstream litehtml. With the fork the only litehtml, it can go.
