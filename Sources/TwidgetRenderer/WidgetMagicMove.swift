import AppKit
import LiteHTML

final class WidgetMagicMove {
    struct Capture {
        let keyed: [String: Int]
        let ancestors: [String: Set<String>]
        let rects: [Int: CGRect]
        let presented: [String: CGRect]
        let signatures: [String: Int]
        var leaving: [(key: String, image: CGImage, ink: CGRect, border: CGRect)] = []
        var changed: [String: (image: CGImage, ink: CGRect, border: CGRect)] = [:]
    }

    private struct Active {
        let layer: CALayer
        let target: CGRect
    }

    static let attribute = "data-transition"
    static let maxLayers = 48
    static let maxPixels = 4096 * 4096
    static let structuralOps: Set<String> = ["el", "ins", "rm", "data", "text"]

    private weak var host: CALayer?
    private var active: [String: Active] = [:]
    private var leavingLayers: [CALayer] = []
    private var generation = 0
    private(set) var excluded: Set<Int> = []
    private(set) var transitions = 0
    var onFinish: (() -> Void)?

    init(host: CALayer?) {
        self.host = host
    }

    static func isStructural(_ ops: [[String: Any]]) -> Bool {
        for op in ops {
            guard let kind = op["op"] as? String else {
                continue
            }
            if structuralOps.contains(kind) || (kind == "attr" || kind == "rattr") && (op["name"] as? String) == "class" {
                return true
            }
        }
        return false
    }

    static func signature(_ node: WidgetNode) -> Int {
        var hasher = Hasher()
        func visit(_ node: WidgetNode) {
            hasher.combine(node.tag)
            hasher.combine(node.text)
            for attribute in node.attributes {
                hasher.combine(attribute.name)
                hasher.combine(attribute.value)
            }
            for declaration in node.declarations {
                hasher.combine(declaration.name)
                hasher.combine(declaration.value)
            }
            hasher.combine(node.children.count)
            for child in node.children {
                visit(child)
            }
        }
        visit(node)
        return hasher.finalize()
    }

    static func keyedNodes(_ dom: WidgetDOM, signatures: UnsafeMutablePointer<[String: Int]>? = nil) -> (keyed: [String: Int], ancestors: [String: Set<String>]) {
        var keyed: [String: Int] = [:]
        var ancestors: [String: Set<String>] = [:]
        var duplicates: Set<String> = []
        func key(_ node: WidgetNode) -> String? {
            if let value = node.attribute("data-key"), !value.isEmpty {
                return "k:" + value
            }
            if let value = node.attribute("id"), !value.isEmpty {
                return "i:" + value
            }
            return nil
        }
        func visit(_ node: WidgetNode, inside: Bool, chain: [String]) {
            if node.isText || node.svg && node.tag != "svg" {
                return
            }
            var chain = chain
            var inside = inside
            if inside, let k = key(node) {
                if keyed[k] != nil {
                    duplicates.insert(k)
                } else {
                    keyed[k] = dom.publicId(node)
                    ancestors[k] = Set(chain)
                    signatures?.pointee[k] = signature(node)
                }
                chain.append(k)
            }
            if node.attribute(attribute) == "move" {
                inside = true
            }
            for child in node.children {
                visit(child, inside: inside, chain: chain)
            }
        }
        visit(dom.body, inside: false, chain: [])
        for k in duplicates {
            keyed[k] = nil
            ancestors[k] = nil
        }
        return (keyed, ancestors)
    }

    func capture(dom: WidgetDOM, document: LHDocument) -> Capture? {
        var signatures: [String: Int] = [:]
        let (keyed, ancestors) = WidgetMagicMove.keyedNodes(dom, signatures: &signatures)
        guard !keyed.isEmpty else {
            return nil
        }
        var rects: [Int: CGRect] = [:]
        let wanted = Set(keyed.values.map { String($0) })
        for (key, value) in document.bounds(byAttribute: WidgetDOM.idAttribute) where wanted.contains(key) {
            if let id = Int(key) {
                rects[id] = value.rectValue
            }
        }
        var presented: [String: CGRect] = [:]
        for (key, item) in active {
            presented[key] = WidgetMagicMove.borderFrame(of: item.layer.presentation() ?? item.layer, target: item.target)
        }
        return Capture(keyed: keyed, ancestors: ancestors, rects: rects, presented: presented, signatures: signatures)
    }

