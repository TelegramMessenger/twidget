# Ordinary widget replay benchmarks

Each file freezes one desktop widget session. Run each implementation in a fresh process on the same file, for example `twidget_replay article-card.twrec --source article-card --benchmark`. Every step must pass the normal replay checks before the program emits a timing report.

| Recording | Source in the widget-sandbox corpus | Steps | First nonempty paint, step / commands |
| --- | --- | ---: | ---: |
| article-card | `common/cards/03-article-digest.html` | 79 | 10 / 114 |
| poll | `common/interactive/03-poll.html` | 59 | 11 / 52 |
| long-article | `common/heavy/04-long-article.html` | 78 | 9 / 169 |
| canvas-chart | `generated/without-profile/16-make-a-bar-chart-of-the-top-6-programming.html` | 60 | 12 / 18 |
| canvas-equalizer | `htmlbubbles-ported/12-motion-lab/05-mo-bare.html` | 185 | 9 / 20 |
| joystick | `gestures/1-pairs/01-joystick-xy.html` | 102 | 12 / 44 |
| svg-line-chart | `compat/svg-line-chart.html` | 85 | 14 / 98 |
| swatch-grid | `common/heavy/05-swatch-grid.html` | 78 | 9 / 37 |
| calendar | `common/interactive/07-calendar.html` | 65 | 10 / 18 |
| notes-storage | `compat/notes-storage.html` | 56 | 12 / 44 |
| table-sort | `compat/table-sort.html` | 64 | 12 / 78 |
| theme | `compat/dark-mode.html` | 71 | 19 / 20 |

The capture script loads each widget at 320 logical pixels and 2× scale, after the host's `theme`, `surface`, `safeArea`, `deviceSafeArea` and `policy` messages. It asks gesture-claim questions, presses, moves, releases and hovers. It changes the width to 240 and back to 320, then switches to dark and light themes. The bar chart submits a 28-operation drawing batch during initial render. The music player's equalizer submits 44 redraws of 25 operations each after first paint.

The recorder ran on 2026-11-24 on Windows x64 (MSVC 19.44, Release) with twidget `cea5126`, widget-sandbox `830d380`, nova-safe `a0d05ea` and `TW_RECORD_BACKEND=stub`. The origin in each file names twidget `cea5126-dirty`: only the Rust port next to the C++ the recorder compiles was uncommitted. Each file embeds the original widget path, input script, compiler and platform. To capture a fresh version, build [twidget-record](../../../tools/record/CMakeLists.txt) with the stub backend, then run `twidget-record --keep --out NEW.twrec PATH` with the corresponding corpus path. Capture uses a live clock, so step and frame counts vary between captures. The checked-in files freeze its values and the backend's answers for repeatable comparisons.

The report gives nanoseconds spent driving both renderer protocol ends over the in-memory channel. Each step includes delivery and synchronous replies. File I/O, comparison against the recording, JSON reporting, execution of widget JavaScript, rasterization and screen presentation are outside the timing interval. `first_paint_step` is the one-based step of the first nonempty painted display list. `first_paint_ns` adds elapsed time for all renderer steps through that paint. The earlier empty paint is preserved. `frames` lists each frame's step, whether it painted and its command count, including idle frames. Both renderers receive the same recorded inputs, clock reads and backend answers on every pass. The replay checks emitted messages, gesture claims, selection and state, paint and damage, keyed-command correspondence and ordered backend calls.

These twelve sessions and `../corpus-stub.twrec` were captured again after the bot's profile page (`cea5126`) gave every renderer's root the place's variables, including a bubble's four `--tg-safe-area-inset-*: 0px`. litehtml's name table, which the replay checks before every step, then holds four more names of 102 bytes. The sessions replay through the C++ renderer from `cea5126` on and the Rust renderer at the revision that commits them. An earlier renderer, including the original C++ at `7cfa295`, first differs at step 2 on those name counts. The previous captures of these twelve, made at twidget `7deb25e`, remain in twidget `cea5126` with the renderers that replay them: the original C++ at `7cfa295`, the C++ at `7deb25e` and the Rust renderer at that revision. The stub's previous capture, made at `34da900`, remains there too. The older native `../corpus.twrec` records the earlier name counts, the earlier font-release order and a gradient coordinate that differs by one float ULP on Apple arm64 with FP contraction disabled. It remains a separate parity fixture until it is recorded again on macOS.
