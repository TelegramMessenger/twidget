import AppKit
import LiteHTML
import QuartzCore

struct WidgetScrollGeometry: Equatable {
    var viewport: CGRect
    var content: CGSize
    var radii: [CGFloat]

    var maxOffset: CGPoint {
        return CGPoint(x: max(0, content.width - viewport.width), y: max(0, content.height - viewport.height))
    }

    func clamp(_ offset: CGPoint) -> CGPoint {
        let limit = maxOffset
        return CGPoint(x: min(max(0, offset.x), limit.x), y: min(max(0, offset.y), limit.y))
    }

    static func rubberBand(_ overshoot: CGFloat, dimension: CGFloat) -> CGFloat {
        guard dimension > 0 else {
            return 0
        }
        let magnitude = (1 - 1 / (abs(overshoot) * 0.55 / dimension + 1)) * dimension
        return overshoot < 0 ? -magnitude : magnitude
    }

    func banded(_ raw: CGPoint) -> CGPoint {
        let clamped = clamp(raw)
        return CGPoint(x: clamped.x + WidgetScrollGeometry.rubberBand(raw.x - clamped.x, dimension: viewport.width), y: clamped.y + WidgetScrollGeometry.rubberBand(raw.y - clamped.y, dimension: viewport.height))
    }
}

final class WidgetScrollLayer {
    let id: Int
    let clip = CALayer()
    let content = CALayer()
    let thumb = CALayer()
    private(set) var geometry: WidgetScrollGeometry
    private var tiles: [Int: (layer: CALayer, signature: Int)] = [:]
    private var pending: Set<Int> = []
    private var signature = 0
    private var generation = 0
    static let tileSize = CGSize(width: 512, height: 256)
    static let queue = DispatchQueue(label: "twidget.scroll.raster", qos: .userInitiated)
    private(set) var rasterCount = 0

    init(id: Int, geometry: WidgetScrollGeometry) {
        self.id = id
        self.geometry = geometry
        clip.anchorPoint = .zero
        clip.masksToBounds = true
        clip.name = "tw.scroll"
        content.anchorPoint = .zero
        content.name = "tw.scroll.content"
        clip.addSublayer(content)
        thumb.name = "tw.scroll.thumb"
        thumb.backgroundColor = CGColor(gray: 0, alpha: 0.45)
        thumb.borderColor = CGColor(gray: 1, alpha: 0.3)
        thumb.borderWidth = 0.5
        thumb.cornerRadius = 3
        thumb.opacity = 0
        thumb.actions = ["position": NSNull(), "bounds": NSNull(), "opacity": NSNull()]
        clip.addSublayer(thumb)
        apply(geometry)
    }

    func apply(_ geometry: WidgetScrollGeometry) {
        let resized = geometry.viewport.size != self.geometry.viewport.size || geometry.content != self.geometry.content
        self.geometry = geometry
        clip.frame = geometry.viewport
        content.frame = CGRect(origin: .zero, size: geometry.viewport.size)
        if geometry.radii.contains(where: { $0 > 0 }) {
            let mask = (clip.mask as? CAShapeLayer) ?? CAShapeLayer()
            mask.frame = clip.bounds
            mask.path = WidgetCompositor.roundedPath(clip.bounds, geometry.radii)
            clip.mask = mask
        } else {
            clip.mask = nil
        }
        if resized {
            invalidate()
        }
    }

    var offset: CGPoint {
        return content.bounds.origin
    }

    func show(_ offset: CGPoint) {
        content.bounds.origin = offset
    }

