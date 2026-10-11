import AppKit
import LiteHTML

enum CSSTransform {
    static func parse(_ text: String?, box: CGSize) -> CGAffineTransform? {
        guard let text = text?.trimmingCharacters(in: .whitespaces), !text.isEmpty, text.lowercased() != "none" else {
            return .identity
        }
        var result = CGAffineTransform.identity
        var rest = Substring(text)
        while let open = rest.firstIndex(of: "(") {
            guard let close = rest[open...].firstIndex(of: ")") else {
                return nil
            }
            let name = rest[..<open].trimmingCharacters(in: .whitespaces).lowercased()
            let args = rest[rest.index(after: open)..<close].split(whereSeparator: { $0 == "," || $0 == " " }).map(String.init)
            guard let function = make(name, args, box: box) else {
                return nil
            }
            result = function.concatenating(result)
            rest = rest[rest.index(after: close)...]
        }
        if !rest.trimmingCharacters(in: .whitespaces).isEmpty {
            return nil
        }
        return result
    }

    private static func length(_ text: String, relative: CGFloat) -> CGFloat? {
        let t = text.lowercased()
        if t.hasSuffix("px"), let v = Double(t.dropLast(2)) {
            return CGFloat(v)
        }
        if t.hasSuffix("%"), let v = Double(t.dropLast()) {
            return CGFloat(v) / 100 * relative
        }
        if let v = Double(t), v == 0 {
            return 0
        }
        return nil
    }

    private static func angle(_ text: String) -> CGFloat? {
        let t = text.lowercased()
        if t.hasSuffix("deg"), let v = Double(t.dropLast(3)) {
            return CGFloat(v * .pi / 180)
        }
        if t.hasSuffix("grad"), let v = Double(t.dropLast(4)) {
            return CGFloat(v * .pi / 200)
        }
        if t.hasSuffix("rad"), let v = Double(t.dropLast(3)) {
            return CGFloat(v)
        }
        if t.hasSuffix("turn"), let v = Double(t.dropLast(4)) {
            return CGFloat(v * 2 * .pi)
        }
        if let v = Double(t), v == 0 {
            return 0
        }
        return nil
    }

    private static func make(_ name: String, _ args: [String], box: CGSize) -> CGAffineTransform? {
        func number(_ i: Int) -> CGFloat? {
            return i < args.count ? Double(args[i]).map { CGFloat($0) } : nil
        }
        switch name {
        case "translate":
            guard let x = args.first.flatMap({ length($0, relative: box.width) }) else {
                return nil
            }
            let y = args.count > 1 ? length(args[1], relative: box.height) : 0
            return y.map { CGAffineTransform(translationX: x, y: $0) }
        case "translatex":
            return args.first.flatMap { length($0, relative: box.width) }.map { CGAffineTransform(translationX: $0, y: 0) }
        case "translatey":
            return args.first.flatMap { length($0, relative: box.height) }.map { CGAffineTransform(translationX: 0, y: $0) }
        case "scale":
            guard let x = number(0) else {
                return nil
            }
            return CGAffineTransform(scaleX: x, y: number(1) ?? x)
        case "scalex":
            return number(0).map { CGAffineTransform(scaleX: $0, y: 1) }
        case "scaley":
            return number(0).map { CGAffineTransform(scaleX: 1, y: $0) }
        case "rotate", "rotatez":
            return args.first.flatMap(angle).map { CGAffineTransform(rotationAngle: $0) }
        case "skew":
            guard let x = args.first.flatMap(angle) else {
                return nil
            }
            let y = args.count > 1 ? angle(args[1]) ?? 0 : 0
            return CGAffineTransform(a: 1, b: tan(y), c: tan(x), d: 1, tx: 0, ty: 0)
        case "skewx":
            return args.first.flatMap(angle).map { CGAffineTransform(a: 1, b: 0, c: tan($0), d: 1, tx: 0, ty: 0) }
        case "skewy":
            return args.first.flatMap(angle).map { CGAffineTransform(a: 1, b: tan($0), c: 0, d: 1, tx: 0, ty: 0) }
        case "matrix":
            let v = args.compactMap { Double($0).map { CGFloat($0) } }
            return v.count == 6 ? CGAffineTransform(a: v[0], b: v[1], c: v[2], d: v[3], tx: v[4], ty: v[5]) : nil
        default:
            return nil
        }
    }

