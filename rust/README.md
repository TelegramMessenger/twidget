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

| `oracle/` | the C++ the port is compared with: small programs built from twidget's sources, cases on stdin |
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
