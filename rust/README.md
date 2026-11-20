# twidget in safe Rust

The port of everything Telegram's widget helper runs to Rust without `unsafe`, compared with the C++ at
every stage. `PLAN.md` has the goal, the rules, the stages and where they stand.

| Path | What |
|---|---|
| `crates/wire` | `twidget_wire`: the paint values and their binary forms (`core/src/wire.cpp`) |
| `crates/paint` | `twidget_paint`: heavy boxes, the paint limit and its ledger (`core/src/paint_policy`) |
| `crates/remote` | `twidget_remote`: the messages between the renderer end and the platform end (`core/src/remote.cpp`) |

| `crates/css` | `twidget_css`: TwidgetCore's small CSS helpers (`core/src/css.cpp`) |
| `crates/cstd` | `twidget_cstd`: the C library behaviours the C++ leans on (`strtod`, `atof`, `atoi`, `%.*f`) |
| `crates/compositor` | `twidget_compositor`: layers and their baked tracks, paint digests, damage, inert clips (`core/src/compositor.cpp`) |
| `crates/base64` | `twidget_base64`: the base64 of canvas pixels (`core/src/encode.cpp`) |
| `crates/canvas` | `twidget_canvas`: the 2D canvas interpreter, commands in and canvas ops out through a `CanvasHost` (`core/src/canvas.cpp`) |
| `crates/animations` | `twidget_animations`: the animator, CSS animations, transitions and Web Animations: declarations and times in, style overrides and events out (`core/src/animations.cpp`) |
| `crates/litehtml` | `twidget_litehtml`: the litehtml fork a stage at a time, one module per C++ file (`litehtml/src`); so far `pixel_t`, the name table, strtod, `web_color`, `css_length` and the keyword tables |

| `oracle/` | the C++ the port is compared with: small programs built from twidget's sources, cases on stdin; `lhdump` answers for the litehtml fork, stage by stage |
| `tools/gate.py` | the gate: oracle build, `cargo test` with the comparison required, clippy, rustfmt, `unsafe` report |
| `tools/unsafe_report.py` | `unsafe` in the workspace and in every crate it links; `#![forbid(unsafe_code)]` on every root |

```sh
python -I tools/gate.py            # add --release to run the tests optimized too
cargo test                         # without the oracle: the comparison tests skip
TWIDGET_ORACLE_DIR=oracle/build/Release TWIDGET_ORACLE_REQUIRED=1 cargo test   # what the gate runs
```

The oracle needs a C++20 compiler, CMake (`cmake -S oracle -B oracle/build`) and widget-sandbox next to
twidget (`-DTWIDGET_WIDGET_SANDBOX=` elsewhere); the JSON tests read its files the same way
(`TWIDGET_WIDGET_SANDBOX`). On the Telegram
machines the toolchain is `ThirdParty/rust` (`RUSTUP_HOME`, `CARGO_HOME`); nothing here pins it.