    static func origin(_ text: String?, box: CGSize) -> CGPoint {
        var x = box.width / 2, y = box.height / 2
        guard let parts = text?.split(separator: " ").map({ String($0).lowercased() }), !parts.isEmpty else {
            return CGPoint(x: x, y: y)
        }
        func resolve(_ part: String, horizontal: Bool) -> CGFloat? {
            let size = horizontal ? box.width : box.height
            switch part {
            case "left", "top":
                return 0
            case "right", "bottom":
                return size
            case "center":
                return size / 2
            default:
                return length(part, relative: size)
            }
        }
        if parts.count == 1 {
            if parts[0] == "top" || parts[0] == "bottom" {
                y = resolve(parts[0], horizontal: false) ?? y
            } else {
                x = resolve(parts[0], horizontal: true) ?? x
            }
        } else {
            let first = parts[0], second = parts[1]
            if first == "top" || first == "bottom" || second == "left" || second == "right" {
                y = resolve(first, horizontal: false) ?? y
                x = resolve(second, horizontal: true) ?? x
            } else {
                x = resolve(first, horizontal: true) ?? x
                y = resolve(second, horizontal: false) ?? y
            }
        }
        return CGPoint(x: x, y: y)
    }
}

final class WidgetCompositor {
    private struct Promoted {
        let layer: CALayer
        var box: CGRect
        var origin: CGPoint
        var ink: CGRect
    }

    static let maxLayers = 48
    static let maxPixels = 4096 * 4096

    private var promoted: [Int: Promoted] = [:]
    private weak var host: CALayer?
    private(set) var promotions = 0

    var ids: Set<Int> {
        return Set(promoted.keys)
    }

    init(host: CALayer?) {
        self.host = host
    }

    func attach(_ host: CALayer?) {
        self.host = host
        for item in promoted.values where item.layer.superlayer !== host {
            host?.addSublayer(item.layer)
        }
    }

    func reset() {
        for item in promoted.values {
            item.layer.removeFromSuperlayer()
        }
        promoted.removeAll()
    }

    func update(document: LHDocument, animator: WidgetAnimator, dom: WidgetDOM, declarations: [Int: [String: String]], scale: CGFloat, space: CGColorSpace) {
        guard let host = host else {
            reset()
            return
        }
        let animated = animator.animatedIDs()
        let boxes = document.bounds(byAttribute: WidgetDOM.idAttribute)
        var wanted: [Int: (box: CGRect, origin: CGPoint, maxScale: CGFloat)] = [:]
        for id in animated where animator.isCompositable(id) {
            guard let node = dom.node(id), !node.svg, let box = boxes[String(id)]?.rectValue, box.width > 0, box.height > 0, box.width * box.height < 2_000_000 else {
                continue
            }
            if animated.contains(where: { $0 != id && WidgetCompositor.isAncestor(id, of: $0, dom: dom) }) || animated.contains(where: { $0 != id && WidgetCompositor.isAncestor($0, of: id, dom: dom) }) {
                continue
            }
            let origin = CSSTransform.origin(declarations[id]?["transform-origin"], box: box.size)
            var reach = box
            var maxScale: CGFloat = 1
            var ok = true
            for value in animator.reachableValues(id) {
                guard let transform = CSSTransform.parse(value.transform, box: box.size) else {
                    ok = false
                    break
                }
                let anchored = CGAffineTransform(translationX: -origin.x, y: -origin.y).concatenating(transform).concatenating(CGAffineTransform(translationX: origin.x + box.minX, y: origin.y + box.minY))
                reach = reach.union(CGRect(origin: .zero, size: box.size).applying(anchored))
                maxScale = max(maxScale, sqrt(abs(transform.a * transform.d - transform.b * transform.c)))
                if let opacity = value.opacity, Double(opacity) == nil {
                    ok = false
                    break
                }
            }
            if !ok || !document.isElement(attribute: WidgetDOM.idAttribute, value: String(id), isolatedIn: reach) {
                continue
            }
            wanted[id] = (box, origin, min(maxScale, 3))
        }
        var budget = WidgetCompositor.maxPixels
        var admitted: [Int: (box: CGRect, origin: CGPoint, maxScale: CGFloat)] = [:]
        for (id, want) in wanted.sorted(by: { $0.value.box.width * $0.value.box.height < $1.value.box.width * $1.value.box.height }) {
            let factor = scale * want.maxScale
            let pixels = Int(((want.box.width + 96) * factor).rounded(.up) * ((want.box.height + 96) * factor).rounded(.up))
            if admitted.count >= WidgetCompositor.maxLayers || pixels > budget {
                continue
            }
            budget -= pixels
            admitted[id] = want
        }
        wanted = admitted
        for (id, item) in promoted where wanted[id] == nil {
            item.layer.removeFromSuperlayer()
            promoted[id] = nil
        }
        for (id, want) in wanted {
            var ink = CGRect.zero, border = CGRect.zero
            guard let image = document.copyImageOfElement(attribute: WidgetDOM.idAttribute, value: String(id), scale: scale * want.maxScale, inkBounds: &ink, borderBox: &border) else {
                promoted[id]?.layer.removeFromSuperlayer()
                promoted[id] = nil
                continue
            }
            let layer = promoted[id]?.layer ?? CALayer()
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            layer.contents = image
            layer.contentsScale = scale * want.maxScale
            layer.contentsGravity = .resize
            layer.bounds = CGRect(origin: .zero, size: ink.size)
            let anchor = CGPoint(x: border.minX + want.origin.x, y: border.minY + want.origin.y)
            layer.anchorPoint = CGPoint(x: (anchor.x - ink.minX) / ink.width, y: (anchor.y - ink.minY) / ink.height)
            layer.position = anchor
            layer.zPosition = 1
            if layer.superlayer !== host {
                host.addSublayer(layer)
            }
            CATransaction.commit()
            if promoted[id] == nil {
                promotions += 1
            }
            promoted[id] = Promoted(layer: layer, box: border, origin: want.origin, ink: ink)
        }
    }

