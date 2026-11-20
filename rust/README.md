# twidget in safe Rust

The port of everything Telegram's widget helper runs to Rust without `unsafe`, compared with the C++ at
every stage. `PLAN.md` has the goal, the rules, the stages and where they stand.

| Path | What |
|---|---|
| `crates/wire` | `twidget_wire`: the paint values and their binary forms (`core/src/wire.cpp`) |
| `oracle/` | the C++ the port is compared with: small programs built from twidget's sources, cases on stdin |
| `tools/gate.py` | the gate: oracle build, `cargo test` with the comparison required, clippy, rustfmt, `unsafe` report |
| `tools/unsafe_report.py` | `unsafe` in the workspace and in every crate it links; `#![forbid(unsafe_code)]` on every root |

```sh
python -I tools/gate.py            # add --release to run the tests optimized too
cargo test                         # without the oracle: the comparison tests skip
TWIDGET_ORACLE_DIR=oracle/build/Release TWIDGET_ORACLE_REQUIRED=1 cargo test   # what the gate runs
```

The oracle needs only a C++20 compiler and CMake (`cmake -S oracle -B oracle/build`). On the Telegram
machines the toolchain is `ThirdParty/rust` (`RUSTUP_HOME`, `CARGO_HOME`); nothing here pins it.
