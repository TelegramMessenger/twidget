# Performance parity with the Swift renderer

What the Apple renderer (`Sources/TwidgetRenderer`, `Sources/LiteHTML/LH*.mm`) does to save work that
TwidgetCore did not, from an audit against `main` at 82ec7f0 (October 2026). The litehtml fork is
shared, so its own optimisations are not listed. Paths are relative to `Sources/`; line numbers are
as of the audit. The status column records what TwidgetCore has since.

The goal is parity, measured: `Tests/TwidgetCoreTests/bench.cpp` drives the renderer with protocol
ops as `FrameCostBenchTests.swift` does on Apple, and each change below reports its effect there.

## Missing or partial

| # | Item | Apple | TwidgetCore before | Status |
|---|---|---|---|---|
| 1 | **Damage regions.** Repaint only what changed: the fork's patch and layout dirty regions, else a diff of two display lists. | `LiteHTMLWidgetRenderer.swift:1696-1716`; `LHDocument.mm:1204-1281` (`patchDirtyRects`, `layoutDirtyRects`), `391-480` (`changedRectsSince`); `document::draw(..., cull)` | `Renderer::paint` returns the whole list, `Container::draw` clips to the viewport without culling, painters paint everything | open |
| 2 | **Patch ops in place.** A patch log of text, class, style, attribute and form value changes goes to `update_holes`, without serialising. | `WidgetDOM.swift:303-478` (`PatchLog`), `LiteHTMLWidgetRenderer.swift:1547` (`patchedDocument`), `LHDocument.mm:1039-1109` | every batch serialises the shadow tree and the fork diffs it | done: patch_document, checked against full builds by a randomised test. The fork's update_holes is now most of a style-heavy frame (about 13 us per changed style on MSVC), shared with Swift |
| 3 | **Skip batches that leave the document alone.** Canvas-only and no-op batches render nothing; canvases show in their own layer. | `LiteHTMLWidgetRenderer.swift:874-877, 1253-1271`; `WidgetCanvasPresenter.swift` | every `ops` message rebuilds; a canvas frame repaints the document | done: batches that change nothing serialised skip the build; a canvas layer is still to do |
| 4 | **Render coalescing.** Batches mark the document stale; one render per frame, input-driven batches at once, queries force it. | `LiteHTMLWidgetRenderer.swift:812-830, 985-1010` (`renderOrDefer`, `setNeedsRender`, `renderAtTick`) | builds synchronously on each message | open |
| 5 | **One build per batch.** Overrides are sampled before layout; a second layout only when they differ. | `LiteHTMLWidgetRenderer.swift:1123-1131` | `animate()` calls `build_document()` again when overrides change (a class toggle that starts a transition builds twice) | done |
| 6 | **Scripted transform promotion.** Elements a script moves frame after frame become layers updated without a render; crowded batches are not promoted. | `LiteHTMLWidgetRenderer.swift:1796-1868`, `WidgetCompositor.swift:513-590, 1155-1180`; af5799a | only animator-driven elements are layers | open |
| 7 | **Second layout only when the viewport height matters.** Fixed boxes, % heights on html/body, vh units, height media queries. | `LHDocument.mm:91-105, 1410-1418` (`NeedsViewportHeight`) | `build_document` lays out again whenever the height changes | done: depends_on_viewport_height (fixed boxes, % heights on html/body, viewport units, height media queries, absolute boxes against the viewport); also on the in-place path |
| 8 | **Compositor work per tick.** Ancestor sets once per update; no compositor update on animation-only frames. | `WidgetCompositor.swift:1915-1950, 1162-1167`; `LiteHTMLWidgetRenderer.swift:1413-1445` | pairwise `is_ancestor`, candidate and background walks every tick before the key early-out | done: key before the filters (also when no layer is wanted), one upward walk for nested animations, background locality only for animated ancestors. Bench css 400: tick 9.5 to 7.9 ms |
| 9 | **Occlusion scan caching.** Pre/post-order intervals and boxes per layout epoch; answers memoised across rounds. | `LHDocument.mm:758-851` (`ensureCoverLocked`); `WidgetCompositor.swift:1219-1250` | `covered()` finds itself in the trace, clips every item's box and walks parents per call | open |
| 10 | **Cheap paint-order trace.** Draw callbacks return at once while tracing; the trace is cached. | `LHContainer.mm:1880-1884`; `LHDocument.mm:728-749` | `paint_order()` records a whole display list and drops it | done: draw callbacks return while tracing; trace cached on document, paint generation and layout |
| 11 | **Font and width caches.** Fonts deduplicated by description process-wide; per-font width caches. | `LHContainer.mm:942-944, 1197-1253, 1505-1560` | a new handle per `create_font` (the width cache misses after a reparse), a key built per lookup, `measure_text` makes a font per query | done: faces shared by description across documents (bounded, least recently used go), widths per face without key allocation, measureText through the same faces |
| 12 | **Serialisation caches.** Open tags and escaped text per node and epoch, the stylesheet list, resolved sheets, a token check before light-dark. | `WidgetDOM.swift:931-1031, 1162`; `LiteHTMLWidgetRenderer.swift:1012-1028` | every tag rebuilt and every `<style>` rewritten per build; `animate()` re-walks before its key check | open |
| 13 | **Inert clips.** A clip applies only to ops that can cross it; a layer clip that can never clip is dropped. | `LHContainer.mm:1712, 2641` (`clips_needed`, `apply_clips`; 2dcc2ee); `WidgetCompositor.swift:1611` (`clipIsInert`) | every clip is emitted and applied | open |
| 14 | **No repaint without a change.** | empty diffs invalidate nothing | `build_document` sets `dirty_` unconditionally | done |
| 15 | **Render throttle.** 16 ms passes at a 25 % duty cycle, 5 % under sustained load; input bypasses it. | `LiteHTMLWidgetRenderer.swift:376-470` | none | open |
| 16 | **Shimmer and SVG layers.** Background-position animations and SVGs touched three frames running get layers. | `WidgetShimmers.swift`, `WidgetSVGLayers.swift` | transform and opacity only | open |
| 17 | **Cached SVG rasters.** `draw_cached` keyed raster cache. | `LHContainer.mm:3453-3484`; 736b141 | not overridden: SVG paths are recorded and tessellated every paint | open |
| 18 | **Layer content from a signature.** Re-raster decided from DOM and style keys, not by recording. | `WidgetCompositor.swift:1412-1413, 1757-1803` | every dirty paint records and digests every layer | done: layers keep content unless a patched element is inside them or a build, layout or hover change touched everything. Bench css 400: paint 2.4 to 0.4 ms |
| 19 | **Allocations.** Interest checked before building events; no size report per tick; declarations unchanged answer without a copy; canvas colour cache and interned command names; cached timing parses. | `LiteHTMLWidgetRenderer.swift:2707, 2748-2756`; `LHDocument.mm:2608`; `WidgetCanvas.swift:813`, `WidgetCanvasStream.swift`; `WidgetAnimations.swift:87-105` | events built then dropped, `report_size` per tick, map copies, string compares | open |
| 20 | **Memory trimming.** Caches purged under pressure and when hidden. | `LHContainer.mm:1123-1195`; `setVisible` | none | open |