    func flashIndicator(_ offset: CGPoint) {
        let viewport = geometry.viewport.size
        let total = max(viewport.height, geometry.content.height)
        guard geometry.maxOffset.y > 0.5, total > 0 else {
            return
        }
        let inset: CGFloat = 2
        let track = viewport.height - inset * 2
        var length = max(24, track * viewport.height / total)
        var position = inset + (track - length) * min(1, max(0, offset.y / geometry.maxOffset.y))
        if offset.y < 0 {
            length = max(8, length + offset.y)
            position = inset
        } else if offset.y > geometry.maxOffset.y {
            length = max(8, length - (offset.y - geometry.maxOffset.y))
            position = viewport.height - inset - length
        }
        thumb.frame = CGRect(x: viewport.width - 6 - inset, y: position, width: 6, height: length)
        thumb.removeAnimation(forKey: "tw.fade")
        thumb.opacity = 1
        let fade = CABasicAnimation(keyPath: "opacity")
        fade.fromValue = 1
        fade.toValue = 0
        fade.beginTime = thumb.convertTime(CACurrentMediaTime(), from: nil) + 0.9
        fade.duration = 0.25
        fade.fillMode = .forwards
        fade.isRemovedOnCompletion = false
        thumb.add(fade, forKey: "tw.fade")
    }

    func settle(to target: CGPoint) {
        let from = content.presentation()?.bounds.origin ?? content.bounds.origin
        content.bounds.origin = target
        guard from != target else {
            return
        }
        let animation = CASpringAnimation(keyPath: "bounds.origin")
        animation.fromValue = NSValue(point: from)
        animation.toValue = NSValue(point: target)
        animation.mass = 1
        animation.stiffness = 210
        animation.damping = 29
        animation.duration = animation.settlingDuration
        content.add(animation, forKey: "tw.settle")
    }

    func invalidate() {
        generation += 1
        for (index, tile) in tiles {
            tiles[index] = (tile.layer, 0)
        }
    }

    func update(signature: Int) {
        if signature != self.signature {
            self.signature = signature
            invalidate()
        }
    }

    private func columns() -> Int {
        return max(1, Int(ceil(max(geometry.viewport.width, geometry.content.width) / WidgetScrollLayer.tileSize.width)))
    }

    private func rect(_ index: Int) -> CGRect {
        let columns = self.columns()
        let size = WidgetScrollLayer.tileSize
        let width = max(geometry.viewport.width, geometry.content.width), height = max(geometry.viewport.height, geometry.content.height)
        let x = CGFloat(index % columns) * size.width, y = CGFloat(index / columns) * size.height
        return CGRect(x: x, y: y, width: min(size.width, width - x), height: min(size.height, height - y))
    }

    func ensureTiles(document: LHDocument, scale: CGFloat, attribute: String, velocity: CGFloat = 0) {
        let columns = self.columns()
        let size = WidgetScrollLayer.tileSize
        let visible = CGRect(origin: geometry.clamp(offset), size: geometry.viewport.size)
        let ahead = visible.insetBy(dx: -geometry.viewport.width * 0.5, dy: -max(geometry.viewport.height, 256)).offsetBy(dx: 0, dy: velocity)
        let keep = visible.insetBy(dx: -geometry.viewport.width * 2, dy: -geometry.viewport.height * 3)
        let width = max(geometry.viewport.width, geometry.content.width), height = max(geometry.viewport.height, geometry.content.height)
        let rows = max(1, Int(ceil(height / size.height)))
        for index in Array(tiles.keys) where !rect(index).intersects(keep) {
            tiles[index]?.layer.removeFromSuperlayer()
            tiles[index] = nil
        }
        let wanted = ahead.intersection(CGRect(x: 0, y: 0, width: width, height: height))
        guard !wanted.isNull else {
            return
        }
        let firstRow = max(0, Int(floor(wanted.minY / size.height))), lastRow = min(rows - 1, Int(floor((wanted.maxY - 0.001) / size.height)))
        let firstColumn = max(0, Int(floor(wanted.minX / size.width))), lastColumn = min(columns - 1, Int(floor((wanted.maxX - 0.001) / size.width)))
        guard firstRow <= lastRow, firstColumn <= lastColumn else {
            return
        }
        var order: [Int] = []
        for row in firstRow...lastRow {
            for column in firstColumn...lastColumn {
                order.append(row * columns + column)
            }
        }
        order.sort { abs(rect($0).midY - visible.midY) < abs(rect($1).midY - visible.midY) }
        for index in order where tiles[index]?.signature != signature && !pending.contains(index) {
            let tileRect = rect(index)
            guard tileRect.width > 0, tileRect.height > 0 else {
                continue
            }
            pending.insert(index)
            let generation = self.generation, signature = self.signature, value = String(id)
            let layer = tiles[index]?.layer ?? makeTile(tileRect)
            if tiles[index] == nil {
                tiles[index] = (layer, 0)
            }
            WidgetScrollLayer.queue.async { [weak self] in
                let image = document.copyScrollContent(attribute: attribute, value: value, rect: tileRect, scale: scale)
                DispatchQueue.main.async {
                    guard let self = self else {
                        return
                    }
                    self.pending.remove(index)
                    guard generation == self.generation, let tile = self.tiles[index], tile.layer === layer else {
                        return
                    }
                    CATransaction.begin()
                    CATransaction.setDisableActions(true)
                    layer.frame = tileRect
                    layer.contentsScale = scale
                    layer.contents = image
                    CATransaction.commit()
                    self.tiles[index] = (layer, signature)
                    self.rasterCount += 1
                }
            }
        }
    }

