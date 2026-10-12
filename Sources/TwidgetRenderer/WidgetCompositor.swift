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
    private var driven: [Int: String] = [:]
    private(set) var bakes = 0

    var drivenIDs: Set<Int> {
        return Set(driven.keys)
    }

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
        driven.removeAll()
    }

    func undrive() {
        for id in driven.keys {
            promoted[id]?.layer.removeAnimation(forKey: "tw.transform")
            promoted[id]?.layer.removeAnimation(forKey: "tw.opacity")
        }
        driven.removeAll()
    }

    func drive(animator: WidgetAnimator, now: CFTimeInterval, enabled: Bool) {
        for id in driven.keys where promoted[id] == nil || !enabled {
            promoted[id]?.layer.removeAnimation(forKey: "tw.transform")
            promoted[id]?.layer.removeAnimation(forKey: "tw.opacity")
            driven[id] = nil
        }
        guard enabled else {
            return
        }
        for (id, item) in promoted {
            guard let baked = animator.bakeable(id) else {
                if driven[id] != nil {
                    item.layer.removeAnimation(forKey: "tw.transform")
                    item.layer.removeAnimation(forKey: "tw.opacity")
                    driven[id] = nil
                }
                continue
            }
            let spec = baked.spec
            let signature = "\(spec.name)|\(spec.duration)|\(spec.timingText)|\(spec.delay)|\(spec.iterations)|\(spec.direction)|\(spec.fill)|\(baked.start)|\(item.box)|\(item.origin)"
            if driven[id] == signature {
                continue
            }
            item.layer.removeAnimation(forKey: "tw.transform")
            item.layer.removeAnimation(forKey: "tw.opacity")
            driven[id] = nil
            let base = animator.base(id)
            guard let track = WidgetCompositor.bake(baked, base: base, box: item.box.size, origin: item.origin) else {
                continue
            }
            let localNow = item.layer.convertTime(CACurrentMediaTime(), from: nil)
            var begin = localNow - (now - (baked.start + spec.delay))
            if begin == 0 {
                begin = 1e-9
            }
            let reversedOrder = spec.direction == "reverse" || spec.direction == "alternate-reverse"
            let alternate = spec.direction.hasPrefix("alternate")
            func configure(_ animation: CAKeyframeAnimation, _ values: [Any]) {
                animation.values = reversedOrder ? values.reversed() : values
                animation.keyTimes = (reversedOrder ? track.times.reversed().map { 1 - $0 } : track.times).map { NSNumber(value: $0) }
                animation.calculationMode = .linear
                animation.duration = max(spec.duration, 0.0001)
                animation.beginTime = begin
                animation.autoreverses = alternate
                let count = alternate ? spec.iterations / 2 : spec.iterations
                animation.repeatCount = count.isFinite ? Float(count) : .infinity
                switch spec.fill {
                case "forwards":
                    animation.fillMode = .forwards
                case "backwards":
                    animation.fillMode = .backwards
                case "both":
                    animation.fillMode = .both
                default:
                    animation.fillMode = .removed
                }
                animation.isRemovedOnCompletion = !(spec.fill == "forwards" || spec.fill == "both")
            }
            if track.transforms.contains(where: { $0 != track.transforms[0] }) {
                let animation = CAKeyframeAnimation(keyPath: "transform")
                configure(animation, track.transforms.map { NSValue(caTransform3D: CATransform3DMakeAffineTransform($0)) })
                item.layer.add(animation, forKey: "tw.transform")
            }
            if track.opacities.contains(where: { $0 != track.opacities[0] }) {
                let animation = CAKeyframeAnimation(keyPath: "opacity")
                configure(animation, track.opacities.map { NSNumber(value: $0) })
                item.layer.add(animation, forKey: "tw.opacity")
            }
            driven[id] = signature
            bakes += 1
        }
    }

    func handoffError(animator: WidgetAnimator, now: CFTimeInterval) -> (geometry: CGFloat, alpha: Float, offset: Double) {
        var geometry: CGFloat = 0
        var alpha: Float = 0
        var offsetAtWorst = 0.0
        for id in driven.keys {
            guard let item = promoted[id], let shown = item.layer.presentation() else {
                continue
            }
            let actual = CATransform3DGetAffineTransform(shown.transform)
            var best: (CGFloat, Float, Double) = (.greatestFiniteMagnitude, 1, 0)
            for step in -25...25 {
                let offset = Double(step) / 1000
                guard let value = animator.peek(id, at: now + offset) else {
                    continue
                }
                let transform = CSSTransform.parse(value.transform, box: item.box.size) ?? .identity
                let opacity = Float(max(0, min(1, Double(value.opacity ?? "1") ?? 1)))
                var error: CGFloat = 0
                for corner in [CGPoint(x: -item.origin.x, y: -item.origin.y), CGPoint(x: item.box.width - item.origin.x, y: item.box.height - item.origin.y), CGPoint(x: item.box.width - item.origin.x, y: -item.origin.y)] {
                    let a = corner.applying(transform), b = corner.applying(actual)
                    error = max(error, hypot(a.x - b.x, a.y - b.y))
                }
                let alphaError = abs(opacity - shown.opacity)
                if error + CGFloat(alphaError) * 100 < best.0 + CGFloat(best.1) * 100 {
                    best = (error, alphaError, offset)
                }
            }
            if best.0 > geometry {
                geometry = best.0
                offsetAtWorst = best.2
            }
            alpha = max(alpha, best.1)
        }
        return (geometry, alpha, offsetAtWorst)
    }

    struct Track {
        var times: [Double]
        var transforms: [CGAffineTransform]
        var opacities: [Float]
    }

    static var handoffEnabled = ProcessInfo.processInfo.environment["TW_NO_HANDOFF"] == nil
    static let bakeTolerance: CGFloat = 0.05
    static let maxBakeDepth = 9

    static func bake(_ baked: WidgetAnimator.Baked, base: [String: String], box: CGSize, origin: CGPoint) -> Track? {
        func eval(_ p: Double) -> (CGAffineTransform, Float)? {
            let value = baked.value(p)
            guard let transform = CSSTransform.parse(value.transform ?? base["transform"], box: box) else {
                return nil
            }
            let opacityText = value.opacity ?? base["opacity"] ?? "1"
            guard let opacity = Double(opacityText.trimmingCharacters(in: .whitespaces)), opacity.isFinite else {
                return nil
            }
            return (transform, Float(max(0, min(1, opacity))))
        }
        let corners = [CGPoint(x: -origin.x, y: -origin.y), CGPoint(x: box.width - origin.x, y: -origin.y), CGPoint(x: -origin.x, y: box.height - origin.y), CGPoint(x: box.width - origin.x, y: box.height - origin.y)]
        func error(_ a: (CGAffineTransform, Float), _ b: (CGAffineTransform, Float), _ mid: (CGAffineTransform, Float)) -> (CGFloat, Float) {
            var worst: CGFloat = 0
            for corner in corners {
                let pa = corner.applying(a.0), pb = corner.applying(b.0), pm = corner.applying(mid.0)
                worst = max(worst, hypot(pm.x - (pa.x + pb.x) / 2, pm.y - (pa.y + pb.y) / 2))
            }
            return (worst, abs(mid.1 - (a.1 + b.1) / 2))
        }
        var times: [Double] = []
        var samples: [(CGAffineTransform, Float)] = []
        var failed = false
        func subdivide(_ t0: Double, _ s0: (CGAffineTransform, Float), _ t1: Double, _ s1: (CGAffineTransform, Float), _ depth: Int) {
            if failed {
                return
            }
            let tm = (t0 + t1) / 2
            guard let sm = eval(tm) else {
                failed = true
                return
            }
            let (geometry, alpha) = error(s0, s1, sm)
            if geometry <= bakeTolerance && alpha <= 0.002 {
                times.append(t1)
                samples.append(s1)
                return
            }
            if depth >= maxBakeDepth {
                failed = true
                return
            }
            subdivide(t0, s0, tm, sm, depth + 1)
            subdivide(tm, sm, t1, s1, depth + 1)
        }
        let initial = 8
        guard var previous = eval(0) else {
            return nil
        }
        times.append(0)
        samples.append(previous)
        for step in 1...initial {
            let t0 = Double(step - 1) / Double(initial), t1 = Double(step) / Double(initial)
            guard let next = eval(t1) else {
                return nil
            }
            subdivide(t0, previous, t1, next, 0)
            previous = next
        }
        if failed || times.count > 4096 {
            return nil
        }
        return Track(times: times, transforms: samples.map { $0.0 }, opacities: samples.map { $0.1 })
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
        for item in promoted.values {
            let shown = item.layer.presentation() ?? item.layer
            if shown.opacity <= 0 {
                continue
            }
            let local = shown.convert(point, from: shown.superlayer)
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
            let layer = item.layer.presentation() ?? item.layer
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
            let values = driven[id] != nil ? [] : overrides[id] ?? []
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
