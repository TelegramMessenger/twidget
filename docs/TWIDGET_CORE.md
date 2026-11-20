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
| The renderer and its platform at the two ends of a byte channel, in versioned messages with strict decoders | `src/remote.*`, `src/renderer_end.*`, `src/platform_end.*` |
| A face's cache of measured widths, and a layout's unknown widths in a few bulk calls | `src/text_widths.*` |

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
  `text_widths` measures many texts in one call, each in its own font; its default is `text_width` for
  each in order, and a backend that measures them together overrides it. The renderer asks for a
  layout's new words, the units its paint spaces and the breaks it measures that way ("A layout's
  words" below).
  A canvas's fonts come from `create_canvas_font`, which is `create_font` with empty features unless
  the backend overrides it to tell them apart, for an allowance of their own. The renderer caches them
  by the CSS spelling of `font`, and releases them all once it holds more than 64 and at its
  destruction: one font in two spellings is two handles, each released once, so a backend that hands
  out one font for several counts its handles. An embedder that keeps canvas fonts its own way sets
  `set_canvas_font_hook` instead: each time canvas text (fillText, strokeText, measureText) needs a
  font, the hook gets the renderer's Backend and the spelling unparsed, as the canvas holds it (at
  most 512 bytes) or measureText names it (any length), and returns a handle, 0 for none, with its
  metrics. The renderer neither parses nor caches the spelling then, and gives the metrics no em box
  of its own, so the hook sets one as `set_em_box` does here. The hook's fonts are its own: the
  renderer releases none of them, neither past 64 nor at its destruction. A canvas batch reaches
  `canvas_run` after all its texts got their fonts, so a font the hook releases while one is being
  run must not be one it gave to that batch. Setting a hook releases the fonts the renderer cached
  before; an empty hook gives the cache back. Without one nothing changes.
- **A painter** for `DisplayList` (`src/display_list.h`): rectangles, borders, text, images,
  gradients, clips, paths, groups and shadows, in CSS px, and the CSS filters, whose colour matrices
  and whose CPU pass over a painter's own pixels are in `src/filters.h`.
- **A clock.** Call `tick()` while `wants_tick()` holds, and once `next_tick_in()` seconds have passed
  when it does not. A build inside `paint()`, `last_html()` or `claim_at()` keeps `wants_tick()` until a
  `tick()` hands out its messages. The renderer reads the steady clock, or the `twlite::Clock` given to
  its constructor (`src/renderer.h`): `now_ms()`, finite, non-negative and nondecreasing milliseconds
  from an origin the host picks, read on the renderer's thread; the Clock outlives the renderer.
  Everything timed in that renderer follows it, and nothing in another: animations and transitions,
  where `anim` records start, move coalescing, holds and the gesture arbiter, the render throttle and
  the time it charges, the build timings in `stats()`. The static `Renderer::clock()` stays the steady
  clock in seconds. Compositor tracks start on the renderer's clock in seconds: on `Renderer::clock()`
  for a renderer given no Clock, on the given one otherwise, which a host then samples its tracks on.
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
  itself and reports each in a `bubble` message. `claim_at(x, y)` answers without pressing: the claim a
  press there would get when it starts no selection, which says whether a hold would be claimed, and
  no claim while frozen. It neither presses nor selects and returns no event.