    private func makeTile(_ rect: CGRect) -> CALayer {
        let layer = CALayer()
        layer.anchorPoint = .zero
        layer.frame = rect
        layer.contentsGravity = .topLeft
        layer.actions = ["contents": NSNull(), "position": NSNull(), "bounds": NSNull()]
        content.addSublayer(layer)
        return layer
    }

    var tileCount: Int {
        return tiles.count
    }

    func draw(into ctx: CGContext, document: LHDocument, scale: CGFloat) {
        let shown = geometry.clamp(offset)
        guard let image = document.copyScrollContent(attribute: WidgetDOM.idAttribute, value: String(id), rect: CGRect(origin: shown, size: geometry.viewport.size), scale: scale) else {
            return
        }
        ctx.saveGState()
        ctx.addPath(WidgetCompositor.roundedPath(geometry.viewport, geometry.radii))
        ctx.clip()
        ctx.translateBy(x: geometry.viewport.minX, y: geometry.viewport.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.draw(image, in: CGRect(origin: .zero, size: geometry.viewport.size))
        ctx.restoreGState()
    }
}

final class WidgetScrolling {
    private(set) var layers: [Int: WidgetScrollLayer] = [:]
    private(set) var offsets: [Int: CGPoint] = [:]
    private(set) var geometries: [Int: WidgetScrollGeometry] = [:]
    private weak var host: CALayer?
    static let enabled = ProcessInfo.processInfo.environment["TW_NO_SCROLL_LAYERS"] == nil

    struct Gesture {
        var id: Int?
        var decided = false
        var raw = CGPoint.zero
    }

    private var gesture: Gesture?

    var promotedIDs: Set<Int> {
        return Set(layers.keys)
    }

    func attach(_ host: CALayer?) {
        if self.host === host {
            return
        }
        self.host = host
        for layer in layers.values {
            host?.addSublayer(layer.clip)
        }
    }

    func reset() {
        for layer in layers.values {
            layer.clip.removeFromSuperlayer()
        }
        layers.removeAll()
    }

