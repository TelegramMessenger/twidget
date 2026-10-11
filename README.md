# twidget

Native rendering for [twidget](#twidget-widgets) widgets on Apple platforms, built on a heavily optimized fork of [litehtml](https://github.com/litehtml/litehtml). It powers the interactive HTML messages in Telegram for macOS.

twidget is one framework shared by every client:
- **The shared parts.** A hostile sandbox runs the widget as written, and a validator checks every DOM operation against one policy.
- **The per-client parts.** Each client implements the engine glue and a renderer.

This repository is the Apple renderer. Validated operations come in, events and layout answers go out, and pixels come from litehtml instead of a WebView. It speaks the same host protocol as the WebView renderer, so any twidget host can use it. The twidget bundles themselves are not part of this repository.

| Product | What it is |
|---|---|
| `TwidgetRenderer` | The twidget renderer: mirror DOM, litehtml layout and drawing, events, layout queries, CSS animations and transitions, canvas 2D on CoreGraphics |
| `LiteHTML` | The renderer core: parsing, sanitizing, layout, CoreGraphics and Metal drawing, hit testing, text selection |
| `HTMLScript` | A lighter standalone mode: `render()` returns HTML, actions and timers drive it. Runs on QuickJS by default (hard heap cap, same engine as twidget's sandbox); JavaScriptCore is available as `JSCScriptRuntime` |
| `HTML3D` | `<script src="app:three">` for `HTMLScript`: three.js r160 with its WebGL renderer replaced by a Metal one, on QuickJS or JavaScriptCore |
| `HTMLSandbox` | The CPU quota policy and the large-stack script thread |

macOS 10.13+, Swift 5.5+, C++17.

```swift
.package(url: "https://github.com/macosswift/twidget.git", branch: "main")
```

## twidget widgets

`LiteHTMLWidgetRenderer` implements the renderer side of the twidget host protocol:
- **Host → renderer:** `ops`, `query`, `listen`, `preventDefault`, `requestFrame` and `frozen`.
- **Renderer → host:** `ready`, `size`, `event`, `gesture`, `result`, `frame`, `resize`, `media` and `violation`.

A host adapter is a few lines:

```swift
import TwidgetRenderer
import WidgetSandbox

extension LiteHTMLWidgetRenderer: Renderer {
    public func evaluate(_ script: String, completion: @escaping (String?) -> Void) { completion(nil) }
    public func snapshot(completion: @escaping (NSImage?) -> Void) { completion(snapshot()) }
}

let host = try WidgetHost(html: html, theme: theme, width: width) { _, theme, width in
    LiteHTMLWidgetRenderer(theme: theme, width: width)
}
```

How the protocol maps onto native code:

| Protocol | Native implementation |
|---|---|
| DOM operations | A mirror document, serialized and laid out by litehtml. Frames that only change styles or text are patched in place, not parsed again. |
| `light-dark()`, theme | Resolved for the current theme, re-rendered on `setTheme` |
| `@keyframes`, `animation`, `transition` | A native animation engine: timing functions, iterations, direction, fill modes, and interpolation of numbers, colors and transforms. It writes into an animation layer that litehtml keeps out of the cascade it reports back. |
| Canvas 2D | CoreGraphics: paths, Path2D, gradients, text, transforms, clipping, shadows, compositing, `getImageData` and `putImageData` |
| Layout queries | `rect`, `box`, `cs`, `measureText`, `elementFromPoint`, `matchMedia`, `viewport` and canvas calls, answered synchronously |
| Events | Pointer, mouse, click, double-click, hover, key and animation events, with per-frame coalescing of moves like the WebView renderer |

Both renderers render the ported demo corpus and Chart.js and D3 widgets. `tools/perflab/TwidgetLab` in the Telegram for macOS repository compares them side by side.

On the ported corpus, every widget lays out to WebKit's height within 4 pt, and the remaining pixel differences come from glyph antialiasing.

**Known gaps:**
- Form controls render as static content.
- No scroll containers.
- Fallback fonts for symbols such as ✕ can differ from WebKit's choice.
- WebGL is not part of the twidget protocol.

### Magic move

Mark a container with `data-transition="move"`. When an operations batch adds, removes or reorders its descendants, keyed descendants animate from their old boxes to their new ones instead of jumping:

```html
<div class="list" data-transition="move">
  <div data-key="a">…</div>
  <div data-key="b">…</div>
</div>
```

- An element's key is its `data-key`, or else its `id`. Keys must be unique; duplicates don't animate.
- An element whose key survives the batch moves and resizes on a spring. If its content changed, the old snapshot morphs along the same path and cross-fades into the new one.
- New keys fade and scale in, and removed keys fade and scale out.
- A keyed element inside another animating keyed element rides inside its ancestor's snapshot.
- Elements that are running a CSS animation or transition keep their own motion.
- A change that arrives mid-flight starts from the elements' on-screen positions.
- Nothing animates off screen, with Reduce Motion on, or beyond the layer budget (48 layers, 4096² pixels). The change then applies at once.

The moving elements are snapshots on Core Animation layers, so the flight costs no layout or drawing. At the end, the layers are removed in the same transaction that repaints the document.

## Rendering a document

```swift
import LiteHTML

let document = LHDocument(html: html, css: css)
document.colorSpace = CGColorSpace(name: CGColorSpace.displayP3)
let size = document.layout(withMaxWidth: 360)
document.draw(in: context, origin: .zero, clip: CGRect(origin: .zero, size: size))
```

- **Links and actions.** `anchorHandler` receives links that passed the sanitizer. `actionHandler` receives clicks on elements with `data-action` (and an optional `data-value`).
- **Pointer input.** `mouseMoved`, `mouseDown`, `mouseUp` and `cursor` handle hover and active states.
- **Animated documents.** Build each new frame with `LHDocument(html:css:previous:)`. When only inline `style` values and text changed, the previous tree is patched in place instead of being parsed again.
- **Partial repaints.** `displayList(maxWidth:)` and `changedRects(since:)` give exact dirty rectangles, so a host repaints only what changed.
- **Images.** `copyImage(maxWidth:rect:scale:base:dirty:)` redraws only a dirty area on top of the previous bitmap.

## Security model

Every document is validated before layout. The validator is an allowlist, not a blocklist:

- **Tags.** HTML and SVG tags are allowlisted. Scripts, iframes, frames, objects, embeds, audio and video, `<base>`, `<meta>`, `<link>` and `<template>` are dropped. Unknown tags render as `<span>`, with the original name kept in `data-lh-tag`, so their text survives and their behavior does not.
- **Attributes.** Attributes are allowlisted, every `on*` handler is dropped, and attributes with the reserved `data-lh-` prefix cannot be forged.
- **URLs.** Links may use `http`, `https`, `mailto` and `#fragment`. Images may only use `data:image/…` with an allowlisted MIME type. A host adds its own schemes with `LHSanitizer.setLinkSchemes(_:imageSchemes:)`. URLs with those schemes reach the host's link handler or image provider. The renderer never fetches anything.
- **CSS.** At-rules are allowlisted, so `@import`, `@font-face` and `@namespace` are removed. Every `url()` and image function is checked against the image rules, and `behavior` and `-moz-binding` are blocked.
- **Structure limits.** The parser stops at 256 levels of nesting, 20,000 elements and 200,000 table cells, so a hostile document cannot make layout explode.
- **Reporting.** `sanitizerCounts` and `sanitizerDetails` report what was removed, for logs and tests.

The policy lives in portable C++ (`lhx::SanitizerPolicy`, `lhx::Sanitizer` in `Sources/LiteHTML/core`), so other ports can share it.

## Standalone scripted documents

For hosts that don't run twidget, `HTMLScript` is a smaller model: the script returns HTML instead of driving a DOM.

A document with `<script>` blocks becomes an `HTMLScript`. The script's `render()` function returns the HTML to show. The host gets `scriptDidChange` whenever the output changes and draws `currentHTML`.

```swift
import HTMLScript

let script = HTMLScript.make(html: html)
script?.host = self
script?.setVisible(true)
let output = script?.currentHTML
script?.performAction("tap", value: nil)
```

**What scripts get:**
- `render()`, `onAction(name, value)` and `onKey` / `onPointer` / `onFocus`.
- `setTimeout`, `setInterval`, `requestAnimationFrame` and `performance.now`.
- `update()`, `transition(kind)` and `copyText` (only while handling a user action).
- `animate`, `tween` and `ease` helpers, plus `console`.

**What scripts don't get:** the DOM, `fetch`, XHR, WebSockets, storage, workers or WebAssembly.

**Limits:**

| Limit | Value |
|---|---|
| One call | 50 ms. A runaway call is terminated and the script is stopped. |
| Library load (three.js) | 2 s |
| CPU quota | 200 ms per sliding 1 s window. Timers and frames past the quota are deferred, never dropped. User events always run. |
| Memory | 128 MB per call (see below) |
| Timers | 16 at once, 50 ms minimum interval |
| Output | 256 KB of HTML per `render()` |
| Hidden documents | Timers and frames pause when the host calls `setVisible(false)`. Due timeouts fire when the document becomes visible again. |

On JavaScriptCore, the memory guard measures the process footprint around a call. That makes it a heuristic, not an isolation boundary. On QuickJS, the limit is a hard heap cap per document.

The frame clock is a `CVDisplayLink` bound to the display the host reports through `HTMLScript.setDisplay`, so animations run at the display's refresh rate, including 120 Hz ProMotion. `HTMLScript.errorMarkup` controls how a script error is shown.

## Bring your own engine

### JavaScript

The script engine is reached only through `HTMLScriptRuntime` and `HTMLScriptContext`:

```swift
public protocol HTMLScriptContext: AnyObject {
    func boot(prelude: String, native: [String], call: @escaping HTMLScriptNativeCall, arguments: [HTMLScriptValue]) throws
    func evaluate(_ source: String, name: String, timeLimit: TimeInterval?) throws
    func invoke(_ hook: String, _ arguments: [HTMLScriptValue]) throws -> HTMLScriptValue
}
```

- `boot` runs the prelude. The prelude evaluates to a function that takes a `native` object and returns the hooks object.
- `invoke` calls one of the hooks: `fire`, `frame`, `callRender`, `callAction`, `callInput` or `describe`.
- An engine throws `HTMLScriptError` with `.timeLimit`, `.outOfMemory` or `.exception`. The sandbox turns those into a stopped script or an error line.

`QuickJSScriptRuntime` (the default) and `JSCScriptRuntime` are the two implementations. QuickJS comes from the [macosswift/quickjs](https://github.com/macosswift/quickjs) submodule; clone with `--recurse-submodules` (SwiftPM fetches it automatically). To switch engines:

```swift
HTMLScript.runtime = JSCScriptRuntime.shared
HTMLScript.runtime = QuickJSScriptRuntime(memoryLimitBytes: 64 << 20)
```

QuickJS has no `Intl`. Scripts that format dates or numbers with it need a shim on that runtime.

### Libraries

`<script src="app:name">` loads a host library through `HTMLScriptLibrary`. Register a factory in `HTMLScript.libraryFactories["name"]`, and the script gets an instance of it, which the host reads back with `script.library(MyLibrary.self)`. `HTML3D` is one such library.

### GPU

There are two levels to plug into.

1. **Painter (immediate mode).** Subclass `lhx::Painter` (`Sources/LiteHTML/core/lhx_painter.h`) and call `paintWithMaxWidth:clip:painter:` from `LHDocumentPainting.h`. You receive every clip, group, text run, fill, gradient, image, border, shadow and SVG path in paint order, with resolved geometry. This is the right level for a Skia, Direct2D or WebGPU backend.
2. **Render list (retained).** Call `recordWithMaxWidth:recorder:` with an `lhx::RenderRecorder` (`include/LHRenderList.h`). The document becomes a flat list of `RenderOp`s:
   - `Fill`: a solid device rectangle.
   - `Mask`: a glyph or rounded corner from an alpha atlas, tinted with a color.
   - `Bitmap`: a content-keyed BGRA texture that the container rasterizes into a context you allocate.

   Your engine owns the atlas and texture cache. The callbacks (`cached`, `allocate`, `store`, `glyph`, `mask`) let it skip work it already has. `lhx::rasterizeGlyph` and `lhx::rasterizeMask` produce the same alpha coverage the built-in renderer uses.

`LHGPURenderer` is the Metal client of the render list. It uses tiled IOSurface targets, a glyph atlas, a 32 MB texture LRU and batched submission. If a document needs something the list can't express, `LHDisplayList.gpuSupported` is false and the host draws that document with CoreGraphics.

### Images

`LHImages.setProvider { url in … }` supplies images for the host's own schemes (the scheme must also be allowed with `LHSanitizer`). `LHImages.registerImage(_:name:)` registers `app:name` images. `sf:symbol.name?size=20&color=%23ff0000` renders SF Symbols on macOS 11+.

## three.js and Metal

```html
<script src="app:three"></script>
<script>
const renderer = new THREE.WebGLRenderer();
</script>
```

`HTML3D` bundles three.js r160 (MIT). Its WebGL renderer is replaced by an adapter that sends geometry, instances and a compact per-frame scene record to a native Metal renderer, so there is no WebGL context. The renderer supports:
- Standard and basic materials, vertex colors and instancing.
- Points.
- Up to four lights, with a directional shadow map.
- Transparency sorting.

Call `HTML3DSession.register()` once. Then attach the layer from `script.scene3D?.makeLayer()` to your view. The bridge reads typed arrays in place on both runtimes, through the JavaScriptCore C API or the QuickJS typed-array shim.

## Exactness

All work happens in the Lab, and every optimization is checked against a fresh render:

- **CoreGraphics path:** pixel-identical. `LiteHTMLLab bench --verify` replays recorded animation traces and compares every frame with a from-scratch render. `compare` diffs two runs trace by trace.
- **Metal path:** checked within a tolerance against CoreGraphics (`gpu --verify`). The thresholds are a maximum interior difference, a share of pixels more than 8 levels off (0.000% on the bundled traces) and zero structural differences.
- **Painter path:** receives exactly what the CoreGraphics path draws.

## Lab

`Lab/` is a separate package with these tools:

| Tool | What it does |
|---|---|
| `LiteHTMLLab` | Benchmarks, verification, `sanitize`, `scripttest [--runtime quickjs]`, `threetest` and a live viewer |
| `WebKitLab` | Renders the same chats in WKWebView, for comparison |
| `EngineLab` | Compares JavaScriptCore with QuickJS |
| `PresentLab` | Experiments with presentation paths |

Sample chats live in `Examples/chats`.

```sh
cd Lab
swift build -c release
.build/release/LiteHTMLLab record
.build/release/LiteHTMLLab bench --verify --label mine
.build/release/LiteHTMLLab gpu --verify
.build/release/LiteHTMLLab scripttest
```

## License

BSD 3-Clause, see `LICENSE`. Bundled third-party code (litehtml, Gumbo, QuickJS-ng and three.js) keeps its own license, listed in `THIRD_PARTY_NOTICES.md`.
