# Comparing the Rust widget helper with C++

Run the implementations on the same machine. The original C++ control is the twidget revision from Desktop immediately before the Rust renderer was integrated, `7cfa2957b044bae2e3885037fcf48e24a168234f`. Build both controls from this checkout's benchmark sources so they use the same clock, input and verification code. The C++ renderer itself comes from the historical checkout.

## Build the renderer benchmarks

Place the Desktop-pinned `widget-sandbox` and `nova-safe` checkouts next to this twidget checkout. These commands assume a single-configuration Release build, Ninja, a C++20 toolchain and the Rust toolchain pinned in Desktop's prepare script. Run from the twidget root. On macOS set `MACOSX_DEPLOYMENT_TARGET=12.0` in the environment first.

```sh
git worktree add --detach ../twidget-original 7cfa2957b044bae2e3885037fcf48e24a168234f
cmake -S rust/oracle/bench -B build/bench-original -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INTERPROCEDURAL_OPTIMIZATION=OFF \
    -DTWIDGET_BENCH_CPP_ROOT="$PWD/../twidget-original" \
    -DTWIDGET_WIDGET_SANDBOX="$PWD/../widget-sandbox"
cmake --build build/bench-original --target bench_oracle
cmake -S tests/replay_bench -B build/replay-original -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INTERPROCEDURAL_OPTIMIZATION=OFF \
    -DTWIDGET_REPLAY_CORE="$PWD/../twidget-original" \
    -DTWIDGET_WIDGET_SANDBOX="$PWD/../widget-sandbox" \
    -DTWIDGET_LITEHTML_NO_CORE_FOUNDATION=ON
cmake --build build/replay-original --target twidget_replay
(cd rust && CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true \
    cargo build --locked --release --features metered -p twidget_bench -p twidget_replay)
```

The renderer runs with overflow checks and the helper's metered allocator. FP contraction is disabled in the C++ controls to match Rust. Both replays run with the recorded backend responses. On macOS the portable text and case conversion mode matches these recordings.

## Compare workloads

```sh
python3 rust/tools/benchmark.py \
    --cpp-bench build/bench-original/bench_oracle --rust-bench rust/target/release/twidget_bench \
    --cpp-replay build/replay-original/twidget_replay --rust-replay rust/target/release/twidget_replay \
    --frames 240 --samples 9 --output renderer-results.json
```

The steady-frame workloads cover mixed DOM style changes, transforms, many moving nodes, two sizes of compositor animation, two forms of dragging and canvas. Each case first compares the complete untimed trace and checks the work counters on every timing run. For the historical CSS animation case the C++ trace orders the initial independent animation-start messages by distinct target ID. Their complete contents and every subsequent output still have to match.

Real sessions cover initial layout, measurement, gestures and pointer handling, changes of width and theme, transforms, text, SVG and both static and repeatedly painted canvas. Each timed run also checks its outputs against its recording. The first paint is the first nonempty display list. The remainder includes all later work through destruction. The active-repaint measurement averages only later frames that paint a nonempty list. The full session sums all timed steps. These timings include the renderer and the loopback platform end. The recorded backend answers remove OS rasterization and widget JS execution from this comparison. [Recordings and capture method](../../tests/recordings/ordinary/README.md)

The driver warms both executables, alternates their execution order and records all measured pairs. Ratios are Rust time divided by the paired C++ time. It reports their median and median absolute deviation, executable hashes and recording or trace hashes. The 2× check covers the whole synthetic frame and the first paint, active repaint, remainder and total of each recording. The JSON also retains individual stage costs for diagnosis. A partially written suite has `complete` set to false.

## Measure the shipping helper boundary on macOS

Build and sign two native Release helpers for the same Desktop build version. Use the Desktop commit just before the Rust integration for the C++ build, the candidate helper for the Rust build, the same WidgetJS bundles for both, and disable LTO in both Rust and native builds. Preserve the shipping allocator, overflow checks, sandbox and IPC.

```sh
python3 rust/tools/benchmark_helper.py \
    --cpp /path/to/original/TelegramWidgetHelper \
    --rust /path/to/candidate/TelegramWidgetHelper \
    --bundles /path/to/WidgetJS --pairs 20 --updates 32 --output helper-results.json
```

This comparison starts each helper in its own process and measures the time to complete its hosting handshake, deliver its first useful painted frame and repaint after changing data messages. The workloads are a badge, a DOM bar chart and a Canvas 2D chart. An untimed observer verifies the validated DOM operations. Timed runs request frames on the regular host notifications and verify paint bytes, canvas commands and deltas. First-paint time includes startup and ends on receipt of the frame by the host, before display vsync. To compare a third binary, pass the released Rust helper as `--rust` and the optimized helper as `--candidate`. The harness rotates through all six execution orders.