    func update(document: LHDocument, dom: WidgetDOM, animated: Set<Int>, promoted: Set<Int>, styleKey: Int, scale: CGFloat) -> Bool {
        var found: [Int: WidgetScrollGeometry] = [:]
        for (key, info) in document.scrollers(byAttribute: WidgetDOM.idAttribute) {
            guard let id = Int(key), let viewport = (info["viewport"] as? NSValue)?.rectValue, let content = (info["content"] as? NSValue)?.sizeValue else {
                continue
            }
            let radii = ((info["radii"] as? [NSNumber]) ?? []).map { CGFloat($0.doubleValue) }
            found[id] = WidgetScrollGeometry(viewport: viewport, content: content, radii: radii.count == 8 ? radii : [CGFloat](repeating: 0, count: 8))
        }
        geometries = found
        for (id, offset) in offsets {
            if found[id] == nil {
                offsets[id] = nil
            } else {
                offsets[id] = document.setScrollOffset(offset, attribute: WidgetDOM.idAttribute, value: String(id))
            }
        }
        var wanted: [Int: WidgetScrollGeometry] = [:]
        if WidgetScrolling.enabled, let host = host {
            _ = host
            for (id, geometry) in found where geometry.maxOffset.y > 0.5 || geometry.maxOffset.x > 0.5 {
                guard let node = dom.node(id), geometry.viewport.width > 0, geometry.viewport.height > 0, geometry.viewport.width * geometry.viewport.height < 4_000_000 else {
                    continue
                }
                var walk = node.parent
                var nested = false
                while let parent = walk {
                    if promoted.contains(dom.publicId(parent)) || found[dom.publicId(parent)] != nil {
                        nested = true
                        break
                    }
                    walk = parent.parent
                }
                if nested || promoted.contains(id) || animated.contains(where: { WidgetScrolling.isDescendant($0, of: node, dom: dom) }) || promoted.contains(where: { WidgetScrolling.isDescendant($0, of: node, dom: dom) }) {
                    continue
                }
                guard WidgetCompositor.contentSignature(id, dom: dom, box: geometry.viewport, scale: scale, styleKey: styleKey) != nil else {
                    continue
                }
                guard document.isElement(attribute: WidgetDOM.idAttribute, value: String(id), unobscuredIn: geometry.viewport, ignoring: Set(promoted.map { String($0) }), clipped: false) else {
                    continue
                }
                wanted[id] = geometry
            }
        }
        let changed = Set(wanted.keys) != Set(layers.keys)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for (id, layer) in layers where wanted[id] == nil {
            layer.clip.removeFromSuperlayer()
            layers[id] = nil
        }
        for (id, geometry) in wanted {
            let layer = layers[id] ?? WidgetScrollLayer(id: id, geometry: geometry)
            if layers[id] == nil {
                layers[id] = layer
                host?.addSublayer(layer.clip)
            }
            layer.apply(geometry)
            layer.update(signature: WidgetCompositor.contentSignature(id, dom: dom, box: CGRect(origin: .zero, size: geometry.content), scale: scale, styleKey: styleKey) ?? 0)
            if layer.content.animation(forKey: "tw.settle") == nil && gesture?.id != id {
                layer.show(offsets[id] ?? .zero)
            }
            layer.ensureTiles(document: document, scale: scale, attribute: WidgetDOM.idAttribute)
        }
        CATransaction.commit()
        return changed
    }

    private static func isDescendant(_ id: Int, of node: WidgetNode, dom: WidgetDOM) -> Bool {
        guard var walk = dom.node(id) else {
            return false
        }
        while let parent = walk.parent {
            if parent === node {
                return true
            }
            walk = parent
        }
        return false
    }

    func scroller(at point: CGPoint, delta: CGPoint) -> Int? {
        let candidates = geometries.filter { $0.value.viewport.contains(point) && ($0.value.maxOffset.y > 0.5 || $0.value.maxOffset.x > 0.5) }
        for (id, geometry) in candidates.sorted(by: { $0.value.viewport.width * $0.value.viewport.height < $1.value.viewport.width * $1.value.viewport.height }) {
            let offset = offsets[id] ?? .zero
            let vertical = abs(delta.y) >= abs(delta.x)
            if vertical {
                if delta.y > 0 && offset.y > 0.5 || delta.y < 0 && offset.y < geometry.maxOffset.y - 0.5 {
                    return id
                }
            } else if delta.x > 0 && offset.x > 0.5 || delta.x < 0 && offset.x < geometry.maxOffset.x - 0.5 {
                return id
            }
        }
        return nil
    }