    func documentPoint(for point: CGPoint) -> CGPoint? {
        for item in promoted.values where item.layer.opacity > 0 {
            let local = item.layer.convert(point, from: item.layer.superlayer)
            if item.layer.bounds.contains(local) {
                let mapped = CGPoint(x: item.ink.minX + local.x, y: item.ink.minY + local.y)
                if item.box.contains(mapped) {
                    return mapped
                }
            }
        }
        return nil
    }

    func draw(into ctx: CGContext) {
        for item in promoted.values.sorted(by: { $0.layer.zPosition < $1.layer.zPosition }) {
            guard let contents = item.layer.contents, CFGetTypeID(contents as CFTypeRef) == CGImage.typeID else {
                continue
            }
            let image = contents as! CGImage
            let layer = item.layer
            let size = layer.bounds.size
            ctx.saveGState()
            ctx.translateBy(x: layer.position.x, y: layer.position.y)
            ctx.concatenate(layer.affineTransform())
            ctx.translateBy(x: -layer.anchorPoint.x * size.width, y: -layer.anchorPoint.y * size.height)
            ctx.setAlpha(CGFloat(layer.opacity))
            ctx.translateBy(x: 0, y: size.height)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(image, in: CGRect(origin: .zero, size: size))
            ctx.restoreGState()
        }
    }

    func apply(_ overrides: [Int: [(String, String)]], bases: (Int) -> [String: String]) -> [Int: [(String, String)]] {
        if promoted.isEmpty {
            return overrides
        }
        var rest = overrides
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for (id, item) in promoted {
            let values = overrides[id] ?? []
            rest[id] = nil
            let base = bases(id)
            let transformText = values.last { $0.0 == "transform" }?.1 ?? base["transform"]
            let opacityText = values.last { $0.0 == "opacity" }?.1 ?? base["opacity"]
            let transform = CSSTransform.parse(transformText, box: item.box.size) ?? .identity
            item.layer.setAffineTransform(transform)
            item.layer.opacity = Float(opacityText.flatMap { Double($0) } ?? 1)
        }
        CATransaction.commit()
        return rest
    }

    private static func isAncestor(_ ancestor: Int, of id: Int, dom: WidgetDOM) -> Bool {
        guard var node = dom.node(id) else {
            return false
        }
        while let parent = node.parent {
            if dom.publicId(parent) == ancestor {
                return true
            }
            node = parent
        }
        return false
    }
}
