import QuartzCore
import Foundation

public enum WidgetLayerCommit {
    public static var enabled = ProcessInfo.processInfo.environment["TW_DEFERRED_STRUCTURE"] == "1"
    private static let lock = NSLock()
    private static var pending: [() -> Void] = []
    private static var scheduled = false
    public private(set) static var flushes = 0
    public private(set) static var applied = 0

    static func perform(_ block: @escaping () -> Void) {
        if !enabled || Thread.isMainThread {
            block()
            return
        }
        lock.lock()
        pending.append(block)
        let start = !scheduled
        scheduled = true
        lock.unlock()
        if start {
            DispatchQueue.main.async {
                flush()
            }
        }
    }

    public static func flush() {
        lock.lock()
        let blocks = pending
        pending.removeAll()
        scheduled = false
        lock.unlock()
        if blocks.isEmpty {
            return
        }
        flushes += 1
        applied += blocks.count
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for block in blocks {
            block()
        }
        CATransaction.commit()
    }
}

extension CALayer {
    func twAddSublayer(_ layer: CALayer) {
        WidgetLayerCommit.perform { self.addSublayer(layer) }
    }

    func twInsertSublayer(_ layer: CALayer, at index: UInt32) {
        WidgetLayerCommit.perform { self.insertSublayer(layer, at: min(index, UInt32(self.sublayers?.count ?? 0))) }
    }

    func twInsertSublayer(_ layer: CALayer, below sibling: CALayer?) {
        WidgetLayerCommit.perform {
            if let sibling = sibling, sibling.superlayer === self {
                self.insertSublayer(layer, below: sibling)
            } else {
                self.addSublayer(layer)
            }
        }
    }

    func twRemoveFromSuperlayer() {
        WidgetLayerCommit.perform { self.removeFromSuperlayer() }
    }
}

extension CALayer {
    var twSublayers: [CALayer]? {
        CATransaction.lock()
        defer { CATransaction.unlock() }
        return sublayers
    }
}
