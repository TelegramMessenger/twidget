# The fork's limits and budgets (stage L17)

Every limit, budget, cap and clamp of the litehtml fork (`litehtml/src`, `include/litehtml`, Gumbo) that bounds what a
widget can make it do, where it lives in the C++ and in the port, and the comparison with the C++ that crosses it.
"Crossed" means a differential case through `oracle/lhdump` (or `svg_oracle`) reaches the limit and goes past it, the
C++ and the Rust giving the same answers; "boundary" means cases just under it, at it and just past it.

72 limits: 43 were crossed by earlier stages' tests (21 of them now at their boundary too), 23 are crossed by this
stage's `tests/limits.rs` alone, and 6 cannot be compared: two the port leaves out because the renderer never takes
their path (the `<meta charset>` prescan, fragments), the sheet caches whose hits give what a parse gives, and three
guards no input reaches (the padding charge's 2^30, the tabular fix-up's passes, the paint generation's wrap).

`tests/limits.rs` holds the boundary cases this stage added: each limit's cases run in one lhdump process, as `Q`
(cascade, render tree, layout, paint and every container call, `sanitizer_limit` among them), `j` (Gumbo), `b`
(patches) or `W` (hit testing) records; where the dumps or the port's own counters can show it, a test also checks that the limit was met
on the far side only. A planted off-by-one (`generated_boxes > max_generated_boxes` for `>=`) failed
`generated_content_crosses_its_limits` on the 20001st box, the other 15 cases alike; reverted.

## DOM build and Gumbo

| Limit | C++ | Value, as compared | When met | Rust | Crossed by |
|---|---|---|---|---|---|
| Build depth | `document.h:113` `max_build_depth`, `document.cpp:2793` | 128: `m_build_depth >= 128` | the element and the rest of the document are not built; `sanitizer_limit("depth")`, `m_build_limited` | `document.rs:243`, `:468` | `dom.rs` `the_limits_build_alike` (126–130, 200, 401 levels); the walks of every stage at the limit's depth: `layout.rs`, `paint.rs`, `patch.rs` and `hit.rs` `a_deep_…_on_a_small_stack` |
| Elements built | `document.h:179` `max_build_elements`, `document.cpp:2793` | 20000: `m_build_elements >= 20000` | likewise, `sanitizer_limit("elements")` | `document.rs:245`, `:468` | `dom.rs` `the_limits_build_alike` (19996–20050) |
| Gumbo's open elements | `gumbo/parser.c:66` `kMaxOpenElements`, `:4034` | 400: `> 400` after a token | parse stops, `truncated`; the document reports `sanitizer_limit("elements")` (`document.cpp:83`, `:126`) | `gumbo/parser.rs:45`, `:3269` | `gumbo.rs` `the_limits_are_met_alike` (396–403 open elements of 13 kinds, 400 generated), `dom.rs` `the_limits_build_alike` (401) |
| Gumbo's cloned bytes | `gumbo/parser.c:67` `kMaxClonedBytes`, `:1242`, `:4035` | input length + 1 MiB, each clone `sizeof(GumboNode)` + its tag's length: `>` | as above | `gumbo/parser.rs:46`, `:3269` (`sizeof` as the C's on 64 bits) | `gumbo.rs` `the_limits_are_met_alike` (`formatting_blocks`, 7 shapes) |
| Gumbo's errors | `gumbo/error.c:165` `max_errors` | -1 (no bound) in `kGumboDefaultOptions` (`parser.c:62`); `errors >= max_errors` when set | error not recorded (a duplicate attribute's name then runs into the next, Findings) | `gumbo/error.rs:104` | `gumbo.rs` `the_options_parse_alike` (-1, 0, 1, 2, 5) |
| Structural patch size | `document.cpp:1909` `update_structure` | `total > 20000 \|\| deepest >= 128` (Gumbo's tree of the new HTML) | refused, the document as it was | `patch.rs:2944` | boundary: `limits.rs` `a_structural_patch_crosses_the_build_limits` (125–129 levels, 19996–20001 elements) |
| Structural patches after a limit | `document.cpp:1889` | `m_build_limited` set by any build limit | every later `update_structure` refused | `patch.rs` (`update_structure`) | `patch.rs` `the_corpus_is_patched_alike`; `limits.rs` `a_structural_patch_crosses_the_build_limits` |
| Gumbo's spec loops | `gumbo/parser.c:1216` (Noah's Ark), `:1815`, `:1940` (adoption agency) | 3 identical formatting elements; 8 outer, 3 inner rounds | the earliest dropped; the loops end | `gumbo/parser.rs` | `gumbo.rs` `formatting_elements_the_parser_opens_again_in_every_block_that_follows`, `generated_html_reaches_every_mode_state_and_error` |
| Numeric references | `gumbo/char_ref.c:139`, `:163` | the code point wraps past 2^32 (`unsigned`), U+FFFD past 0x10FFFF and for surrogates | replaced, an error | `gumbo/char_ref.rs` | `gumbo.rs` `generated_html_parses_alike`; boundary: `limits.rs` `numeric_references_cross_their_bounds` (0x10FFFF and 0x110000, the surrogates' ends, 2^32 - 1, 2^32, 2^32 + 0x41) |
| Tab stops | `gumbo/utf8.c:189` | `tab_stop` 8 (an option) | source columns only | `gumbo/utf8.rs` | `gumbo.rs` `the_options_parse_alike` (1, 3, 4, 8) |
| `<meta charset>` prescan | `encodings.cpp:7877`-`7894` | 1024 bytes | stops | not ported: `createFromString`'s sniffing is not the renderer's path (L3) | none |
| Fragments | `document.cpp:3842`-`3852` `append_children_from_string` | depth not seeded, `truncated` ignored, text to its first NUL | none | not ported: the renderer never appends fragments | none |

## CSS syntax, selectors and the cascade

| Limit | C++ | Value, as compared | When met | Rust | Crossed by |
|---|---|---|---|---|---|
| Nesting of blocks and functions | `css_parser.cpp:347` `kMaxNesting`, `:382`, `:443` | 128: `m_depth >= 128` | the block or function is skipped whole, kept empty | `css_parser.rs:172`, `:430`, `:470` | `tokens.rs` `generated_tokens_alike` (`any_css` nests past it); boundary: `limits.rs` `css_nesting_crosses_128` (125–130, style attributes and sheets, `()`, `[]`, `calc(`) |
| Whitespace kept by list length | `css_parser.cpp:45` `kLargeSize` | 50 tokens: `> 50` | decides against the original tokens instead of those kept (Findings) | `css_parser.rs:59`, `:76`, `:113`, `:127` | `tokens.rs` `whitespace_kept_by_list_length_alike` |
| Escapes | `css_tokenizer.cpp:180`, `:192` | 6 hex digits; 0, surrogates and past 0x10FFFF are U+FFFD | none | `css_tokenizer.rs` | `tokens.rs` `generated_tokens_alike` (`any_css`: escapes of every length) |
| Compounds of a selector | `css_selector.cpp:771` `kMaxCompounds`, `:791`, `:825` | 128: `++g_compounds > 128`, across nested `:is()`/`:not()` lists | the selector is refused | `css_selector.rs:275`, `:915`, `:944` | boundary: `limits.rs` `selector_compounds_cross_128` (126–130: `+` chains, nested `:is()` and `:not()`, lists) |
| Selector steps | `html_tag.cpp:200` `kSelectorStepBudget`, `:906` | 4000000: `++steps > 4000000` while a sheet is applied or a `selector_budget` lives; a child walk costs `count / 64` (`:251`) | every later match fails until the scope ends | `html_tag.rs:70`, `:107`, `:133` | `cascade.rs` `the_selector_budget_runs_out_alike` (a deep chain and 600 siblings: the last rule reaches some elements and misses others) |
| Sibling index | `html_tag.cpp:1999` | used for a parent of `> 16` children in a sheet's pass (`LH_NO_SIBLING_INDEX` off) | sibling searches charge the budget differently | `html_tag.rs:425` | `cascade.rs` `the_selector_budget_runs_out_alike`, `the_switches_cascade_alike`; boundary: `limits.rs` `sibling_thresholds_match_alike` (15–17, 63–65, 127–129 children) |
| Used selectors | `html_tag.cpp:467` `max_used_styles` | 256: `size() >= 256` | later matching selectors are not applied | `html_tag.rs:890`, `:1004` | boundary: `limits.rs` `used_styles_cross_256` (248–262 rules; the port's list stops at 256) |
| Match memos | `html_tag.cpp:460` | 2: `size() >= 2` drops the oldest | a flip back to a forgotten class is matched again | `patch.rs:1514` | `patch.rs` (`b` dumps every tag's memos); `limits.rs` `match_memos_keep_two` (four classes in turn) |
| Class flips | `html_tag.cpp:312`, `:408` | 3: `size() >= 3` drops the oldest | a flip back is restyled again | `patch.rs` (`note_flip`) | `patch.rs` `the_corpus_is_patched_alike`, `written_patches_are_alike` (flips dumped per tag); `limits.rs` `match_memos_keep_two` (four classes) |
| Counter increments | `html_tag.cpp:2167`, `element.cpp:457` | 2 changes an element a pass (`< 2`); the sum clamped to `int` (resets are not) | not applied; clamped | `html_tag.rs:1166`, `element.rs` | boundary: `limits.rs` `clamps_cross_their_bounds` (INT_MAX - 1 to + 1, a `:hover` and a refresh) |
| Counter names | `element.cpp:434` `max_names` | 16: `names++ < 16` | the 17th and later names of a `counter-reset`, `-set` or `-increment` are ignored | `element.rs:555` | boundary: `limits.rs` `counter_names_cross_16` (14–18 names) |
| var() expansion | `style.cpp:2576` `max_var_substitution_tokens`, `:2626` | 16384 tokens an element's declarations expand to: `size > left` | that declaration and every later one is `unset` (a custom property: guaranteed-invalid) | `style.rs:1005` | boundary: `limits.rs` `var_tokens_and_depth_cross_their_limits` (16382–16386, and two of 8191–8193) |
| var() depth | `style.cpp:2601` | 32: `depth > 32` | invalid | `style.rs:1066` | boundary: `limits.rs` `var_tokens_and_depth_cross_their_limits` (chains 29–44 deep) |
| Declaration cache | `style.cpp:156`–`157`, `:238`, `:241` | keys of 512 bytes at most (`<= 512`), emptied whole past 50000 entries or 1 MiB of keys (`>`), viewport lengths not kept; process-wide | a verdict replayed or asked again (Findings: keyed on whether there is a container) | `style.rs:394`–`396`, `:423`, `:1213` | `properties.rs` `declaration_cache_limits_alike` |
| Sheet caches | `document.cpp:2499` (master, `> 64`), `:2586` (document sheets, `> 128` or 256 KiB) | emptied whole | none visible: a hit gives what a parse gives | not kept (`document.rs`'s header) | nothing to compare: no output depends on them |

## Values

| Limit | C++ | Value, as compared | When met | Rust | Crossed by |
|---|---|---|---|---|---|
| calc() nodes | `css_length.cpp:29` `MAX_NODES`, `:300` | 128: `node_count > 128` | the calc() is refused | `css_length.rs:58`, `:432` | boundary: `limits.rs` `calc_nodes_and_depths_cross_their_limits` (sums of 60–68 terms, products, groups) |
| calc() atom depth | `css_length.cpp:75` | 16: `depth > 16` | refused | `css_length.rs:103` | boundary: `limits.rs` `calc_nodes_and_depths_cross_their_limits` (12–20 levels) |
| Math folding depth | `css_effects.cpp:267` (`calc_atom`), `:501` (`fold_math`) | 16: `depth > 16` | not folded, the value refused | `css_effects.rs:667`, `:953` | boundary: `limits.rs` `calc_nodes_and_depths_cross_their_limits` (`min()`, `clamp()`, transforms, filters, opacity 12–20 deep) |
| Math function arguments | `css_effects.cpp:304` | 32: `args.size() > 32` | refused | `css_effects.rs` (`calc_atom`) | boundary: `limits.rs` `effect_counts_cross_32` (31–33) |
| Filter functions | `css_effects.cpp:633` | 32 tokens: `size() > 32` | the filter is refused | `css_effects.rs` (`parse_filter`) | boundary: `limits.rs` `effect_counts_cross_32` (31–33, `filter` and `backdrop-filter`) |
| Filter radius and offsets | `css_properties.cpp:984`–`986` | radius `min(r, 256)` px, offsets `clamp(-1024, 1024)` px | clamped | `css_properties.rs:1500`–`1502` | boundary: `limits.rs` `clamped_values_cross_their_bounds` |
| Font feature value | `style.cpp:430` | `min(n, 65535)` | clamped | `style.rs:582` | boundary: `limits.rs` `clamped_values_cross_their_bounds` |
| Integers of the cascade | `style.cpp:909` (`order`), `css_properties.h:459` (`z-index`), `style.cpp:1887` (`font-weight` 1-1000), `style.cpp:1005` (`opacity` 0-1), `style.cpp:1438` (1-4 values) | clamped to `int`, refused outside 1-1000, clamped, refused past 4 | none | `style.rs`, `css_properties.rs` | `properties.rs` `generated_declarations_alike` (weights 1000, 1001); boundary: `limits.rs` `clamps_cross_their_bounds` (INT_MAX +- 1, INT_MIN - 1, 1e20; weights 0, 1, 999-1001) |
| `clamp_length` | `css_length.h:70`-`73` | +-2^25 px (NaN 0) for % and calc() and every `to_pixels`; a plain px from `calc_percent` not clamped | clamped | `css_length.rs:313` | boundary: `limits.rs` `clamps_cross_their_bounds` (2^25 - 1 to + 1, -(2^25 + 1), 1e10, 1e8 %) |
| `color-mix()` | `css_effects.cpp:444`, `:463`-`476` | each % 0-100, their sum above 0, scaled under 100, channels and alpha clamped | refused; scaled | `css_effects.rs` | `properties.rs` `effects_alike`; boundary: `limits.rs` `clamps_cross_their_bounds` (0, 100, -1, 101, sums 0, 60, 100, 120) |
| Grid lines | `css_effects.cpp:1216`, `:1233`-`1240` | clamped to +-1000; 0, fractions, negative spans and a second `/` refused | none | `css_effects.rs` | `properties.rs` `generated_declarations_alike`; boundary: `limits.rs` `clamps_cross_their_bounds` (999-1001, -1001, spans 1000, 1001) |
| Flex factors | `flex_item.cpp:16`-`27`, `flex_line.cpp:81`, `:181` | x1000, rounded, `fmin(..., INT_MAX)`; a negative shrink 1000; sums under 1000 scale the free space | clamped | `flex_item.rs:89`-`93`, `flex_line.rs` | `layout.rs` `written_flex_shapes_lay_out_alike` (factors under one); boundary: `limits.rs` `clamps_cross_their_bounds` (2147483.646-.648, 1e10, 0.0004, 0.0005, 0.999) |
| Grid tracks | `css_effects.cpp:1134` `max_tracks`, `:1174`, `:1184`, `:1199` | 1000: a repeat of `> 1000` sizes refused, copies while `<= 1000`, written tracks while `< 1000` | the rest dropped | `css_effects.rs:1612`, `:1642`, `:1651`, `:1664` | `properties.rs` `generated_declarations_alike` (`repeat(1001, …)`), `layout.rs` `written_grid_shapes_lay_out_alike` (`repeat(1000, 1px)`); boundary: `limits.rs` `grid_tracks_cross_1000` (998–1002, repeats, lists, rows, auto-fill) |
| Fast transform path | `css_effects.cpp:756`, `:802`, `:846`, `:848`, `:872` | value `<= 512` bytes, `<= 32` functions, `<= 6` numbers each, numbers `<= 32` characters and `<= 1e9` | the tokenizer's path instead (same result) | `css_effects.rs:1227` | boundary: `limits.rs` `a_patched_transform_crosses_512_bytes`, `effect_counts_cross_32` (through `update_styles`) |
| Roman numerals | `num_cvt.cpp:69`, `:109` | 1 to 3999 | decimal | `num_cvt.rs:44` | `oracle.rs` `markers_alike` (3999, 4000, the alphabetic edges) |
| Alphabetic markers | `num_cvt.cpp:16`, `:34` | from 1, no upper bound | decimal under 1 | `num_cvt.rs` | `oracle.rs` `markers_alike` (26, 27, 702, 703, 18278, 18279, INT_MAX) |
| Attributes read as integers | `table.cpp:14` (spans, `atoi`), `render_block.cpp:65`-`73` (`<ol start>`, `val < INT_MAX`), `render_block.cpp:323` (`<textarea rows>`, `strtol`, out of 1 to INT_MAX is 2), `el_font.cpp:26` (`<font size>` +-7), `el_form.cpp:546` (`<input size>` 1-1000, else 20), `html_microsyntaxes.cpp:13`, `:54` | as written, `long` 32 bits on Windows (Findings) | clamped or defaulted | `twidget_cstd` (`atoi`, `strtol` per platform), `render_block.rs`, `el_font.rs`, `el_form.rs` | `dom.rs` `numbers_in_attributes_read_alike`, `every_attribute_of_every_class_builds_alike`; boundary: `limits.rs` `clamps_cross_their_bounds` (0, 1, 1000, 1001, -5, 2^32 + 1; starts INT_MAX - 1 to + 1, INT_MIN - 1; rows INT_MAX, + 1, 2^32) |

## Generated content

| Limit | C++ | Value, as compared | When met | Rust | Crossed by |
|---|---|---|---|---|---|
| Generations | `el_before_after.h:26` `max_generations`, `el_before_after.cpp:39` | 8: `m_generations >= 8` between two styles | the rule's content is not made | `el_before_after.rs:16`, `:42` | boundary: `limits.rs` `content_generations_cross_8` (6–10 rules, texts and images) |
| A box's bytes | `document.h:184` `max_generated_box`, `document.cpp:1472` | 32 KiB: `bytes > 32768` (a word, space or image url) | no more content in the document, `sanitizer_limit("content")` | `document.rs:248`, `:583` | boundary: `limits.rs` `generated_content_crosses_its_limits` (32766–32770, words and urls) |
| A document's bytes | `document.h:185` `max_generated_bytes` | 1 MiB: `bytes > 1048576 - used` | as above | `document.rs:249`, `:583` | boundary: `limits.rs` `generated_content_crosses_its_limits` (1 MiB + 0, 1, 2) |
| A document's boxes | `document.h:183` `max_generated_boxes` | 20000: `boxes >= 20000` | as above | `document.rs:247`, `:583` | `cascade.rs` (generated documents); boundary: `limits.rs` `generated_content_crosses_its_limits` (19999–20001) |

## Render tree, tables, grid and layout

| Limit | C++ | Value, as compared | When met | Rust | Crossed by |
|---|---|---|---|---|---|
| Table cells | `document.h:180` `max_table_cells`, `document.cpp:1455` | 200000: `cells + count > 200000`, a cell's span and the padding of rows (`table.cpp:21`, `:185`) | the cell spans 1 × 1, the padding stops; `sanitizer_limit("table")` | `document.rs:251`, `table.rs:773`, `:744` | `render_tree.rs` `written_shapes_build_alike` (`budget_documents`); boundary: `limits.rs` `table_cells_cross_200000` (199999, 200000, 200001 cells; two tables sharing) |
| Span clamps | `table.cpp:14`–`19` | colspan and rowspan 1 to 1000 | clamped | `table.rs` (`add_cell`) | `render_tree.rs` `generated_documents_build_alike` (`1001`, `0`, `-1`); boundary: `limits.rs` `table_cells_cross_200000` (0, 1, 999, 1000, 1001, 5000) |
| A cell's height in its row | `render_table.cpp:359` | `min(h, 2^20)` px | clamped | `render_table.rs:593` | boundary: `limits.rs` `clamped_values_cross_their_bounds` |
| Padding charge | `table.cpp:185` | `count < 2^30` | not charged, the padding stops | `table.rs:744` | unreachable under the 20000-element limit (a row of 1000-column spans pads at most 2 × 10^7 cells) |
| Tabular fix-up passes | `document.cpp:3640` | `size × 8 + 64` items processed | the rest not fixed | `document.rs:937` | unreachable: each item adds at most three anonymous parents |
| Grid placement | `render_grid.cpp:154`, `:174`, `:188` | 200000 cells examined or marked: `budget <= 0` | the items left stack in rows of their own | `render_grid.rs:234`, `:253`, `:266` | `layout.rs` `generated_grid_documents_lay_out_alike`, `written_grid_shapes_lay_out_alike`; boundary: `limits.rs` `grid_placement_budget_crosses_200000` (198–202 items of 1000 columns, items of 199–201 × 1000) |
| Auto repeats | `render_grid.cpp:87`, `:89` | `min(room / step, 1000)`, then `1000 / repeat size` | fewer tracks | `render_grid.rs:172`, `:174` | `layout.rs` `written_grid_shapes_lay_out_alike`; boundary: `limits.rs` `clamps_cross_their_bounds` (999-1001 and 2000 px of 1 px tracks, a repeat of two) |
| Grid sizing rounds | `render_grid.cpp:410`-`413` (growth to limits, `guard < 8`, free space `<= 0.5` px), `:451`, `:832` (`fr` freezing, `guard < 16`) | 8 and 16 rounds | stops | `render_grid.rs` | `layout.rs` `generated_grid_documents_lay_out_alike` (L11: every block reached) |
| Fixed-size flex reuse | `render_item.cpp:106`, `:114` | `++count > 256`, `pending + count >= 256` | the flex is not reused (laid out again) | `layout.rs:382`, `:391` | boundary: `limits.rs` `layout_and_culling_thresholds_cross_alike` (254-258 items, laid out three times) |
| Measure cache | `render_item.h:113`-`114` | two measures and the last layout an item | older measures forgotten (what costs the layout budget) | `layout.rs` (`measured`) | `layout.rs` (every `Y` dump writes each item's cache), `the_layout_switches_lay_out_alike` |
| Layout budget | `document.h:111`, `:120` | `++spent > budget && budget` (0: none) | every box returns at once, the document keeps its last layout | `layout.rs:494`, `:507` | `layout.rs` `the_budget_is_crossed_alike` (1 to 1000); boundary: `limits.rs` `the_layout_budget_crosses_exactly` (all the boxes − 1, all, + 1) |
| The renderer's layout budget | `core/src/renderer.cpp:762` | 100000 + 50 a DOM element (set on the fork) | the widget stops with the badge | `twidget_renderer` | `renderer` `tests/oracle.rs` `budget_and_names` (stage 4.3) |

## Painting and SVG

| Limit | C++ | Value, as compared | When met | Rust | Crossed by |
|---|---|---|---|---|---|
| Repeating gradient stops | `background.cpp:765` `max_repeated_color_points`, `:827`, `:868` | 4096 | the layer drawn as its average colour | `background.rs:677`, `:736`, `:756` | `paint.rs` `written_paint_shapes_paint_alike` (periods past it); boundary: `limits.rs` `repeated_gradient_stops_cross_4096` (25 periods across it, the three kinds) |
| `<use>` depth | `el_svg.cpp:1156` | 8: `depth > 8` | not drawn | `el_svg.rs:1164` | `paint.rs` `written_svg_shapes_paint_alike` ("use chains deeper than eight") |
| `<use>` draws a pass | `el_svg.cpp:908` `kMaxUseDrawn`, `:1800` | 10000 units a paint generation, shared by the document's `<svg>`s | the budget is spent for good in that generation | `el_svg.rs:95`, `:1706` | `paint.rs` `written_svg_shapes_paint_alike` ("use budget shared by svgs", "the use budget short at …") |
| `<use>` charges | `el_svg.cpp:909` `kUseLookup`, `:987`, `:1053`, `:1140`, `:1204` | 16 a lookup inside a `<use>`, 1 a child, 1 per 256 children, points or bytes | as above | `el_svg.rs:98`, `:1141`, `:1892`, `:1907`, `:2039`, `:2115` | `paint.rs` `written_svg_shapes_paint_alike` (short at a group's children, a path's points, a text's bytes) |
| Reference ids | `el_svg.cpp:746` `svg_reference_id` | 1 to 128 bytes: `size() > 128` | names nothing | `el_svg.rs:227` | boundary: `limits.rs` `svg_reference_ids_cross_128_bytes` (126–130, `href` and `url(#…)`) |
| Elements in a paint key | `el_svg.cpp:877`, `:1886` | 4000: `++count > 4000` | not cacheable / no key | `el_svg.rs:1550`, `:1601` | `paint.rs` `written_svg_shapes_paint_alike` ("an svg of more than 4000 elements") |
| Cached groups | `el_svg.cpp:877`, `:886` | top-level `<g>`s of 8 to 4000 elements | not drawn through the cache | `el_svg.rs:1550` | `paint.rs` `written_svg_shapes_paint_alike` ("cached groups", "groups the cache refuses") |
| A full `<use>` budget elsewhere | `el_svg.cpp:1264`, `:1754`, `:1768` | 10000 for each cached group's painter, `paint_shapes` and hit test | none | `el_svg.rs:1755`, `:1891`, `:1988` | `paint.rs` `written_svg_shapes_paint_alike` (with `paint_shapes`), `hit.rs` `the_corpus_svgs_point_alike` (`el_svg::element_at`) |
| Transformed culling | `render_item.cpp:1557`, `:1574` | `abs(det) > 1e-8`, every corner within 1e6 | no culling under that transform | `paint.rs:876`, `:894` | `paint.rs` `the_switches_paint_alike` (`LH_NO_TRANSFORMED_CULLING`); boundary: `limits.rs` `layout_and_culling_thresholds_cross_alike` (scales 1e-4, 9e-5, 1e-5; translations 999999, 1000001 px) |
| Paint generations | `document.h:260` | `uint32`, 0 skipped | wraps | `paint.rs` (`DocPaint::generation`) | unreachable (2^32 paints) |
| Hit transforms | `render_item.cpp:1838` (`get_child_by_point`) | `fabs(det) < 1e-12` | the transformed child is passed over | `hit.rs:296` | `hit.rs` `generated_documents_point_alike`, `written_shapes_point_alike` (`scale(0)`); boundary: `limits.rs` `hit_transforms_cross_1e_12` (determinants about 1e-12 either side, points in and out, the pointer moved) |

## Patching and incremental relayout

| Limit | C++ | Value, as compared | When met | Rust | Crossed by |
|---|---|---|---|---|---|
| Dirty regions | `document.cpp:3156` `layout_dirty_regions` | 128 boxes: `size() > 128`, and their area at most half the root's | no regions (the whole document repaints) | `patch.rs:3280` | boundary: `limits.rs` `dirty_regions_cross_128_boxes` (a word and 60–66 inline blocks moved) |

## Other numbers

`url.cpp:167` drops a trailing incomplete escape when decoding (L2, `sheets.rs` `declarations_and_values_alike`);
`strtod.cpp`'s exponent bound of 511 and 18 digits (L0, `litehtml.rs` `the_forks_own_strtod_and_an_exponent_that_fits_no_int`,
`oracle.rs` `numbers_alike`); the table widths' 1/64 px rounding and its epsilon (L12); SVG's 64 steps a Bezier, its
300 x 150 default size and its group bounds of +-100000 (L14) are constants of an algorithm, compared by their stages.

## The renderer's bounds on the fork

The name table has no bound in the fork: `_id_usage()` (`string_id.cpp:66`) reports its names and bytes, and the
renderer stops a widget past 20000 names or 1 MiB of them (`core/src/renderer.cpp:536`–`537`, `twidget_renderer`'s
`MAX_NAMES`, `MAX_NAME_BYTES`), crossed by `renderer` `tests/oracle.rs` `budget_and_names` and
`tests/renderer.rs` `names_stop_a_widget_that_keeps_inventing_them` (stage 4.3).

## Switches

Every switch the C++ reads with `getenv` is a field of `Switches` (`document.rs`, `patch.rs`) and is set on both sides
by a test: `LH_NO_LAYOUT_CACHE`, `LH_LAYOUT_CACHE_CHECK`, `LH_NO_POSITIONED_REUSE`, `LH_NO_FIXED_LAYOUT_REUSE`,
`LH_NO_COLUMN_REUSE` (`layout.rs` `the_layout_switches_lay_out_alike`, `paint.rs` `the_switches_paint_alike`),
`LH_NO_TRANSFORMED_CULLING` (`paint.rs`), `LH_NO_SVG_GROUP_CACHE`, `LH_SVG_LATE_CACHE` (`paint.rs`
`the_switches_paint_svgs_alike`), `LH_NO_DESCENDANT_PRUNE`, `LH_NO_SIBLING_INDEX`, `LH_NO_ID_SHARE`,
`LH_NO_STYLE_SHARING` (`cascade.rs` `the_switches_cascade_alike`), and `LH_NO_TEMPLATE`, `LH_NO_STRUCTURE`,
`LH_NO_CLASS_HOLES`, `LH_NO_CLASS_FLIP`, `LH_NO_MATCH_MEMO`, `LH_NO_BACKGROUND_PATCH`, `LH_NO_PROPERTY_PATCH`,
`LH_NO_COLOR_REGIONS`, `LH_NO_TEXT_REGIONS`, `LH_NO_INPLACE_TEXT`, `LH_NO_SHALLOW_WIDTH_PATCH`, `LH_NO_KEEP_TREE`,
`LH_NO_PAINT_ONLY`, `LH_NO_PATCH_REGIONS`, `LH_NO_SAME_SIZE_TEXT` (`patch.rs` `the_patching_switches_are_alike`).