    func offset(_ id: Int) -> CGPoint {
        return offsets[id] ?? .zero
    }

    enum Outcome {
        case ignored
        case moved(Int)
        case unchanged
    }

    func scroll(event: NSEvent, at point: CGPoint, document: LHDocument?, scale: CGFloat, attribute: String) -> Outcome {
        let precise = event.hasPreciseScrollingDeltas
        let unit: CGFloat = precise ? 1 : 16
        let delta = CGPoint(x: event.scrollingDeltaX * unit, y: event.scrollingDeltaY * unit)
        let phase = event.phase, momentum = event.momentumPhase
        let phased = !phase.isEmpty || !momentum.isEmpty
        if phase.contains(.began) || phase.contains(.mayBegin) {
            gesture = Gesture()
        } else if phased && gesture == nil {
            gesture = Gesture()
        }
        let finished = momentum.contains(.ended) || momentum.contains(.cancelled) || phase.contains(.cancelled)
        defer {
            if finished {
                gesture = nil
            }
        }
        let id: Int
        if phased {
            if gesture?.decided == false {
                guard delta != .zero else {
                    return .unchanged
                }
                let target = scroller(at: point, delta: delta)
                gesture?.decided = true
                gesture?.id = target
                gesture?.raw = target.flatMap { offsets[$0] } ?? .zero
            }
            guard let locked = gesture?.id else {
                return .ignored
            }
            id = locked
        } else {
            guard let found = scroller(at: point, delta: delta) else {
                return .ignored
            }
            id = found
        }
        guard let geometry = geometries[id] else {
            gesture?.id = nil
            return .ignored
        }
        let current = offsets[id] ?? .zero
        let promoted = layers[id] != nil
        var raw: CGPoint
        if phased && precise {
            raw = CGPoint(x: (gesture?.raw.x ?? current.x) - delta.x, y: (gesture?.raw.y ?? current.y) - delta.y)
            if geometry.maxOffset.x <= 0.5 {
                raw.x = 0
            }
            if geometry.maxOffset.y <= 0.5 {
                raw.y = 0
            }
            if !promoted {
                raw = geometry.clamp(raw)
            }
            gesture?.raw = raw
        } else {
            raw = geometry.clamp(CGPoint(x: current.x - delta.x, y: current.y - delta.y))
        }
        let clamped = geometry.clamp(raw)
        let released = (phase.contains(.ended) || phase.contains(.cancelled)) && momentum.isEmpty || finished
        offsets[id] = document?.setScrollOffset(clamped, attribute: attribute, value: String(id)) ?? clamped
        if let layer = layers[id] {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            layer.content.removeAnimation(forKey: "tw.settle")
            layer.show(geometry.banded(raw))
            layer.flashIndicator(geometry.banded(raw))
            if released {
                gesture?.raw = clamped
                layer.settle(to: clamped)
            }
            CATransaction.commit()
            if let document = document {
                layer.ensureTiles(document: document, scale: scale, attribute: attribute, velocity: -delta.y * 8)
            }
        }
        return offsets[id] == current ? .unchanged : .moved(id)
    }

    func setOffset(_ id: Int, _ offset: CGPoint, document: LHDocument?) -> Bool {
        guard let geometry = geometries[id] else {
            return false
        }
        let clamped = geometry.clamp(offset)
        let previous = offsets[id] ?? .zero
        offsets[id] = document?.setScrollOffset(clamped, attribute: WidgetDOM.idAttribute, value: String(id)) ?? clamped
        if let layer = layers[id] {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            layer.content.removeAnimation(forKey: "tw.settle")
            layer.show(offsets[id] ?? clamped)
            layer.flashIndicator(offsets[id] ?? clamped)
            CATransaction.commit()
        }
        return previous != offsets[id]
    }

    func draw(into ctx: CGContext, document: LHDocument, scale: CGFloat) {
        for layer in layers.values {
            layer.draw(into: ctx, document: document, scale: scale)
        }
    }
}
