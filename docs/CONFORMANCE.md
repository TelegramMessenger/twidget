# Conformance with widget-sandbox v0.1.0

widget-sandbox v0.1.0 freezes widget profile 1 and ships a conformance set that every client runs on its
renderer (`packages/conformance/CLIENTS.md` there). This page tracks TwidgetCore against it: what the
contract asks of the renderer, and what the set reports, item by item.

Measured with `tools/conformance` (Windows, the Direct2D painter on WARP, 390 CSS px, DPR 2) and the
set's `tools/report.mjs`. The verdicts that come from the widgets' own checks (layout, computed style,
events) hold for TwidgetCore on every platform; the pixel stages judge the Direct2D painter only, and
each platform's painter needs its own run.

## Where it stands

| Date | Commit | In profile (173): pass / partly / fail / untested | Out of profile (82): neutralised / not yet / fail |
|---|---|---|---|
| 2026-10-18 | f649455 (contract) | 90 / 49 / 24 / 10 | 69 / 3 / 8 |
| 2026-10-18 | 3420fc5 (computed style) | 111 / 38 / 14 / 10 | 73 / 2 / 5 |
| 2026-10-18 | 099f3cb (inline boxes) | 120 / 32 / 11 / 10 | 73 / 2 / 5 |
| 2026-10-18 | image load and error (E1) | 121 / 32 / 10 / 10 | 73 / 2 / 5 |
| 2026-10-19 | the small fixes (Q2, E5, E6, L5, L10, L12, O1–O3, sans-serif) | 129 / 26 / 8 / 10 | 79 / 1 / 0 |
| 2026-10-19 | events and animations (E2–E4, A1, A2, L7) | 135 / 21 / 7 / 10 | 79 / 1 / 0 |
| 2026-10-19 | layout and style in the fork (L1–L13, O4) | 148 / 9 / 6 / 10 | 80 / 0 / 0 |
| 2026-10-19 | SVG queries and painting (Q3, P1–P9) | 158 / 5 / 0 / 10 | 80 / 0 / 0 |

What is left partly passing is not TwidgetCore's to fix: see the last section.

The 10 untested features are the host's (limits, restart, storage, gesture claims, message edits, size
hints): the automatic set does not reach them, and each client checks them in its own host.

For comparison, the old litehtml renderer copy in widget-sandbox (web build, upstream litehtml) is at
66 / 41 / 52 in widget-sandbox's own results.

## Done

