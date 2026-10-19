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

The 10 untested features are the host's (limits, restart, storage, gesture claims, message edits, size
hints): the automatic set does not reach them, and each client checks them in its own host.

For comparison, the old litehtml renderer copy in widget-sandbox (web build, upstream litehtml) is at
66 / 41 / 52 in widget-sandbox's own results.

## The renderer contract (done)

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

## To do

Each item names the set's checks it fails (catalogue id: check). "First suspect" is where to look,
not a diagnosis.

### Queries the renderer answers

| # | Item | Fails | First suspect |
|---|---|---|---|
| Q3 | **SVG queries:** `getComputedTextLength`, and viewBox scaling seen through `getCTM`/rects | `svg.text`, `svg.structure` | the `svg` query |

### Painting (Direct2D; each painter to check on its platform)

| # | Item | Stage |
|---|---|---|
| P1 | Background images tile (repeat, size, position) | `css.background.basic` |
| P2 | Borders, outline | `css.box.border`, `css.box.outline` |
| P3 | `object-fit` | `css.box.object-fit` |
| P4 | Conic gradients | `css.gradient.conic` |
| P5 | `filter` (blur, brightness, drop-shadow) and `backdrop-filter` | `css.filter.blur-brightness-drop-shadow`, `css.backdrop-filter` |
| P7 | `text-overflow: ellipsis` | `css.text.overflow` |
| P8 | Colour emoji | `css.text.emoji` |
| P9 | Canvas `drawImage` | `canvas.draw-image` |

### Not TwidgetCore's

| Item | Where |
|---|---|
| `Intl`: number grouping, dates, plural rules, relative time, collation | the C++ desktop host's `IntlProvider` (`BasicIntl`); a Windows client needs ICU or Windows.Globalization behind it |
| `innerHTML` replacing children, live `getElementsByClassName` | widget-sandbox's shim (its `docs/changes.md` lists the second as known) |
| A Web Animations seek seen by a layout query in the same task (`anim.waapi`) | widget-sandbox's `query()` flushes the DOM ops but not the animation records, which go out at the end of the task; its own renderers fail this check too |
| Right click / long press | the runner has no secondary button; profile 1 forwards no `contextmenu` anyway |