## From the Mac comparison, 2026-10-14

Found by running the macOS demo against the Swift renderer with the `widget-stress` fixtures (see
`TWIDGET_CORE.md`, Portability check), and from Swift work since the audit.

| # | Item | Apple | TwidgetCore before | Status |
|---|---|---|---|---|
| 21 | **Canvas-market charts** show a flat fill instead of their content, in replay and live. | correct | wrong | open: needs the fixture |
| 22 | **Configurable height bound.** A chat host needs a bounded height of its own choosing; the core stops at a fixed 900 CSS px. | host-configured | `MAX_HEIGHT` constant | open |
| 23 | **Layer budget and nesting.** 256 layers; nested animated elements and their overlapping labels promoted together (dense overlap promotion, b3670ec). | `WidgetCompositor.swift` | 64 layers; an animated ancestor or descendant rules a layer out | partly: 256 layers and overlap promotion (up to 3 still layers over a wanted one) done; nested layers need a parent link in Composition and every player (23b), open |
| 24 | **CoreGraphics caches.** Bounded canvas text and compiled-gradient caches (b3670ec); text lines and gradients made once. | `WidgetCanvas.swift` | `CGBackend` creates lines and gradients per draw | open |
| 25 | **Collapsed transforms** clip to nothing instead of inverting a singular matrix (d2832ed). | `LHContainer.mm` | not affected: clips are recorded in the group's own space and painters only concatenate the matrix, never invert it | n/a |
| 26 | **Qt adapter composites.** The Qt demo paints a new QImage per repaint and does not enable composition. | Core Animation | whole frames | open |
| 27 | **Focus model.** Focus and selection operations. | yes | ignored | open |
| 28 | **Swift branch `codex/widget-stress-optimizations`** (unmerged, 10 commits on 2026-10-14/15): paint exclusion reuse, local paint reuse, fixed-flex layout reuse across resizes, guarded width-only restyles, scroll raster and tile work. | branch | — | tracked until merged |

## Already at parity

Style holes for animation frames (`apply_overrides` → `update_styles`), paint-only patches without
layout, the declaration scan cache, document reuse across resizes, the interpolation memo, move and
wheel coalescing, the layer budget, the composition key early-out, track reuse, layer content
versions by digest, the canvas font cache, box-first occlusion tests, local background animations
under layers, the element index.

## Not applicable

Core Animation tiles, off-main tile raster and surface pools, CoreText line caches and glyph
batching, Core Image blur caches (platform painters' work); Objective-C set bridging and Swift
JSONSerialization (encode.cpp writes JSON directly); off-main document teardown (litehtml calls back
into the backend); window event coalescing (host); scrolling and magic move (not in TwidgetCore yet).
