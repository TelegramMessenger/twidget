import AppKit
import IOSurface
import QuartzCore

final class WidgetCanvasPresenter {
    static let enabled = WidgetCanvasSurface.useSurfaces && ProcessInfo.processInfo.environment["TW_CANVAS_LAYER"] != "0"
    static let maxSurfaces = 3

    let layer = CALayer()
    private var surfaces: [IOSurfaceRef] = []
    private var shown: IOSurfaceRef?
    private var busy = false
    private var dirty = false
    private weak var canvas: WidgetCanvas?
    private(set) var presented = 0
    private(set) var skipped = 0

    init(canvas: WidgetCanvas) {
        self.canvas = canvas
        layer.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "frame": NSNull(), "hidden": NSNull()]
        layer.contentsGravity = .resize
        layer.isOpaque = false
        layer.name = "twidget-canvas"
    }

    func remove() {
        layer.removeFromSuperlayer()
        layer.contents = nil
        surfaces = []
        shown = nil
    }

    private func freeSurface(_ canvas: WidgetCanvas) -> IOSurfaceRef? {
        if let first = surfaces.first, IOSurfaceGetWidth(first) != canvas.width || IOSurfaceGetHeight(first) != canvas.height {
            surfaces = []
        }
        for surface in surfaces where surface !== shown && !IOSurfaceIsInUse(surface) {
            return surface
        }
        guard surfaces.count < (canvas.pixelCount > 1 << 20 ? 2 : WidgetCanvasPresenter.maxSurfaces), canvas.width > 0, canvas.height > 0 else {
            return nil
        }
        let properties: [CFString: Any] = [kIOSurfaceWidth: canvas.width, kIOSurfaceHeight: canvas.height, kIOSurfaceBytesPerElement: 4, kIOSurfacePixelFormat: 0x42475241]
        guard let made = IOSurfaceCreate(properties as CFDictionary) else {
            return nil
        }
        if let plist = CGColorSpace(name: CGColorSpace.sRGB)?.copyPropertyList() {
            IOSurfaceSetValue(made, kIOSurfaceColorSpace, plist)
        }
        surfaces.append(made)
        return made
    }

    private func show(_ surface: IOSurfaceRef) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer.contents = surface
        CATransaction.commit()
        shown = surface
        presented += 1
    }

    func presentNow() {
        guard let canvas = canvas, let storage = canvas.storage else {
            return
        }
        canvas.finishGPU()
        guard let target = freeSurface(canvas) else {
            return
        }
        IOSurfaceLock(target, [], nil)
        storage.lock()
        let rows = min(storage.height, IOSurfaceGetHeight(target))
        let bytes = min(storage.bytesPerRow, IOSurfaceGetBytesPerRow(target))
        let source = IOSurfaceGetBaseAddress(storage.surface), destination = IOSurfaceGetBaseAddress(target)
        let sourceRow = storage.bytesPerRow, destinationRow = IOSurfaceGetBytesPerRow(target)
        if sourceRow == destinationRow {
            memcpy(destination, source, sourceRow * rows)
        } else {
            for y in 0..<rows {
                memcpy(destination + y * destinationRow, source + y * sourceRow, bytes)
            }
        }
        storage.unlock()
        IOSurfaceUnlock(target, [], nil)
        show(target)
    }

    func present() {
        guard let canvas = canvas else {
            return
        }
        if busy {
            dirty = true
            return
        }
        #if canImport(Metal)
        if let gpu = canvas.backend as? WidgetCanvasMetal {
            guard let target = freeSurface(canvas) else {
                skipped += 1
                dirty = true
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.004) { [weak self] in
                    self?.retry()
                }
                return
            }
            busy = true
            dirty = false
            let started = gpu.present(canvas: canvas, to: target) { [weak self] in
                guard let self = self else {
                    return
                }
                self.busy = false
                if self.layer.superlayer != nil {
                    self.show(target)
                }
                if self.dirty {
                    self.present()
                }
            }
            if !started {
                busy = false
                presentNow()
            }
            return
        }
        #endif
        presentNow()
    }

    private func retry() {
        if dirty && !busy {
            present()
        }
    }

    func draw(into ctx: CGContext, frame: CGRect) {
        guard let image = canvas?.image else {
            return
        }
        ctx.saveGState()
        ctx.translateBy(x: frame.minX, y: frame.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.draw(image, in: CGRect(origin: .zero, size: frame.size))
        ctx.restoreGState()
    }
}