- **Text selection.** A mouse drag over `user-select: text` selects (widget-sandbox v0.1.1, §5.10) and the
  next paint tints it; the host copies `selected_text()` with its own command and calls `clear_selection()`
  for a press the renderer does not get. `has_selection()` is `selected_text()` being non-empty, and lays
  the text out to say so; `may_have_selection()`, false when nothing is selected, does not, for a host
  that asks after every call. For touch, a host selects with its own UI (the message menu and
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
- `encode(list, caps, out, starts, most)` writes the same bytes and replaces `starts` with where each
  command starts and, last, where the list ends, counted from the list's first byte whatever `out`
  held before, as long as the list keeps within `most` bytes (no bound by default): for a longer list
  `starts` is left empty, and what it held while the list grew is given back. It leaves `starts`
  untouched when it returns false. The first command starts after the list's 6 bytes of form,
  version and count. A command's bytes are those the first commands alone
  encode to past the same start: a `ref` numbers the list's payloads from its first command, so what
  a command is written as depends on the commands before it.
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
and member come back bit for bit and shared as they were, and encode to the bytes above; the starts
an encoder notes put each command of every list sample, shared and repeated payloads included,
where the first commands alone encode it, after a prefix too, and a list a byte longer than the
bytes it is given gets none. The decoders refuse every cut of
every sample, seeded random changes they cannot take, and counts, lengths, references, enum values,
flags and mask bits past their bounds, leaving the output as it was; no decode asks the allocator
for more than a bound linear in its bytes.

## Two ends over a byte channel

`src/remote.h` (namespace `twlite::remote`) runs TwidgetCore with its platform in another process. A
renderer end (`src/renderer_end.h`) owns a `Renderer` whose `Backend` turns each call into a message,
and waits for the answer where the call returns one; it runs the platform's input and frame requests
and reports what the platform needs without being asked. A platform end (`src/platform_end.h`) takes
those messages, checks each one, calls the real `Backend` its host supplies, and gives the host the
decoded frames and reports. The messages are binary and versioned and carry the binary forms above.
twidget knows no transport: the embedder carries the bytes and implements the channel.

They build as two targets. `twidget::remote` is `src/remote.cpp` and `src/platform_end.cpp` on
`twidget::wire`: a program with the platform end links no litehtml, no renderer, no Qt and no platform
API, and of widget-sandbox it reads two headers, `twidget/json.h` and `twidget/gesture_policy.h`, for
the JSON and claim types, and links nothing. `twidget::renderer_end` is `src/renderer_end.cpp` on
`twidget::core` and `twidget::remote`.

The platform end trusts nothing that arrives. It refuses what an honest renderer end never sends, and
its decoders check structure, as the forms' do, not content: it calls the host's Backend with what a
request carries, and hands the host lists, batches and JSON whose ids, numbers, text and paint cost
nothing has looked at. The host checks those before its painter and its platform's services see them.

**The channel.**

```cpp
class Channel {
public:
    virtual ~Channel() = default;
    virtual void send(std::string message) = 0;
};

class RendererChannel : public Channel {
public:
    virtual bool request(std::string message, std::string& answer) = 0;
};
```

The platform end sends on a `Channel`, the renderer end on a `RendererChannel`. The embedder hands
the renderer end's one-way messages to `PlatformEnd::receive`, and its requests to
`PlatformEnd::answer`, whose `reply` it returns as the request's answer; when `answer` returns false,
the request gets none. It hands the platform end's messages to `RendererEnd::deliver`. It keeps these
rules:

- Messages each way arrive whole, once and in the order they were sent.
- A request comes after every message the renderer end sent before it: those reach `receive` before
  the request reaches `answer`. `request` blocks the renderer end's thread until the answer comes
  back, or returns false when none will (the channel closed, the embedder's own timeout). One request
  is out at a time.
- Platform messages reach `deliver` in order, and never while the renderer end is inside a call, a
  waiting request included.
- `receive` and `answer` are called in arrival order, one at a time. The host's callbacks run inside
  them and may call the platform end's input methods, which only send, but not `receive` or `answer`.
- Neither end's `send` calls back into the end that is sending.
- The channel, the Backend and the host outlive their end, and a `Clock` outlives the renderer it is
  given to. No callback destroys an end inside a call, or what it uses, `Host::refused` included: the
  embedder destroys it once the call has returned.
- The renderer end sends from its constructor (the first build's Backend calls, then the first
  `State`) and from its destructor (the canvases and fonts the renderer releases).
- `send` returns nothing: the embedder learns that its transport closed by itself, or when the next
  `request` returns false. The ends have no timeout and no thread of their own, and neither is
  thread-safe.
- `Caps::message` bounds every message either way. The embedder picks it to fit its transport with
  its framing: the default, 32 MiB, leaves 64 KiB of widget-sandbox's frames of 32 MiB and 64 KiB.

**Messages.** Every message is a `u8` kind, a `u8` version (1, `remote::VERSION`), its fields and
nothing after them. Kinds 1-8 are the platform's input and 32-36 its answers to the renderer's
requests 96-100, in the same order; 64-73 go from the renderer one way. `kind_of(message)` reads the
header alone: the kind when it is one of these at version 1, nothing otherwise; it never stands in for
decoding. A changed layout takes a new version or a new kind, and version 1's bytes keep their meaning.

The fields are those of the binary forms, plus:

| Field | Bytes |
|---|---|
| `f64` | a double's IEEE-754 bits as a `u64` |
| `value` | a `u32` length, then exactly one value of the binary forms, of the form named, in that many bytes |
| `state` | `f64` width, `i32` height, `flag` dirty, `flag` wants_tick, `f64` next_tick_in, `flag` frame_asked |
| `claim` | `flag` x, `flag` y, `flag` hold |

A `str` is capped by `Caps::text` unless the table says otherwise. Each message is a struct of the
same name in `src/remote.h`.

| Kind | Message | Way | Fields |
|---|---|---|---|
| 1 | `FrameRequest` | platform → renderer | `u64` seq, `flag` tick, `u8` paint: 0 `Skip`, 1 `IfDirty`, 2 `Force` |
| 2 | `SetWidth` | platform → renderer | `f64` width |
| 3 | `SetMaxHeight` | platform → renderer | `f64` height |
| 4 | `SetTheme` | platform → renderer | `flag` dark |
| 5 | `Touch` | platform → renderer | `i32` action, `f64` x, `f64` y, `i32` pointer, `flag` primary, `flag` mouse, `i32` modifiers, `i32` button |
| 6 | `Hover` | platform → renderer | `i32` action, `f64` x, `f64` y, `i32` modifiers |
| 7 | `ClearSelection` | platform → renderer | none |
| 8 | `ClaimQuery` | platform → renderer | `u64` seq, `f64` x, `f64` y |
| 32 | `FontAnswer` | platform → renderer, answers 96 | `i32` font, `value` (font metrics) |
| 33 | `WidthAnswer` | platform → renderer, answers 97 | `f32` width |
| 34 | `ImageAnswer` | platform → renderer, answers 98 | `flag` ok, `f32` w, `f32` h |
| 35 | `PixelsAnswer` | platform → renderer, answers 99 | `flag` ok; at 1 a `u32` length, exactly the request's w × h × 4 and at most `pixels`, then the RGBA bytes |
| 36 | `WidthsAnswer` | platform → renderer, answers 100 | `count` (cap `widths`, 4 bytes), exactly one for each text of the request, × `f32` width, in the request's order |
| 64 | `Frame` | renderer → platform, answers 1 | `u64` seq, `state`, `flag` painted; at 1 `value` (display list), `value` (damage) |
| 65 | `State` | renderer → platform | `state` |
| 66 | `Messages` | renderer → platform | `count` (cap `messages`, 4 bytes) × `str`, each a renderer message's JSON |
| 67 | `Pointer` | renderer → platform, answers 5 and 6 | `str` cursor, `flag` claimed; at 1 `claim` |
| 68 | `Selection` | renderer → platform | `str` text |
| 69 | `ClaimAnswer` | renderer → platform, answers 8 | `u64` seq, `claim` |
| 70 | `FontRelease` | renderer → platform | `i32` font |
| 71 | `CanvasRun` | renderer → platform | `i32` id, `i32` w, `i32` h, `value` (canvas batch) |
| 72 | `CanvasRelease` | renderer → platform | `i32` id |
| 73 | `FrameDelta` | renderer → platform, answers 1 | `u64` seq, `state`, `u64` base, `u32` length, `count` (6 bytes) × run, `value` (damage); a run is a `u8` tag, then at 0 (a copy) `u32` offset and `u32` length, at 1 (a literal) a `u32` length and that many bytes |
| 96 | `FontRequest` | renderer → platform, waits | `flag` canvas, `str` families, `f32` size, `i32` weight, `flag` italic; at canvas 0 `str` features |
| 97 | `WidthRequest` | renderer → platform, waits | `i32` font, `str` text |
| 98 | `ImageRequest` | renderer → platform, waits | `i32` id, `str` url (cap `url`) |
| 99 | `PixelsRequest` | renderer → platform, waits | `i32` id, `i32` x, `i32` y, `i32` w, `i32` h; w and h from 0 to `Canvas::MAX_SIDE` (8192), w × h at most `Canvas::MAX_PIXELS` (4096 × 4096) |
| 100 | `WidthsRequest` | renderer → platform, waits | `count` (cap `widths`, 8 bytes) × (`i32` font, `str` text); the texts together at most `widths_text` |

- `encode(message, caps, out)` appends the message to `out`. It returns false, `out` cut back to its
  size on entry, for what the decoder would refuse: a `str` or a count past its cap or past
  `UINT32_MAX`, a value `wire::encode` refuses, a `Paint` or a pixel size out of range, a
  WidthsRequest whose texts together pass `widths_text`, or a message past `Caps::message` (what
  `out` held before does not count). A field its flag leaves out must hold
  its default: an unpainted Frame with a list or damage, a canvas's FontRequest with features and a
  failed PixelsAnswer with pixels are refused rather than sent without them.
  `encode_canvas_run(id, w, h, ops, caps, out)` writes a CanvasRun without copying the batch, and
  `encode(frame, caps, out, most, list)` writes a Frame as `encode` does and tells, in a
  `WrittenList`, where its list lies in `out` and, while the list keeps within `most` bytes, where
  its commands start, as `wire::encode` notes them. `widths_in_request(texts, from, caps)` says how
  many texts from `from` on one WidthsRequest holds, by the rules `encode` applies: the counts,
  each text and their total, the message cap and the `u32` fields; the WidthsAnswer to a request
  that fits fits too.
- `decode(message, caps, out)` takes the one message all of `message` holds. It returns false and
  leaves `out` untouched for anything else: a message past `Caps::message`, another kind or version, a
  field cut short, a count or a length past its cap or past the bytes left, a `value` longer than the
  bytes left or that `wire::decode` does not take whole under `Caps::values`, a flag or a `Paint` out
  of range, a field present or missing against its flag, and bytes left over. A PixelsAnswer decodes
  against the request it answers, `decode(message, caps, asked, out)`, which must itself be in range;
  so does a WidthsAnswer, whose request must be within `widths`, `text` and `widths_text`, and which
  holds exactly one width for each of its texts. A WidthsRequest's count is checked against its cap and
  the bytes left before any room is made, and each text against `text` and against what is left of
  `widths_text` before it is copied.
  `decode(message, caps, frame, list)` also gives a Frame's list as a view of its bytes in
  `message`, empty for an unpainted Frame.
  `out` is replaced once the last byte has passed, and what a decoder allocates follows its bytes, as
  in the binary forms, `std::bad_alloc` included.

**Caps.** `remote::Caps` holds the caller's bounds:

| Cap | Bounds | Default |
|---|---|---|
| `values` | the lists, damage and batches inside messages: a `wire::Caps` | its defaults |
| `message` | the bytes of one message, either way | 32 MiB |
| `text` | the bytes of one `str` but a URL | 4 MiB |
| `url` | the bytes of an image URL | 8 MiB |
| `messages` | the texts of one Messages report | 1024 |
| `widths` | the texts of one WidthsRequest, and so the widths of its answer | 256 |
| `widths_text` | the bytes of a WidthsRequest's texts together | 16 KiB |
| `frame` | the bytes of one Frame, and of the Frame a FrameDelta stands for | `SIZE_MAX`: `message` alone |
| `delta_base` | the bytes of the painted list each end keeps for a FrameDelta to copy from, and of a list a FrameDelta rebuilds | 256 KiB; 0 sends every painted Frame whole |

At two caps the renderer gets what an in-process backend that refuses would give it; past any other,
the end that would send stops:

- an image URL longer than `url` is a failed load, with no request;
- a PixelsRequest whose w × h × 4 bytes pass `values.pixels`, or whose answer, 7 bytes and the
  pixels, would pass `message`, is answered not ok without a `canvas_read`, and the renderer fills
  the `imageData` with zeroes as for any failed read. With the default caps that is a read of more
  than 8388606 pixels.

The widths caps bound how much one request makes the platform measure: `widths` the strings, which
cost a call each however short, and `widths_text` their bytes. They change how many requests carry a
layout's words, never what is measured: a text that no WidthsRequest can hold alone goes as a
WidthRequest, under the `text` cap as ever, and with `widths` at 0 every word does. The renderer end
cuts its requests where `widths_in_request` says, so no request it sends is one the codec refuses.

**Frames: the host drives the clock.** The renderer end never ticks or paints by itself.
`PlatformEnd::request_frame(tick, paint)` sends a FrameRequest and returns its seq, counted from 1.
The renderer end calls `tick()` when `tick` is set, then paints when `paint` is `Force`, or `IfDirty`
and the renderer is dirty; `Skip` never paints. It answers every FrameRequest with one Frame of the
same seq: painted, with the list and its damage (nullopt: paint everything), or not, when nothing
changed or the host said `Skip`. A host that asks for no frame gets none: ops, input and the engine's
frame requests leave the work unpainted until it asks, and a widget the host does not ask costs
nothing. State's `dirty`, `wants_tick` and `next_tick_in` tell the host when to ask, as in "A clock"
above.

The engine's own frame request (`requestFrame` in widget-sandbox's protocol, which a host answers with
a `frame` message) goes to `RendererEnd::want_frame()`. It sets State's `frame_asked`, reported at
once, so that the platform learns to ask. The next FrameRequest that ticks takes it, and that call's
`Output::frame` tells the helper that the engine's frame message is due after the call's messages. A
FrameRequest that does not tick leaves it set.

**Frames as deltas.** A painted frame may cross as a FrameDelta instead of a Frame: the same seq,
state and damage, and its list as runs that copy byte ranges of the list of the last painted frame,
its base, or carry bytes of their own. Both ends keep that list, as encoded, while it is no longer
than `delta_base`: a painted frame, whole or as a delta, replaces it, or drops it when its list is
longer, an unpainted one keeps it, and a stop, a refusal or the platform end's `close()` clears it.
Both ends take the same caps, `delta_base` among them, so they keep the same list. The renderer end
writes each painted Frame once, straight into the message it sends, noting where its list's commands
start while the list keeps within `delta_base`, and keeps a copy of a list that does. Without a kept
list, or with a list longer than `delta_base`, the frame goes whole as written. Otherwise each
command is looked for in the kept list: the one after the last copied, then up to four of those with
the same hash; only bytes found equal are copied, and the head of form, version and count is one more
such range. Of the matches, it copies
those that make the runs shortest, no more bytes in all than the kept list holds, and sends the
FrameDelta only when its message is shorter than the whole Frame's; a frame whose runs would copy
nothing goes whole. Copies are byte ranges of the encoding, not commands: a `ref` to a shared
payload is copied as the number it is, which means the same payload only because the bytes before
it are the same too.

`rebuild(message, caps, base_seq, base, frame, list)` gives the Frame a FrameDelta stands for and the
bytes of its list. Before it makes room for the list it checks the whole message: its base is
`base_seq` and `base` no longer than `delta_base`; the length is from 1 to `delta_base`; there are
at most one run more than `values.commands` (a head and a run for each command), no more than the
length nor than the bytes left hold at 6 bytes each; every run has a known tag and at least one
byte, each copy lies within the base and all copies together take no more bytes than the base
holds, so that a small range copied again and again cannot multiply what the bytes allow; each
literal is within the bytes left; the runs add up to the length exactly, by sums that cannot wrap;
the damage's length is within the bytes left and nothing follows it; and the Frame it stands for,
with its list of that length and that damage, fits `frame` and `message`, as the message itself
does. All of that reads the runs and the damage where they lie, making nothing. Only then does it
decode the damage, copy the runs into a list of exactly that length and decode the list, as a
Frame's, under `values`. `decode(message, caps, out)` takes a FrameDelta's fields as they are, its
copies checked against a base of `delta_base` bytes, with the same checks before it makes the runs,
their literals or the damage, and `delta_ids(message, seq, base)` reads its seq and its base alone,
which never stands in for decoding. Beside what decoding the Frame takes, the platform end
then holds the message, the kept list and the rebuilt one, which becomes the next kept list: at most
two `delta_base` beyond the message, as when a whole Frame replaces the kept list with a copy of its
own.

**What is reported unasked.** Each call into the renderer end ends with its reports, in this order,
each only when there is one:

1. `Messages`: the call's renderer messages whose `type` is `size`, `gesture`, `limit`, `violation`
   or `bubble`, as their JSON; `is_platform_message(type)` names the five for both ends. The others,
   `event`, `result` and `resize` among them, are the engine's: `receive` and `deliver` hand the
   helper every message of the call, those five included.
2. `Pointer`, after a Touch or a Hover: `cursor()` there, and the claim (`x`, `y`, `hold`) of the
   call's `gesture` message, which a primary press reports.
3. `Selection`, when the selected text differs from the text reported last (at first empty): empty
   means no selection, as `has_selection()` says. The text is laid out only when
   `may_have_selection()`.
4. The `Frame` that answers a FrameRequest, or the `ClaimAnswer` that answers a ClaimQuery with
   `claim_at(x, y)`: whether a hold there would be claimed, without pressing.
5. `State`, when it differs from the one reported last (doubles by their bits) and no Frame carried
   it. The constructor sends the first.

State is `width()` and `viewport_height()`, the widget's CSS size (the height the document converged
to, kept within the minimum and the maximum height), `dirty()`, `wants_tick()`, `next_tick_in()`
(seconds, infinity when nothing is scheduled) and `frame_asked`. The document's own height is the
`size` message's `height`. Time passing alone reports nothing: State is read at the end of a call.

