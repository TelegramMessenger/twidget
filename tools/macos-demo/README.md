# macOS demo

widget-sandbox's corpus in a feed of bubbles on macOS, rendered by TwidgetCore, the portable C++
renderer that also drives the Windows spike (Direct2D), the Qt demo (QPainter), Android and the web.
It is the same pipeline as `LiteHTMLWidgetRenderer` with the Swift taken out:

| Piece | Here | In the Swift renderer |
|---|---|---|
| Widget JS, validator, host | widget-sandbox's C++ `WidgetHost` (`packages/desktop`) | `WidgetHost.swift` |
| DOM, CSS, layout, events, animations, canvas | TwidgetCore (`Sources/TwidgetCore`) | `TwidgetRenderer` + `LHDocument` |
| Painting | `CGBackend` (`platform/apple/coregraphics.cpp`) | `LHContainer` |
| Compositor layers | the core's `Composition`, played on CALayers by `widget_view.mm` | `WidgetCompositor` |

Each widget's document is painted into an image on its own clock thread. Transform and opacity
animations become CALayers whose keyframe tracks Core Animation plays on the render server, as the
Windows spike plays them on Windows.UI.Composition. A frame's document image and its layers land in
one `CATransaction`.

## Build

```sh
cmake -S tools/macos-demo -B build-macos -G Ninja      # or -G Xcode
cmake --build build-macos
open build-macos/twidget-macos-demo.app                 # or pass a corpus folder or .html file
```

widget-sandbox must be checked out next to twidget, or its path given in `TWIDGET_WIDGET_SANDBOX`.

## State

**Never compiled.** It was written on Windows. The C++ (`mac_renderer.cpp`) was type-checked against
stub Apple headers; the Objective-C++ (`widget_view.mm`, `main.mm`) was not checked at all. Expect a
few fixes on the first build. Parts worth a look when something is off:

- **Coordinates:** the view hosts its own layer tree (y up). A `stage` layer's `sublayerTransform`
  turns document coordinates (y down) into the view's. Each layer's transform is the track's matrix
  after `layer_transform`'s map from the layer's own space, so content images stay upright.
- **Clips:** a clipped layer sits in a container masked by a `CAShapeLayer` with per-corner radii.
- **Hit testing:** the core samples the same tracks on its own clock (`Renderer::clock()`).
  Core Animation starts them a dispatch later, so a fast-moving layer can be hit a frame off.
