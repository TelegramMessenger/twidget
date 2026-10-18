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

## To do

Each item names the set's checks it fails (catalogue id: check). "First suspect" is where to look,
not a diagnosis.

### Queries the renderer answers

| # | Item | Fails | First suspect |
|---|---|---|---|
| Q2 | **scrollHeight** of overflowing content | `js.layout.box` | `box` reports the client height |
| Q3 | **SVG queries:** `getComputedTextLength`, and viewBox scaling seen through `getCTM`/rects | `svg.text`, `svg.structure` | the `svg` query |

### Events

| # | Item | Fails | First suspect |
|---|---|---|---|
| E2 | **`animationstart`** | `anim.events` | the animation events reach the widget only with the next host message |
| E3 | **ResizeObserver** (`resize` to the host) | `js.layout.resize-observer` | `observeResize` |
| E4 | **Tapping `<summary>` toggles its `<details>`** (and `toggle`) | `html.el.details` | the click's default action |
| E5 | **movementX/Y** on pointer moves | `input.drag` | `event_json` |
| E6 | **`pointer-events: none`** lets taps through | `css.pointer-events` | `widget_id_at` |

### Animations

| # | Item | Fails | First suspect |
|---|---|---|---|
| A1 | A paused CSS animation, or one with a negative delay, shows its frame | `anim.keyframes`, `anim.play-state` | sampling paused animations |
| A2 | A Web Animations seek shows in the computed transform and in a layout query in the same task | `anim.waapi`, `anim.waapi-css-control` | `anim` records applied after the query |

### Layout and style in the fork

| # | Item | Fails |
|---|---|---|
| L1 | `calc()`, `min()`, `max()`, `clamp()` with percentages | `css.value.math` |
| L2 | The `ch` unit; `width: fit-content` | `css.value.units`, `css.box.size` |
| L3 | Floats push inline text aside; table cells share the row | `css.box.float`, `css.display.table` |
| L4 | `word-break: break-all` | `css.text.wrap` |
| L5 | CSS-wide keywords: `initial`, `unset`, `revert` | `css.value.keywords` |
| L6 | Generated content: `::before` boxes, `attr()`, counters, `quotes`, `<q>` marks | `css.value.content`, `html.el.inline-text`, `behaviour.policy-strict` (attr of data-*) |
| L7 | `<details>` closed is as tall as its summary; `<summary>` is a list item | `html.el.details` |
| L8 | `dir=rtl` lays inline boxes right to left; `text-align: end` in RTL | `html.attr.dir`, `css.text.rtl`, `css.text.align` |
| L9 | Selectors: `:lang()`, `:empty` with whitespace | `html.attr.global`, `sel.structural` |
| L10 | Named colours (`rebeccapurple`) | `css.color.basic` |
| L11 | The `translate` property | `css.transform.2d` |
| L12 | `<p>` block margins (1em) | `html.el.structure` |
| L13 | `letter-spacing` and `word-spacing` widen the text | `css.text.transform` |

### Painting (Direct2D; each painter to check on its platform)

| # | Item | Stage |
|---|---|---|
| P1 | Background images tile (repeat, size, position) | `css.background.basic` |
| P2 | Borders, outline | `css.box.border`, `css.box.outline` |
| P3 | `object-fit` | `css.box.object-fit` |
| P4 | Conic gradients | `css.gradient.conic` |
| P5 | `filter` (blur, brightness, drop-shadow) and `backdrop-filter` | `css.filter.blur-brightness-drop-shadow`, `css.backdrop-filter` |
| P6 | 2D transforms stage | `css.transform.2d` |
| P7 | `text-overflow: ellipsis` | `css.text.overflow` |
| P8 | Colour emoji | `css.text.emoji` |
| P9 | Canvas text | `canvas.text` |
| P10 | The no-selection stage | `behaviour.no-selection` |

### Out of the profile, not neutralised yet

| # | Item | Fails |
|---|---|---|
| O1 | A refused image (SVG, BMP, ICO, remote) shows the 48 px placeholder | `html.el.img-svg`, `html.el.img-other-formats`, `html.el.img-remote` |
| O2 | `createConicGradient` draws nothing | `canvas.conic-gradient` |
| O3 | `text-shadow` stage | `css.text.shadow` |
| O4 | Partly: legacy elements, logical properties | `html.el.legacy`, `css.box.logical` |

### Not TwidgetCore's

| Item | Where |
|---|---|
| `Intl`: number grouping, dates, plural rules, relative time, collation | the C++ desktop host's `IntlProvider` (`BasicIntl`); a Windows client needs ICU or Windows.Globalization behind it |
| `innerHTML` replacing children, live `getElementsByClassName` | widget-sandbox's shim (its `docs/changes.md` lists the second as known) |
| Right click / long press | the runner has no secondary button; profile 1 forwards no `contextmenu` anyway |