**The platform end.** `PlatformEnd(backend, host, channel, caps)` sends its host's input with
`request_frame`, `set_width`, `set_max_height`, `set_theme`, `touch`, `hover`, `clear_selection`
and `ask_claim`, which returns its seq too, and takes the renderer end's messages in `receive` (kinds
64-73) and its requests in `answer` (96-100):

- FontRelease, CanvasRun and CanvasRelease call `delete_font`, `canvas_run` and `canvas_release` at
  once, in the order they arrive, so they keep their place among the frames. The host paints a frame,
  or keeps what its list names, before it hands the platform end later messages: a release after the
  frame may free a font or a canvas the list draws with.
- A FontRequest calls `create_canvas_font` when it is a canvas's and `create_font` otherwise, a
  WidthRequest `text_width`, an ImageRequest `load_image` with `w` and `h` at 0, a PixelsRequest
  `canvas_read` (past the caps, as above), each out-parameter default-constructed as at the renderer's
  call sites. Pixels other than exactly w × h × 4 bytes are answered not ok. A WidthsRequest, checked
  whole first, calls `text_widths` once with all its texts on the thread that answers, and a Backend
  that gives another number of widths than texts stops the end. The answer replaces `reply` only once
  it is made.
- Frame, State, Messages, Pointer, Selection and ClaimAnswer go to the host's `frame`, `state`,
  `messages`, `pointer`, `selection` and `claim`, and a FrameDelta, once rebuilt on the kept list,
  to `frame` as the Frame it stands for. `state()` is the last State, a Frame's included, updated
  before the callback. Each Messages text is parsed with `Json::parse(text, 4)` and must be an object
  whose `type` is one of the five.

