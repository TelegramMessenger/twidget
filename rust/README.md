# twidget in safe Rust

The port of everything Telegram's widget helper runs to Rust without `unsafe`, compared with the C++ at
every stage. `PLAN.md` has the goal, the rules, the stages and where they stand.

| Path | What |
|---|---|
| `crates/wire` | `twidget_wire`: the paint values and their binary forms (`core/src/wire.cpp`) |
| `crates/paint` | `twidget_paint`: heavy boxes, the paint limit and its ledger (`core/src/paint_policy`) |
| `crates/remote` | `twidget_remote`: the messages between the renderer end and the platform end (`core/src/remote.cpp`) |

| `crates/computed_style` | `twidget_computed_style`: the `cs` query's strings, getComputedStyle as Chromium writes it, from what the fork computed for an element (`core/src/computed_style.cpp`), made from the Rust litehtml's cascade and render tree but for the layout boxes |
| `crates/css` | `twidget_css`: TwidgetCore's small CSS helpers (`core/src/css.cpp`) |
| `crates/cstd` | `twidget_cstd`: the C library behaviours the C++ leans on (`strtod`, `strtof`, `strtol`, `atof`, `atoi`, `%.*f`, `%g`) |
| `crates/compositor` | `twidget_compositor`: layers and their baked tracks, paint digests, damage, inert clips (`core/src/compositor.cpp`) |
| `crates/base64` | `twidget_base64`: the base64 of canvas pixels (`core/src/encode.cpp`) |
| `crates/filters` | `twidget_filters`: CSS filter functions for painters: colour matrices, a chain's reach, the CPU pass over pixels (`core/src/filters.cpp`) |
| `crates/png` | `twidget_png`: the rows of a damaged PNG a browser keeps, and the size an image header declares (`core/src/png.cpp`) |
| `crates/encode` | `twidget_encode`: the JSON forms of a display list, a canvas batch and a composition, for painters outside C++ (`core/src/encode.cpp`) |
| `crates/canvas` | `twidget_canvas`: the 2D canvas interpreter, commands in and canvas ops out through a `CanvasHost` (`core/src/canvas.cpp`) |
| `crates/animations` | `twidget_animations`: the animator, CSS animations, transitions and Web Animations: declarations and times in, style overrides and events out (`core/src/animations.cpp`) |
| `crates/shadow` | `twidget_shadow`: the renderer's shadow tree, the render ops applied to it with their limits and the canvases they make, and the HTML it is written as (`core/src/renderer.cpp`) |
| `crates/events` | `twidget_events`: the renderer's input: pointers, hover and gestures with their listeners, the presses a widget prevents, the gesture claim and the arbiter's verdicts, pointer capture, `<details>` and `<img>` events, resize observers, focus and the platform's editor, text selection and its tint, over the shadow tree and an `EventHost` for what layout and the rest of the renderer answer (`core/src/renderer.cpp`) |
| `crates/renderer` | `twidget_renderer`: the renderer: host messages in, the document built from the widget's ops and patched in place where the fork can (its holes, its style holes, the serialised HTML), the coalesced batch and the viewport that follows the document, the layout budget and the name table's bounds that stop a widget, `frozen` and its badge, the theme and the safe area with the master CSS made from them, the size reports and resize diffs, animation overrides, and every query kind with its `result`; the shadow tree and the input driven as the C++ drives them, over an `Engine` trait for litehtml's document and a `Layers` trait for stage 4.5 (`core/src/renderer.cpp`) |
| `crates/ends` | `twidget_ends`: the two ends over a byte channel: `RendererEnd` with its Backend proxy over a `Renderer` trait, and `PlatformEnd` (`core/src/renderer_end.cpp`, `platform_end.cpp`) |
| `crates/litehtml` | `twidget_litehtml`: the litehtml fork a stage at a time, one module per C++ file (`litehtml/src`); so far `pixel_t`, the name table, strtod, `web_color`, `css_length`, the keyword tables, the CSS tokenizer and parser, stylesheets with their at-rules and media queries, selectors and their matching, style blocks (`style.cpp`: every property's declaration as the typed value the fork keeps, shorthands, `!important`, the declaration cache), gradients, the value parsers and math folding of `css_effects.cpp`, and the DOM: a document built from Gumbo's tree as the renderer builds one (the element classes, words and spaces, the limits, the source template and its holes, what each class takes from its attributes), the cascade (the master sheet, presentational attributes, the document's sheets under their media and the user sheet, `::before`/`::after` with their content and counters, `var()`, the CSS-wide keywords, style sharing, the selector budget, media changes and `:hover`/`:active` restyles) and every element's computed style (`css_properties.cpp`, fonts through the container, the form controls' look, text and SVG paints), the render tree (`render_item.cpp` and the classes' `init()`s: an arena of render items of every class, anonymous blocks and flex items, inlines split around blocks, the table fix-up's anonymous boxes, table grids with spans and the cell budget, `list_index`, each element's render items as weak references), block and inline layout (`render_item.cpp`'s `render` with the layout cache, its marks and the layout budget, `render_block`, the block and inline formatting contexts, floats and clearance, margins collapsing, line boxes with the container's text breaker, white space, `text-overflow: ellipsis`, bidi runs, `vertical-align`, replaced boxes, the document's size and scroll views; positioned placement, flex, grid and tables are later stages'), and SVG geometry over an `SvgElements` trait (`el_svg.cpp`: path data and its errors, arcs, shape outlines, `transform` attributes and CSS transforms, the CTM, view boxes and intrinsic sizes, a `<use>`'s placement and budget, gradient coordinates, path length and the point at a length) |
| `crates/litehtml/src/gumbo` | `twidget_litehtml::gumbo`: Gumbo, the HTML parser the fork parses with (`litehtml/src/gumbo`), one module per C file: the UTF-8 reader, character references with char_ref.c's Ragel machine, the tokenizer, tree construction and its limits |

| `oracle/` | the C++ the port is compared with: small programs built from twidget's sources, cases on stdin; `lhdump` answers for the litehtml fork, stage by stage, `svg_oracle` for el_svg.cpp's geometry, `events_oracle` for the renderer's input (renderer.cpp copied with probes at the input code's calls, `events_oracle.cmake`), `renderer_oracle` for the renderer (the same copy with probes at its calls into the document and stage 4.5's code, `renderer_oracle.cmake`; the shared mechanism in `probe.cmake` and `input_probes.cmake`) |
| `tools/gate.py` | the gate: oracle build, `cargo test` with the comparison required, clippy, rustfmt, `unsafe` report |
| `tools/unsafe_report.py` | `unsafe` in the workspace and in every crate it links; `#![forbid(unsafe_code)]` on every root |
| `tools/gen_gumbo_tables.py` | writes `gumbo/tables.rs` and `tag_enum.rs` from Gumbo's C sources (the tag tables and char_ref.c's machine); `tests/gumbo.rs` checks them against the C |

```sh
python -I tools/gate.py            # add --release to run the tests optimized too
cargo test                         # without the oracle: the comparison tests skip
TWIDGET_ORACLE_DIR=oracle/build/Release TWIDGET_ORACLE_REQUIRED=1 cargo test   # what the gate runs
```

The oracle needs a C++20 compiler, CMake (`cmake -S oracle -B oracle/build`) and widget-sandbox next to
twidget (`-DTWIDGET_WIDGET_SANDBOX=` elsewhere); the JSON tests read its files the same way
(`TWIDGET_WIDGET_SANDBOX`). On the Telegram
machines the toolchain is `ThirdParty/rust` (`RUSTUP_HOME`, `CARGO_HOME`); nothing here pins it.
