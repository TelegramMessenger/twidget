import AppKit
import LiteHTML

public enum WidgetCanvasImages {
    public static var fallback: ((String) -> NSImage?)?

    private final class Weak {
        weak var canvas: WidgetCanvas?
        init(_ canvas: WidgetCanvas) {
            self.canvas = canvas
        }
    }

    private static let lock = NSLock()
    private static var canvases: [String: Weak] = [:]
    private static var installed = false

    static func install() {
        lock.lock()
        defer { lock.unlock() }
        if installed {
            return
        }
        installed = true
        let previous = LHImages.provider
        LHImages.setProvider { url in
            if url.hasPrefix("twcanvas:") {
                return WidgetCanvasImages.image(url)
            }
            return previous?(url) ?? WidgetCanvasImages.fallback?(url)
        }
    }

    static func register(_ key: String, _ canvas: WidgetCanvas) {
        lock.lock()
        canvases[key] = Weak(canvas)
        lock.unlock()
    }

    static func unregister(_ key: String) {
        lock.lock()
        canvases[key] = nil
        lock.unlock()
    }

    private static func image(_ url: String) -> NSImage? {
        let body = url.dropFirst("twcanvas:".count)
        guard let dash = body.lastIndex(of: "-"), let version = Int(body[body.index(after: dash)...]) else {
            return nil
        }
        let key = String(body[..<dash])
        lock.lock()
        let canvas = canvases[key]?.canvas
        lock.unlock()
        guard let target = canvas, target.version == version, let image = target.image else {
            return nil
        }
        return NSImage(cgImage: image, size: NSSize(width: target.width, height: target.height))
    }
}