It expects one answer for each `touch`, `hover`, `request_frame` and `ask_claim`, in the order it sent
them: a Pointer, a Frame or a FrameDelta with the request's seq, a ClaimAnswer with the query's.
State, Messages, Selection and the one-way Backend calls may come at any time. It decodes and checks
a whole message, every JSON text of a report included, before the Backend or the host sees any of
it, and refuses:

- a Frame, FrameDelta, Pointer or ClaimAnswer that is not the next answer it expects: unsolicited,
  repeated, out of order or for another seq, a FrameDelta's checked before it is rebuilt;
- a FrameDelta with no kept list or another base than the last painted frame, or that `rebuild`
  refuses, the list it makes included;
- a request in `receive`, a one-way message in `answer`, and a kind of its own direction in either;
- a message of no kind it knows or of another version, one that does not decode, and a report that
  is not one of the five;
- an input or an answer that does not encode within its caps.

A whole Frame's request is taken only once the copy of its list the end keeps is made, so a frame
whose copy fails to allocate leaves the end as it was, and the same frame given again is taken.

A refusal is final: `refusing()` turns true, `Host::refused` gets the reason once, and from then on
`receive` and `answer` return false, `request_frame` and `ask_claim` 0, and nothing touches the
Backend, the host or the channel. The host stops the widget; the request being answered gets no
answer. `close()` stops the end the same way without a word to the host, for an embedder that ends
the route itself: it lets go of the answers it expected and of the kept list, and closing a stopped
end again changes nothing. Closing allocates nothing and never throws, so an embedder may close from
a destructor. A refusal closes the end this way once it has its reason, before it calls
`Host::refused`. Out of memory, a call may throw `std::bad_alloc` before any refusal starts, while
it decodes, encodes or makes the reason: the end is then still open, holds the answers it expected
and its kept list, and has told the host nothing. An embedder that catches that failure may call
`close()` to let go of them.

