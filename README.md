# twidget

Native renderers for Telegram's HTML widgets, built on a heavily optimized fork of [litehtml](https://github.com/litehtml/litehtml).

Widgets run on [widget-sandbox](https://github.com/TelegramMessenger/widget-sandbox), one framework shared by every client:
- **The shared parts.** A hostile sandbox runs the widget as written, and a validator checks every DOM operation against one policy.
- **The per-client parts.** Each client implements the engine glue and a renderer.

This repository holds renderers: validated operations come in, events and layout answers go out, and pixels come from litehtml instead of a WebView. They speak widget-sandbox's renderer protocol, so any widget-sandbox host can use them. The sandbox, the validator and the JavaScript bundles are widget-sandbox's and are not part of this repository.

**TwidgetCore** is the cross-platform renderer: portable C++20, with backends for Windows (Direct2D), Android, the web, Qt and Apple (iOS and macOS), built with CMake from the repository root. **The Apple renderer** is a separate Swift package that powers the HTML widgets in Telegram for macOS. Both are built on the same litehtml fork.

## Layout

| Folder | What it holds | Used by |
|---|---|---|
| `litehtml/` | The litehtml fork, with its Gumbo parser | TwidgetCore and the Apple renderer |
| `core/` | TwidgetCore: the protocol, layout on the fork, animations, canvas, compositor layers, the paint limit. `core/include/twidget_litehtml.h` is its C ABI | Every backend in `platforms/` |
| `platforms/` | One backend per platform: `windows` (DirectWrite, Direct2D), `qt` (QPainter), `apple` (CoreGraphics), `android` (JNI and Kotlin), `web` (WebAssembly and TypeScript) | Clients, the demos and the tools |
| `tests/` | TwidgetCore's checks, run by `ctest`, and portable checks of the Apple renderer's `lhx` helpers | |
| `tools/` | `conformance` and `stress` run widget-sandbox's conformance set and stress kit on Windows; `core-run` turns a message log into a PNG on macOS; `build-apple.sh` builds the iOS and macOS static library | |
| `demos/` | TwidgetCore in a feed of bubbles: `qt`, `macos` and `android` | |
| `apple/` | The Apple renderer, the Swift package `Package.swift` describes: its sources, tests, Lab and examples | Telegram for macOS |
| `docs/` | The documentation below | |

`Package.swift` stays at the root so that SwiftPM finds the package at the repository's URL.

## Building

widget-sandbox has to be checked out next to twidget (`../widget-sandbox`), at release `v0.6.0` with its submodules, or its path given in `TWIDGET_WIDGET_SANDBOX`.

```sh
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
ctest --test-dir build
```

This builds `twidget::litehtml`, `twidget::core` and the platform's backend. Android and Emscripten builds go through their toolchain files, and `-DTWIDGET_QT=ON` adds the Qt backend.

## Documentation

- [docs/TWIDGET_CORE.md](docs/TWIDGET_CORE.md): what TwidgetCore does, what a platform implements, each backend, the build, the demos and the tools.
- [docs/CONFORMANCE.md](docs/CONFORMANCE.md): where TwidgetCore stands against widget-sandbox's conformance set.
- [apple/README.md](apple/README.md): the Apple renderer, its standalone scripted documents, three.js on Metal and the Lab.
- [docs/WIDGET_ASSETS.md](docs/WIDGET_ASSETS.md): images and the `icon:` set of standalone documents.

## License

BSD 3-Clause, see `LICENSE`. Bundled third-party code (litehtml, Gumbo, three.js and Phosphor Icons) keeps its own license, listed in `THIRD_PARTY_NOTICES.md`.
