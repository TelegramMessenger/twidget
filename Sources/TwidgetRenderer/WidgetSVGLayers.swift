import Foundation
import QuartzCore
import LiteHTML

final class WidgetSVGLayers {
    static let enabled = ProcessInfo.processInfo.environment["TW_NO_SVG_LAYERS"] == nil
    static let streakNeeded = 3

    private(set) var ids: Set<Int> = []
    private(set) var frames: [Int: CGRect] = [:]
    private var streaks: [Int: Int] = [:]
    private var refused: Set<Int> = []
    private var containers: [Int: CALayer] = [:]
    private weak var host: CALayer?

    func observe(_ touched: Set<Int>) -> Bool {
        guard WidgetSVGLayers.enabled else {
            return false
        }
        var next: [Int: Int] = [:]
        for id in touched where !refused.contains(id) {
            next[id] = (streaks[id] ?? 0) + 1
        }
        for id in ids where next[id] == nil {
            next[id] = WidgetSVGLayers.streakNeeded
        }
        streaks = next
        let wanted = Set(streaks.filter { $0.value >= WidgetSVGLayers.streakNeeded }.keys)
        let added = !wanted.subtracting(ids).isEmpty
        ids.formUnion(wanted)
        return added
    }

    private var suspended = false

    func setSuspended(_ value: Bool) {
        guard value != suspended else {
            return
        }
        suspended = value
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for layer in containers.values {
            layer.isHidden = value
        }
        CATransaction.commit()
    }

    func covers(_ rect: CGRect) -> Bool {
        if suspended {
            return false
        }
        return frames.values.contains { $0.contains(rect) }
    }

    func reset() {
        for layer in containers.values {
            layer.removeFromSuperlayer()
        }
        containers.removeAll()
        frames.removeAll()
        ids.removeAll()
        streaks.removeAll()
    }

    func render(document: LHDocument, width: CGFloat, host: CALayer?, scale: CGFloat, dom: WidgetDOM, composited: Set<Int>) -> Bool {
        guard !ids.isEmpty else {
            return false
        }
        var dropped = false
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        if self.host !== host {
            self.host = host
            for layer in containers.values {
                host?.addSublayer(layer)
            }
        }
        for id in ids {
            var nested = false
            var ancestor = dom.node(id)?.parent
            while let node = ancestor, !nested {
                nested = composited.contains(node.id)
                ancestor = node.parent
            }
            guard !nested, !composited.contains(id), let shapes = document.svgShapes(attribute: WidgetDOM.idAttribute, value: String(id), maxWidth: width),
                  WidgetCompositor.unobscured(document, id, shapes.frame, clip: nil, ignoring: []) else {
                refused.insert(id)
                ids.remove(id)
                frames[id] = nil
                containers.removeValue(forKey: id)?.removeFromSuperlayer()
                dropped = true
                continue
            }
            let container: CALayer
            if let existing = containers[id] {
                container = existing
            } else {
                container = CALayer()
                container.masksToBounds = true
                host?.addSublayer(container)
                containers[id] = container
            }
            frames[id] = shapes.frame
            if container.frame != shapes.frame {
                container.frame = shapes.frame
            }
            update(container, shapes: shapes, scale: scale)
        }
        CATransaction.commit()
        return dropped
    }

    private func update(_ container: CALayer, shapes: LHSVGShapes, scale: CGFloat) {
        let origin = shapes.frame.origin
        var shift = CGAffineTransform(translationX: -origin.x, y: -origin.y)
        var layers: [CALayer] = []
        var existing = container.sublayers ?? []
        func take<T: CALayer>(_ type: T.Type) -> T {
            if let index = existing.firstIndex(where: { Swift.type(of: $0) == type }) {
                return existing.remove(at: index) as! T
            }
            let layer = T()
            layer.contentsScale = scale
            return layer
        }
        for shape in shapes.shapes {
            guard let path = shape.path.copy(using: &shift) else {
                continue
            }
            if let colors = shape.gradientColors, let locations = shape.gradientLocations {
                let gradient = take(CAGradientLayer.self)
                gradient.frame = container.bounds
                gradient.colors = colors
                gradient.locations = locations
                let size = container.bounds.size
                gradient.startPoint = CGPoint(x: (shape.gradientStart.x - origin.x) / max(size.width, 1), y: (shape.gradientStart.y - origin.y) / max(size.height, 1))
                gradient.endPoint = CGPoint(x: (shape.gradientEnd.x - origin.x) / max(size.width, 1), y: (shape.gradientEnd.y - origin.y) / max(size.height, 1))
                let mask = (gradient.mask as? CAShapeLayer) ?? CAShapeLayer()
                mask.contentsScale = scale
                mask.frame = container.bounds
                mask.path = path
                mask.fillRule = shape.evenOdd ? .evenOdd : .nonZero
                gradient.mask = mask
                layers.append(gradient)
            } else if let fill = shape.fillColor {
                let layer = take(CAShapeLayer.self)
                layer.frame = container.bounds
                layer.path = path
                layer.fillColor = fill
                layer.fillRule = shape.evenOdd ? .evenOdd : .nonZero
                layer.strokeColor = nil
                layers.append(layer)
            }
            if let stroke = shape.strokeColor {
                let layer = take(CAShapeLayer.self)
                layer.frame = container.bounds
                layer.path = path
                layer.fillColor = nil
                layer.strokeColor = stroke
                layer.lineWidth = shape.lineWidth
                layer.lineCap = shape.lineCap == 1 ? .round : shape.lineCap == 2 ? .square : .butt
                layer.lineJoin = shape.lineJoin == 1 ? .round : shape.lineJoin == 2 ? .bevel : .miter
                layer.miterLimit = shape.miterLimit
                layers.append(layer)
            }
        }
        if container.sublayers?.count != layers.count || zip(container.sublayers ?? [], layers).contains(where: { $0 !== $1 }) {
            container.sublayers = layers
        }
    }
}