    func collectLeaving(_ capture: inout Capture, dom: WidgetDOM, document: LHDocument, scale: CGFloat) {
        var signatures: [String: Int] = [:]
        let now = WidgetMagicMove.keyedNodes(dom, signatures: &signatures).keyed
        let gone = capture.keyed.keys.filter { now[$0] == nil }
        let goneSet = Set(gone)
        var pixels = 0
        for key in gone.sorted() {
            if !(capture.ancestors[key] ?? []).isDisjoint(with: goneSet) {
                continue
            }
            guard let id = capture.keyed[key], let rect = capture.rects[id], rect.width > 0, rect.height > 0 else {
                continue
            }
            pixels += Int(rect.width * scale * rect.height * scale)
            if capture.leaving.count >= WidgetMagicMove.maxLayers / 2 || pixels > WidgetMagicMove.maxPixels / 2 {
                break
            }
            var ink = CGRect.zero, border = CGRect.zero
            if let image = document.copyImageOfElement(attribute: WidgetDOM.idAttribute, value: String(id), scale: scale, inkBounds: &ink, borderBox: &border) {
                capture.leaving.append((key, image, ink, border))
            }
        }
        let changed = capture.signatures.compactMap { entry in signatures[entry.key].flatMap { $0 != entry.value ? entry.key : nil } }
        let changedSet = Set(changed)
        for key in changed.sorted() {
            if !(capture.ancestors[key] ?? []).isDisjoint(with: changedSet) {
                continue
            }
            guard let id = capture.keyed[key], let rect = capture.rects[id], rect.width > 0, rect.height > 0 else {
                continue
            }
            pixels += Int(rect.width * scale * rect.height * scale)
            if capture.leaving.count + capture.changed.count >= WidgetMagicMove.maxLayers / 2 || pixels > WidgetMagicMove.maxPixels / 2 {
                break
            }
            var ink = CGRect.zero, border = CGRect.zero
            if let image = document.copyImageOfElement(attribute: WidgetDOM.idAttribute, value: String(id), scale: scale, inkBounds: &ink, borderBox: &border) {
                capture.changed[key] = (image, ink, border)
            }
        }
    }

