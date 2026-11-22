# Ordinary widget replay benchmarks

Each file freezes one desktop widget session. Run each implementation in a fresh process on the same file, for example `twidget_replay article-card.twrec --source article-card --benchmark`. Every step must pass the normal replay checks before the program emits a timing report.

| Recording | Source in the widget-sandbox corpus | Steps | First nonempty paint, step / commands |
| --- | --- | ---: | ---: |
| article-card | `common/cards/03-article-digest.html` | 76 | 8 / 114 |
| poll | `common/interactive/03-poll.html` | 57 | 9 / 52 |
| long-article | `common/heavy/04-long-article.html` | 76 | 7 / 169 |
| canvas-chart | `generated/without-profile/16-make-a-bar-chart-of-the-top-6-programming.html` | 58 | 10 / 18 |
| canvas-equalizer | `htmlbubbles-ported/12-motion-lab/05-mo-bare.html` | 232 | 10 / 35 |
| joystick | `gestures/1-pairs/01-joystick-xy.html` | 107 | 10 / 44 |
| svg-line-chart | `compat/svg-line-chart.html` | 87 | 12 / 98 |
| swatch-grid | `common/heavy/05-swatch-grid.html` | 75 | 7 / 37 |
| calendar | `common/interactive/07-calendar.html` | 63 | 10 / 68 |
| notes-storage | `compat/notes-storage.html` | 54 | 10 / 44 |
| table-sort | `compat/table-sort.html` | 62 | 10 / 78 |
| theme | `compat/dark-mode.html` | 69 | 17 / 20 |

The capture script loads each widget at 320 logical pixels and 2× scale, asks gesture-claim questions, presses, moves, releases and hovers. It changes the width to 240 and back to 320, then switches to dark and light themes. The bar chart submits a 28-operation drawing batch during initial render. The music player's equalizer submits 60 further 25-operation redraws after first paint.

The recorder ran with twidget `7deb25e`, widget-sandbox `3b6960b`, nova-safe `a0d05ea` and `TW_RECORD_BACKEND=stub`. Each file embeds the original widget path, input script, compiler and platform. To capture a fresh version, build [twidget-record](../../../tools/record/CMakeLists.txt) with the stub backend, then run `twidget-record --out NEW.twrec PATH` with the corresponding corpus path. Capture uses a live clock. The checked-in files freeze its values and the backend's answers for repeatable comparisons.

The report gives nanoseconds spent driving both renderer protocol ends over the in-memory channel. Each step includes delivery and synchronous replies. File I/O, comparison against the recording, JSON reporting, execution of widget JavaScript, rasterization and screen presentation are outside the timing interval. `first_paint_step` is the one-based step of the first nonempty painted display list. `first_paint_ns` adds elapsed time for all renderer steps through that paint. The earlier empty paint is preserved. `frames` lists each frame's step, whether it painted and its command count, including idle frames. Both renderers receive the same recorded inputs, clock reads and backend answers on every pass. The replay checks emitted messages, gesture claims, selection and state, paint and damage, keyed-command correspondence and ordered backend calls.

These twelve sessions replay through the original C++ renderer at `7cfa295`, the C++ renderer at `7deb25e` and the Rust renderer at that revision. The older `../corpus-stub.twrec` first differs on the original renderer in the order two canvas fonts are released at teardown. The older native `../corpus.twrec` records the earlier font-release order and a gradient coordinate that differs by one float ULP on Apple arm64 with FP contraction disabled. Both older recordings remain separate parity fixtures.