**The renderer end.** `RendererEnd(width, dpr, dark, channel, caps, clock)` makes its `Renderer`, with
the clock when one is given, on a Backend that sends:

- `create_font` and `create_canvas_font` as a FontRequest, the second marked as a canvas's,
  `text_width` as a WidthRequest, `text_widths` as WidthsRequests, `load_image` as an ImageRequest
  (`w` and `h` from the answer whether it is ok or not), `canvas_read` as a PixelsRequest, each
  waiting for its answer. Each `text_width` is a round trip; a face asks once per word, as in process.
  `text_widths` makes no request for no texts, and otherwise puts the texts in order into as few
  WidthsRequests as `widths`, `widths_text`, `text` and `message` let each request and its answer
  hold; a text that fits in none alone goes as a WidthRequest. After a failure the texts left get
  width 0 with no request.
- `delete_font`, `canvas_run` and `canvas_release` one way, in the order the renderer makes them.

**A layout's words.** litehtml measures each text in its style pass. A container that returns true
from the fork's `document_container::measures_text_later()` lets a text owe its width instead: the
text keeps its font, transform, height and line-break rules from the style pass, and whatever reads
its size first (the layout, or a patch comparing a hole's sizes, through `get_content_size`)
measures it then through `text_width`, exactly as the style pass would have. The default is false,
so every other container measures as it always did. TwidgetCore's container opts in, and right
before each layout its walk of the document takes the words the texts owe that no face has cached,
in document order, each once per face and no more than each face's cache keeps (texts past the
cache's key size are left out), and measures them in one `text_widths` call. The widths go into the
faces' caches by the same rules as one measured alone, and every text then takes its width from its
face, letter and word spacing added, before the layout starts; what the call left out is measured
one by one. The same call takes the units a paint draws one at a time, for each text the walk finds
outside a `display:none` box whose font has letter spacing (its characters) or word spacing and a
space (its words and spaces), so the paint that follows measures none of them. The walk also keeps,
for the layout alone, the texts outside a `display:none` box it may break, by the two rules
litehtml's own layout applies: those its inline layout may cut where a line ends
(`el_text::breaks_in_line()`: an `overflow-wrap` or `word-break` other than normal, `white-space`
neither `nowrap` nor `pre`, no space) and those whose characters set the narrowest a box sized to its
content can be (`breaks_at_narrowest()`: `overflow-wrap: anywhere`, preserved spaces included). The
first character a `TextBreaker`
measures that its face has not cached, and that the face would keep, asks, in one more
`text_widths` call by the same rules, for every break point of all of them, the characters a word
is broken into when its box is measured at its narrowest. A break's longer pieces, where a line ends
inside a word, are measured one by one and ask for nothing more, and a layout makes that call once at
most. `LayoutWidths` (`src/text_widths.*`) does this for the container. No text pointer or font handle
outlives the layout. A renderer used directly makes these calls on its own Backend, and a renderer
end sends them as WidthsRequests, so both make the same Backend calls in the same order: a build's
fonts in the style pass, then its words and spaced units, then the breaks its layout measures.

It answers each FrameRequest with a Frame or, painted, a FrameDelta, as "Frames as deltas" above
says.

Its embedder, the process that runs the engine, calls:

- `receive(message)` with each engine message for the renderer, and gets the renderer's messages back;
- `want_frame()` for the engine's frame request;
- `deliver(message, out)` with each platform message; `out` is made afresh by each call: the call's
  renderer messages and `frame`;
- `renderer()` for what the channel does not carry (the editor, compositing, `trim`, `wheel`, `key`,
  `image_ready`, `stats`), each call followed by `report(the messages it returned)` so that its reports
  go out. Only while the end is not stopped, and never `tick()` or `paint()`, which only a
  FrameRequest runs. Compositor layers stay on this side: no message carries them. A canvas font
  hook set through `renderer()` runs in this process, and the Backend it gets is the end's: the
  fonts it makes and releases there travel as FontRequests and FontReleases, as the renderer's own
  do, and only while the end lives.

The first failure stops it for good, with `stopped()` and `stop_reason()` for its embedder: a request
that got no answer, an answer that does not decode, a message or a request past the caps, and a
platform message that does not decode or is not platform input. The call in progress unwinds on
neutral Backend answers (font 0 with the metrics untouched, width 0, false) without the channel.
From then on nothing is sent: `deliver` returns false with `out` empty, no partial messages and no
engine frame, `receive` returns nothing, and the destructor sends nothing.

`twidget_remote_tests` (ctest `twidget_remote`) links `twidget::remote` alone. Samples of all 28
kinds come back member by member, and some encode to bytes written out by hand; the decoders refuse
every cut, every other kind and version, seeded changes they cannot take, counts and lengths past
their bounds, and optional fields, pixel sizes and widths counts against their flags and requests,
leaving the output as it was and asking the allocator for no more than a bound linear in their bytes.
FrameDeltas between lists that share payloads, gain and lose their users, start or end empty, rebuild their
Frames member by member and their lists' bytes; every cut, another base and each check of `rebuild`
at its bound and one past refuse, the outputs left alone and, for each check of the runs and for a
delta with a literal and damage whose Frame passes the frame cap, nothing asked of the allocator. A
painted Frame written with its list's place is the Frame's own bytes, and every prefix
`widths_in_request` gives is what `encode` takes, under counts, text and message caps at every
size. A platform-end session against a recording Backend makes every input, request, widths batch,
one-way call and report, deltas whole → delta → unpainted → delta among them, and each refusal
above with its reason, a widths request past its caps and a Backend's wrong number of widths among
them; a closed end refuses quietly, and a whole frame whose kept copy fails to allocate stays due
and is taken when given again. `twidget_ends_tests` (ctest `twidget_ends`) runs a renderer called directly and the two ends
over an in-memory channel (`tests/loopback.h`) with the same Backend behaviour and equal clocks, and
compares, call by call, their messages, decoded lists, damage and batches, the Backend calls in order
from construction to destruction, cursors, claims, hold answers, selections and states; it stops the
renderer end at each kind of failure, a widths request's among them. It also checks a layout's words:
one widths request per build after its fonts, requests and answers within every cap with the words in
order, sizes read before a layout, transforms, spacing, words broken in the layout, more words than a
face's cache keeps, and a plain litehtml container that measures in the style pass as before; the
units of spaced runs in the build's request, none of a hidden run and none measured by the paint; the
breaks of a layout in one more request, those of hidden texts left out, a relayout and a later layout
asking for nothing or for their new characters alone, two boxes sharing the request, the breaks under
small caps, and that request failing every way; a warm relayout's fit probes longer than the cache
keeps asking for no breaks, a preserved tab's space in the breaks' request of a box at its
narrowest, and a warm relayout's breaks making no block for each of a hundred more preserved
tabs. Animations there cross as deltas, whole when nothing is worth copying, a delta would be
no shorter or a list passes `delta_base`; a delta changed on its way, or sent to another route,
stops that platform end with its reason before the host or the Backend hears of it. Watching the
heap, it checks that a paint with deltas off or past `delta_base` sends the ordinary Frame's bytes,
written once, that after a paint the end keeps the list, its bounds and its index and nothing more,
and that a platform end gives its kept list back when closed, asking for no memory, or when a
message does not decode, telling its host once; where a thread can start, another thread's blocks
pass the watch by.

The parity checks compare the two ends with renderers used directly over real workloads, recorded in
one process and replayed in another. `RecordingRenderer` (`tests/recording_renderer.h`) is a
`Renderer` that hides each of its non-const public calls: every call, construction and destruction
included, is a step of the process's one recording (`tests/recording.h`), written as it ends, so the
file holds every live renderer's steps in the order the process made them. A step holds the call's
arguments (a received message in a lossless binary JSON form), the size of litehtml's process-wide
name table at its start, each Backend call the renderer made with what it had put in the outputs and
what the source's own Backend answered, each clock read (steady time in whole microseconds, through
the renderer's `Clock`), and what the renderer gave and showed: its messages, the cursor after a
pointer call, the claim of a `claim_at`, the list and damage of a paint, then the reads
`RendererEnd::finish()` makes, in its order, for the selection and the state. Those observations are
the expected values. The file is binary, with lists, damage and batches in their binary forms, and
ends with a record of its counts and of the recorder's first error; `twidget_replay` refuses a file
that is missing, cut short, without that record, with bytes after it, with counts that disagree or
with an error. In a fresh process it runs only the two ends: a `Loopback` (`tests/loopback.h`) for
each recorded renderer, opened at its construction and closed at its destruction, so that as many
are alive as in the source and the name table grows as it did. Each step goes through the platform
end or, for a call the channel does not carry, to the renderer end's `renderer()` and then
`report()`. The platform end's Backend answers from the step's recorded calls in order and the
renderer's clock gives the step's recorded reads: a call of another kind, a call or a read past them,
or one left unused fails the step. What the host heard is compared with `twidget_ends_tests`' own
code (`tests/reports.h`): the messages, the reports in their order, lists, damage and batches decoded
member by member with their float bits and sharing, the Backend calls in order with their inputs and
answers, cursors, claims, hold answers, selections and states, and the name table's size; the first
difference stops the run, naming the step and the member. `Command::key` is compared through a
correspondence: litehtml mixes heap addresses into an SVG paint-cache key, so the keys differ from
process to process, and within each session zero must meet zero and each recorded key always the
same replayed key and back; while that holds, the replayed keys are mapped to the recorded ones and
compared as every other member. Each source prints how many sessions, steps, lists, Backend calls of
each kind (canvas batches and pixel reads among them), pointer reports, claims, hold queries,
selection changes, ticks, engine frames, resizes, theme changes and keyed commands it compared, in
all and per labelled session, and `--needs` requires those it can reach to be above zero.
`tests/parity.cmake` then replays it perturbed, and each perturbation must pass every earlier step
and fail at the changed one on that member alone: the first painted list's first `rect.x` one float
up, a byte added to the text of the first recorded `text_width`, and, for a source with keyed
commands, one replayed key changed to split a repeated key or to merge two. The sources, each a
ctest:

- `twidget_parity_core`: `tests/core_tests.cpp`, unchanged, compiled with `Renderer` defined as
  `RecordingRenderer` (`twidget_core_record`, `tests/core_record.cpp`). It is recorded at test time
  into the build tree, about 475 MB, and removed after a passing check. Its own checks are printed,
  not gated, as `twidget_core` gates them; a recording that fails does gate. Its canvas-limit
  sessions send batches past the 32 MiB message cap, so its replay raises `Caps::message` to 256 MiB.
  Its canvas font hook check and its check of two renderers on two threads at once are left out
  there (`#ifndef Renderer`). A hook is the embedder's code, which a recording does not carry, so the
  replayed renderer, with none, would make other Backend calls; and a recording takes one step at a
  time, so the second renderer would wait for the first, which waits for it. Only `twidget_core`
  runs them.
- `twidget_parity_bench`: `tests/bench.cpp`, unchanged, in its seven modes (mixed, transform, swarm,
  css, drag and drag-doc at 60 elements, canvas at 50, 10 frames each), one labelled session each
  (`twidget_bench_record`, `tests/bench_record.cpp`), recorded at test time the same way.
- `twidget_parity_corpus`: `tests/recordings/corpus.twrec`, committed, which `tools/record` (see
  "Building") recorded from four widget-sandbox corpus widgets run for real:
  `compat/canvas-line-chart.html` (canvas batches, claims), `gestures/1-pairs/01-joystick-xy.html`
  (a claim that holds, the engine's frame requests, animation), `compat/dark-mode.html` (the theme)
  and `htmlbubbles-ported/03-stress-test/13-fuzz.html` (the corpus's only image). Its replay needs
  no engine. Its origin names the widgets, twidget's and widget-sandbox's versions as `git describe`
  gave them, the recorder, its backend and toolchain, the host's options and the script. It is
  checked on macOS only: `~Renderer` releases canvases and canvas fonts in the order of its hash
  tables, libc++'s in the recording. `parity.cmake` fails it when it is missing or larger than 398615
  bytes (`-DMAX_BYTES`), the recording's 318892 plus a quarter; re-recordings vary in size with
  timing.

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
(the binary forms, on the standard library alone), `twidget::remote` (the two ends' messages and the
platform end, on `twidget::wire`), `twidget::renderer_end` (the renderer end, on `twidget::core`) and
the platform's backend; Android and Emscripten builds go through their toolchain files. With `TWIDGET_BUILD_TESTS`
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

`tools/record` makes the corpus recording of `twidget_parity_corpus`, macOS only. Each widget runs
for real: widget-sandbox's desktop `WidgetHost` with QuickJS, the validator, its `WidgetJS` bundles
and `BasicIntl`, strict with `webAnimations`, at width 320, DPR 2, locale `en` and light, over a
`RecordingRenderer` on `CGBackend`. The adapter between them is shaped as `tools/conformance`'s: one
lock around every core call, messages to the host after it is released, and a 16 ms frame loop that
ticks when the engine asked, the renderer wants a tick or its wake is due, and paints when dirty.
The script is fixed, and the origin carries its numbers: load, 300 ms of frames, `claim_at` over a
grid, a press at the first point that claims with six moves and a release, six hovers and a leave,
width 240 then 320, theme dark then light, destruction. A widget that does not load, render or paint,
or that throws or dies other than by the recorder's own shutdown, ends the recording with that error,
which the replay refuses; that is decided once the host is destroyed, which delivers the engine's
last reports. The project builds widget-sandbox's `packages/desktop`, found as twidget's own build
finds it, with its QuickJS submodule.

```sh
cmake -S tools/record -B build-record -G Ninja -DCMAKE_BUILD_TYPE=Debug && cmake --build build-record
build-record/twidget-record --out tests/recordings/corpus.twrec compat/canvas-line-chart.html \
    gestures/1-pairs/01-joystick-xy.html compat/dark-mode.html htmlbubbles-ported/03-stress-test/13-fuzz.html
```

The committed recording holds what the renderer gave when it was made, so it is made again whenever
that changes: a change to the recording's format, which takes a new version in `tests/recording.h`
(the replay refuses any other), and a change to the binary forms or to the renderer that changes what
it gives these widgets, the Backend calls it makes for them or the clock reads it takes. The new
recording keeps the four widgets and passes the Debug and the sanitizer replay, and `TW_CORPUS_BYTES`
in `CMakeLists.txt`, the cap `parity.cmake` checks, becomes its measured size plus a quarter, which
stays within 512 KiB.

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