    func run(_ capture: Capture, dom: WidgetDOM, document: LHDocument, skip: Set<Int>, scale: CGFloat) -> Bool {
        guard let host = host else {
            return false
        }
        let (keyed, ancestors) = WidgetMagicMove.keyedNodes(dom)
        var rects: [Int: CGRect] = [:]
        let wanted = Set(keyed.values.map { String($0) })
        for (key, value) in document.bounds(byAttribute: WidgetDOM.idAttribute) where wanted.contains(key) {
            if let id = Int(key) {
                rects[id] = value.rectValue
            }
        }
        enum Kind {
            case move(CGRect)
            case enter
        }
        var candidates: [String: (id: Int, kind: Kind, rect: CGRect)] = [:]
        for (key, id) in keyed {
            guard !skip.contains(id), let rect = rects[id], rect.width > 0, rect.height > 0 else {
                continue
            }
            if let old = capture.presented[key] ?? capture.keyed[key].flatMap({ capture.rects[$0] }) {
                if WidgetMagicMove.close(old, rect) {
                    continue
                }
                candidates[key] = (id, .move(old), rect)
            } else {
                candidates[key] = (id, .enter, rect)
            }
        }
        let chosen = Set(candidates.keys)
        candidates = candidates.filter { (ancestors[$0.key] ?? []).isDisjoint(with: chosen) }
        var budget = WidgetMagicMove.maxPixels
        var admitted: [(key: String, id: Int, kind: Kind, rect: CGRect)] = []
        for (key, item) in candidates.sorted(by: { $0.value.rect.width * $0.value.rect.height > $1.value.rect.width * $1.value.rect.height }) {
            let pixels = Int((item.rect.width + 32) * scale * (item.rect.height + 32) * scale)
            if admitted.count + capture.leaving.count >= WidgetMagicMove.maxLayers || pixels > budget {
                return finishWithoutAnimation()
            }
            budget -= pixels
            admitted.append((key, item.id, item.kind, item.rect))
        }
        if admitted.isEmpty && capture.leaving.isEmpty {
            return false
        }
        generation += 1
        transitions += 1
        let current = generation
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        clearLayers()
        var exclusions: Set<Int> = []
        for item in admitted {
            var ink = CGRect.zero, border = CGRect.zero
            guard let image = document.copyImageOfElement(attribute: WidgetDOM.idAttribute, value: String(item.id), scale: scale, inkBounds: &ink, borderBox: &border), ink.width > 0, ink.height > 0 else {
                continue
            }
            let layer = WidgetMagicMove.layer(image: image, ink: ink, border: border, scale: scale)
            layer.zPosition = 2
            host.addSublayer(layer)
            switch item.kind {
            case let .move(old):
                layer.add(WidgetMagicMove.spring("transform", from: NSValue(caTransform3D: WidgetMagicMove.mapping(from: border, to: old)), to: NSValue(caTransform3D: CATransform3DIdentity)), forKey: "move")
                if let previous = capture.changed[item.key], previous.ink.width > 0, previous.ink.height > 0 {
                    let outgoing = WidgetMagicMove.layer(image: previous.image, ink: previous.ink, border: previous.border, scale: scale)
                    outgoing.zPosition = 2
                    outgoing.opacity = 0
                    outgoing.transform = WidgetMagicMove.mapping(from: previous.border, to: border)
                    host.insertSublayer(outgoing, below: layer)
                    outgoing.add(WidgetMagicMove.spring("transform", from: NSValue(caTransform3D: WidgetMagicMove.mapping(from: previous.border, to: old)), to: NSValue(caTransform3D: outgoing.transform)), forKey: "move")
                    outgoing.add(WidgetMagicMove.fade(from: 1, to: 0, duration: 0.3), forKey: "fade")
                    layer.add(WidgetMagicMove.fade(from: 0, to: 1, duration: 0.3), forKey: "fade")
                    leavingLayers.append(outgoing)
                }
            case .enter:
                layer.add(WidgetMagicMove.spring("transform", from: NSValue(caTransform3D: WidgetMagicMove.scaled(layer, 0.92)), to: NSValue(caTransform3D: CATransform3DIdentity)), forKey: "enter")
                let fade = CABasicAnimation(keyPath: "opacity")
                fade.fromValue = 0
                fade.toValue = 1
                fade.duration = 0.22
                fade.timingFunction = CAMediaTimingFunction(controlPoints: 0.33, 1, 0.68, 1)
                layer.add(fade, forKey: "fade")
            }
            active[item.key] = Active(layer: layer, target: border)
            exclusions.insert(item.id)
        }
        for item in capture.leaving {
            let layer = WidgetMagicMove.layer(image: item.image, ink: item.ink, border: item.border, scale: scale)
            layer.zPosition = 1
            layer.opacity = 0
            layer.transform = WidgetMagicMove.scaled(layer, 0.92)
            host.addSublayer(layer)
            let fade = CABasicAnimation(keyPath: "opacity")
            fade.fromValue = 1
            fade.toValue = 0
            fade.duration = 0.18
            fade.timingFunction = CAMediaTimingFunction(controlPoints: 0.32, 0, 0.67, 0)
            layer.add(fade, forKey: "fade")
            layer.add(WidgetMagicMove.spring("transform", from: NSValue(caTransform3D: CATransform3DIdentity), to: NSValue(caTransform3D: layer.transform)), forKey: "leave")
            leavingLayers.append(layer)
        }
        CATransaction.commit()
        excluded = exclusions
        DispatchQueue.main.asyncAfter(deadline: .now() + WidgetMagicMove.duration) { [weak self] in
            guard let self = self, self.generation == current else {
                return
            }
            self.onFinish?()
        }
        return true
    }