| What v0.1.0 asks | Where |
|---|---|
| Real input marked `trusted: true`, or the hosts count no user gesture | `Renderer::event_message` |
| Only profile 1's events: no keys, focus, input, dblclick, contextmenu, touch, wheel or scroll | `PROFILE_EVENTS`, `listened` |
| The `theme` message: `--tg-theme-*`, `--tg-color-scheme` and `--tg-button-*` on the root, important, values checked | `css::theme_variables` |
| The profile's `<button>` template in the UA sheet | `UA_EXTRA` |
| `gesture` claims with `x` and `y` | `Renderer::gesture_claim` |
| The shared height bounds, 50 to 600 CSS px | `MIN_HEIGHT`, `max_height_` |
| `getComputedStyle` answered for every property the fork computes, as Chromium writes it | `computed_style.cpp` |
| An inline element's rect is its line boxes, with a rect per line | `inline_boxes_of` |
| `<img>` sends `load` with its natural size, or `error` | `Renderer::load_images` |
| `scrollWidth` and `scrollHeight` reach the overflowing content | `content_extent` |
| `pointer-events: none` passes a hit to what is below | `Container::hittable` |
| Pointer and mouse events carry `movementX` and `movementY` | `Renderer::emit_event`, `dispatch` |
| `initial`, `unset` and `revert` | the fork's `css_wide_keyword`, `html_tag::ua_property` |
| Every CSS colour name, `rebeccapurple` included | the fork's `web_color.cpp` |
| A refused image is widget-sandbox's 48 px placeholder | `Container::draw_placeholder` |
| A canvas gradient with no length (the validator's conic gradient or pattern) paints nothing | `Canvas::paint_of` |
| On Windows, `sans-serif` and Helvetica are Arial, as in browsers | `D2DBackend::resolve_family` |
| `getComputedStyle` reads running, paused and seeked animations | `Animator::peek` in the `cs` query |
| `animationstart`, `animationend` and `transitionend` go out before the widget's `listen` | `ALWAYS_FORWARD` |
| ResizeObserver: `resize` on observe and on every change | `Renderer::report_resizes` |
| Tapping a `<summary>` opens and closes its `<details>` and sends `toggle`; a closed one shows only its summary, with a disclosure marker | `Renderer::toggle_details`, `UA_EXTRA`, the fork's `list_style_type_disclosure_*` |
| `calc()`, `min()`, `max()`, `clamp()` mixing percentages with lengths | the fork's `css_units_calc`, `calc_evaluate` |
| `width: fit-content`, `min-content`, `max-content` on blocks | the fork's `render_block` |
| Empty auto table cells share the spare width | the fork's `table_grid::calc_table_width` |
| `word-break` (`break-all`, `break-word`) | the fork's `css_properties::get_wrapping` |
| `open-quote` / `close-quote` from `quotes`, and `<q>` marks | the fork's `el_before_after_base::add_quote`, `UA_EXTRA` |
| `direction` and `dir` (`auto` too): right to left lines, alignment, blocks, and a simplified bidi that keeps embedded runs in their order | the fork's `line_box::reorder_bidi`, `render_block_context` |
| `:lang()` on the nearest `lang`, `:empty` | the fork's `html_tag::select_pseudoclass` |
| The `translate`, `rotate` and `scale` properties | the fork's `css_properties::compute_transform` |
| `letter-spacing` and `word-spacing` | `Container` (`spacing_units`), the fork's `font_description` |
| SVG: `getComputedTextLength`, text `getBBox`, a shape's or text's rect through the view box, `<use>` of a `<symbol>`, `<tspan>` with its own fill and shifts | the fork's `svg_ctm`, `svg_text_runs`, `el_svg` painter; `svg_box_of` |
| Dashed and dotted borders and outlines, mitred corners between sides that differ | `Container::draw_sides` |
| `background-repeat` | `Container::draw_image` |
| `object-fit`, `object-position` | the fork's `el_image::fitted` |
| Gradient stop positions fixed up as CSS does (an explicit `0` is a position) | the fork's `prepare_color_points` |
| `text-overflow: ellipsis` | the fork's `render_item_inline_context::apply_ellipsis` |
| `filter` and `backdrop-filter`: blur, brightness, contrast, grayscale, hue-rotate, invert, opacity, saturate, sepia, drop-shadow | `FilterPush`, `Backdrop` in the display list; `D2DBackend::filtered` |
| A canvas drawn from another canvas that the same batch draws later | `Renderer::apply_ops` |

## Each platform's painter

The pixel stages above judge the Direct2D painter. These display list commands are new to the other
painters, which still draw what they wrap as it is:

| Command | CoreGraphics | Android | Web | Qt |
|---|---|---|---|---|
| `FilterPush` / `FilterPop` (filter), `Backdrop` (backdrop-filter) | to do (passed through) | to do (ignored) | to do (ignored) | to do (ignored) |

Dashed and dotted borders are `Path` strokes with dashes, which every painter draws for SVG already.

## Not TwidgetCore's

| Item | Where |
|---|---|
| `Intl`: number grouping, dates, plural rules, relative time, collation | the C++ desktop host's `IntlProvider` (`BasicIntl`); a Windows client needs ICU or Windows.Globalization behind it |
| `innerHTML` replacing children, live `getElementsByClassName` | widget-sandbox's shim (its `docs/changes.md` lists the second as known) |
| The `background` stage (`css.background.basic`) | its 20 px PNG is corrupt: a bad IDAT CRC and zlib checksum, and row 5 has filter byte 51. Chromium stops there and leaves the rows below transparent, WIC decodes them as colours. The tiling is right |
| The `emoji` stage (`css.text.emoji`) | the reference is Apple Color Emoji; Windows draws Segoe UI Emoji |
| A Web Animations seek seen by a layout query in the same task (`anim.waapi`) | widget-sandbox's `query()` flushes the DOM ops but not the animation records, which go out at the end of the task; its own renderers fail this check too |
| Right click / long press | the runner has no secondary button; profile 1 forwards no `contextmenu` anyway |