    func removeLayers() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        clearLayers()
        CATransaction.commit()
        excluded = []
    }

    func cancel() {
        generation += 1
        removeLayers()
    }

    var isActive: Bool {
        return !active.isEmpty || !leavingLayers.isEmpty
    }

    func draw(into ctx: CGContext) {
        for layer in (leavingLayers + active.values.map { $0.layer }).sorted(by: { $0.zPosition < $1.zPosition }) {
            let model = layer.presentation() ?? layer
            guard let contents = layer.contents, CFGetTypeID(contents as CFTypeRef) == CGImage.typeID else {
                continue
            }
            let size = model.bounds.size
            ctx.saveGState()
            ctx.translateBy(x: model.position.x, y: model.position.y)
            ctx.concatenate(CATransform3DGetAffineTransform(model.transform))
            ctx.translateBy(x: -model.anchorPoint.x * size.width, y: -model.anchorPoint.y * size.height)
            ctx.setAlpha(CGFloat(model.opacity))
            ctx.translateBy(x: 0, y: size.height)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(contents as! CGImage, in: CGRect(origin: .zero, size: size))
            ctx.restoreGState()
        }
    }

    private func finishWithoutAnimation() -> Bool {
        if isActive {
            generation += 1
            removeLayers()
            return true
        }
        return false
    }

    private func clearLayers() {
        for item in active.values {
            item.layer.removeFromSuperlayer()
        }
        for layer in leavingLayers {
            layer.removeFromSuperlayer()
        }
        active.removeAll()
        leavingLayers.removeAll()
    }

    static let duration: CFTimeInterval = 0.55

    private static func spring(_ keyPath: String, from: Any, to: Any) -> CAAnimation {
        let spring = CASpringAnimation(keyPath: keyPath)
        spring.fromValue = from
        spring.toValue = to
        spring.mass = 1
        spring.stiffness = 320
        spring.damping = 30
        spring.initialVelocity = 0
        spring.duration = duration
        spring.fillMode = .both
        return spring
    }

    private static func mapping(from a: CGRect, to b: CGRect) -> CATransform3D {
        let sx = a.width > 0 ? b.width / a.width : 1
        let sy = a.height > 0 ? b.height / a.height : 1
        return CATransform3DConcat(CATransform3DMakeScale(sx, sy, 1), CATransform3DMakeTranslation(b.minX - a.minX, b.minY - a.minY, 0))
    }

    private static func fade(from: Float, to: Float, duration: CFTimeInterval) -> CAAnimation {
        let fade = CABasicAnimation(keyPath: "opacity")
        fade.fromValue = from
        fade.toValue = to
        fade.duration = duration
        fade.timingFunction = CAMediaTimingFunction(controlPoints: 0.45, 0, 0.55, 1)
        fade.fillMode = .both
        return fade
    }

    private static func layer(image: CGImage, ink: CGRect, border: CGRect, scale: CGFloat) -> CALayer {
        let layer = CALayer()
        layer.contents = image
        layer.contentsScale = scale
        layer.contentsGravity = .resize
        layer.bounds = CGRect(origin: .zero, size: ink.size)
        layer.anchorPoint = CGPoint(x: (border.minX - ink.minX) / ink.width, y: (border.minY - ink.minY) / ink.height)
        layer.position = border.origin
        return layer
    }

    private static func scaled(_ layer: CALayer, _ factor: CGFloat) -> CATransform3D {
        let size = layer.bounds.size
        let anchor = CGPoint(x: layer.anchorPoint.x * size.width, y: layer.anchorPoint.y * size.height)
        let center = CGPoint(x: size.width / 2 - anchor.x, y: size.height / 2 - anchor.y)
        var transform = CATransform3DMakeTranslation(-center.x, -center.y, 0)
        transform = CATransform3DConcat(transform, CATransform3DMakeScale(factor, factor, 1))
        return CATransform3DConcat(transform, CATransform3DMakeTranslation(center.x, center.y, 0))
    }

    private static func borderFrame(of layer: CALayer, target: CGRect) -> CGRect {
        let t = CATransform3DGetAffineTransform(layer.transform)
        let origin = CGPoint(x: layer.position.x + t.tx, y: layer.position.y + t.ty)
        return CGRect(x: origin.x, y: origin.y, width: target.width * t.a, height: target.height * t.d)
    }

    private static func close(_ a: CGRect, _ b: CGRect) -> Bool {
        return abs(a.minX - b.minX) < 0.5 && abs(a.minY - b.minY) < 0.5 && abs(a.width - b.width) < 0.5 && abs(a.height - b.height) < 0.5
    }
}
