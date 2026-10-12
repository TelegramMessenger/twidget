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

    static func resolve(_ text: String?, variable: (String) -> String?) -> String? {
        guard var text = text else {
            return nil
        }
        var depth = 0
        while let start = text.range(of: "var(") {
            depth += 1
            guard depth <= 16, let close = matching(text, from: start.upperBound) else {
                return nil
            }
            let inner = String(text[start.upperBound..<close])
            let parts = WidgetNode.splitTopLevel(inner, separator: ",")
            let name = parts.first?.trimmingCharacters(in: .whitespaces) ?? ""
            guard name.hasPrefix("--"), let value = variable(name) ?? (parts.count > 1 ? parts.dropFirst().joined(separator: ",").trimmingCharacters(in: .whitespaces) : nil) else {
                return nil
            }
            text.replaceSubrange(start.lowerBound...close, with: value)
            if text.count > 4096 {
                return nil
            }
        }
        while let start = text.range(of: "calc(") {
            guard let close = matching(text, from: start.upperBound), let value = Calc.evaluate(String(text[start.upperBound..<close])) else {
                return nil
            }
            text.replaceSubrange(start.lowerBound...close, with: value)
        }
        return text
    }

    private static func matching(_ text: String, from index: String.Index) -> String.Index? {
        var depth = 1
        var i = index
        while i < text.endIndex {
            if text[i] == "(" {
                depth += 1
            } else if text[i] == ")" {
                depth -= 1
                if depth == 0 {
                    return i
                }
            }
            i = text.index(after: i)
        }
        return nil
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

enum Calc {
    private struct Value {
        var number: Double
        var unit: String
    }

    static func evaluate(_ text: String) -> String? {
        var tokens: [String] = []
        var current = ""
        for ch in text {
            if "+-*/() ".contains(ch) {
                if !current.isEmpty {
                    tokens.append(current)
                    current = ""
                }
                if ch != " " {
                    if (ch == "-" || ch == "+"), tokens.last.map({ "*/(+-".contains($0) }) ?? true {
                        current.append(ch)
                    } else {
                        tokens.append(String(ch))
                    }
                }
            } else {
                current.append(ch)
            }
        }
        if !current.isEmpty {
            tokens.append(current)
        }
        guard tokens.count <= 256 else {
            return nil
        }
        var index = 0
        func atom() -> Value? {
            guard index < tokens.count else {
                return nil
            }
            let token = tokens[index]
            index += 1
            if token == "(" {
                guard let value = sum(), index < tokens.count, tokens[index] == ")" else {
                    return nil
                }
                index += 1
                return value
            }
            let numeric = token.prefix { "0123456789.-+eE".contains($0) }
            guard let number = Double(numeric), number.isFinite else {
                return nil
            }
            let unit = String(token.dropFirst(numeric.count)).lowercased()
            guard ["", "px", "deg", "rad", "turn", "grad", "%"].contains(unit) else {
                return nil
            }
            return Value(number: number, unit: unit)
        }
        func product() -> Value? {
            guard var left = atom() else {
                return nil
            }
            while index < tokens.count, tokens[index] == "*" || tokens[index] == "/" {
                let op = tokens[index]
                index += 1
                guard let right = atom() else {
                    return nil
                }
                if op == "*" {
                    guard left.unit.isEmpty || right.unit.isEmpty else {
                        return nil
                    }
                    left = Value(number: left.number * right.number, unit: left.unit.isEmpty ? right.unit : left.unit)
                } else {
                    guard right.unit.isEmpty, right.number != 0 else {
                        return nil
                    }
                    left = Value(number: left.number / right.number, unit: left.unit)
                }
            }
            return left
        }
        func sum() -> Value? {
            guard var left = product() else {
                return nil
            }
            while index < tokens.count, tokens[index] == "+" || tokens[index] == "-" {
                let op = tokens[index]
                index += 1
                guard let right = product(), left.unit == right.unit || left.number == 0 || right.number == 0 else {
                    return nil
                }
                let unit = left.unit.isEmpty ? right.unit : left.unit
                left = Value(number: op == "+" ? left.number + right.number : left.number - right.number, unit: unit)
            }
            return left
        }
        guard let value = sum(), index == tokens.count, value.number.isFinite else {
            return nil
        }
        return String(format: "%.6f", value.number) + value.unit
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
    var scriptedCandidates: Set<Int> = []
    private var scripted: [Int: (reach: CGRect, scale: CGFloat)] = [:]

    var scriptedIDs: Set<Int> {
        return Set(scripted.keys)
    }

    static func style(_ node: WidgetNode) -> [String: String] {
        var style: [String: String] = [:]
        for declaration in node.declarations {
            style[declaration.name] = declaration.value
        }
        return style
    }

    private static func anchored(_ transform: CGAffineTransform, box: CGRect, origin: CGPoint) -> CGAffineTransform {
        return CGAffineTransform(translationX: -origin.x, y: -origin.y).concatenating(transform).concatenating(CGAffineTransform(translationX: origin.x + box.minX, y: origin.y + box.minY))
    }

    func applyScripted(_ ids: Set<Int>, dom: WidgetDOM) -> Bool {
        var updates: [(CALayer, CGAffineTransform, Float)] = []
        for id in ids {
            guard let item = promoted[id], let limits = scripted[id], let node = dom.node(id) else {
                return false
            }
            let style = WidgetCompositor.style(node)
            guard let transform = parse(style["transform"], box: item.box.size, id: id) else {
                return false
            }
            let opacity: Float
            if let text = style["opacity"] {
                guard let value = Double(text.trimmingCharacters(in: .whitespaces)), value.isFinite else {
                    return false
                }
                opacity = Float(max(0, min(1, value)))
            } else {
                opacity = 1
            }
            let moved = CGRect(origin: .zero, size: item.box.size).applying(WidgetCompositor.anchored(transform, box: item.box, origin: item.origin))
            if !limits.reach.contains(moved) || sqrt(abs(transform.a * transform.d - transform.b * transform.c)) > limits.scale + 0.001 {
                return false
            }
            updates.append((item.layer, transform, opacity))
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for (layer, transform, opacity) in updates {
            layer.setAffineTransform(transform)
            layer.opacity = opacity
        }
        CATransaction.commit()
        return true
    }

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
        for container in containers.values {
            container.removeFromSuperlayer()
        }
        containers.removeAll()
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
            guard let track = WidgetCompositor.bake(baked, base: base, box: item.box.size, origin: item.origin, resolve: { [self] in self.resolved($0, id: id) }) else {
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
            let baseTransform = parse(base["transform"], box: item.box.size, id: id) ?? .identity
            let baseOpacity = Float(max(0, min(1, Double(base["opacity"] ?? "1") ?? 1)))
            if track.transforms.contains(where: { $0 != track.transforms[0] }) || track.transforms[0] != baseTransform {
                let animation = CAKeyframeAnimation(keyPath: "transform")
                configure(animation, track.transforms.map { NSValue(caTransform3D: CATransform3DMakeAffineTransform(WidgetCompositor.decomposable($0))) })
                item.layer.add(animation, forKey: "tw.transform")
            }
            if track.opacities.contains(where: { $0 != track.opacities[0] }) || track.opacities[0] != baseOpacity {
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
                let transform = parse(value.transform, box: item.box.size, id: id) ?? .identity
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
            if best.0 > 1, WidgetCompositor.log {
                let parentLayer = item.layer.superlayer
                print("HANDOFF \(id) error=\(best.0) offset=\(best.2) nested=\(parentLayer !== host) transform=\(animator.peek(id, at: now)?.transform ?? "nil") presentation=\(CATransform3DGetAffineTransform(shown.transform)) box=\(item.box) origin=\(item.origin)")
            }
            if best.0 > geometry {
                geometry = best.0
                offsetAtWorst = best.2
            }
            alpha = max(alpha, best.1)
        }
        return (geometry, alpha, offsetAtWorst)
    }

    static func decomposable(_ t: CGAffineTransform) -> CGAffineTransform {
        if abs(t.a * t.d - t.b * t.c) >= 1e-8 {
            return t
        }
        return CGAffineTransform(a: t.a >= 0 ? 1e-4 : -1e-4, b: 0, c: 0, d: t.d >= 0 ? 1e-4 : -1e-4, tx: t.tx, ty: t.ty)
    }

    struct Track {
        var times: [Double]
        var transforms: [CGAffineTransform]
        var opacities: [Float]
    }

    static let log = ProcessInfo.processInfo.environment["TW_COMPOSITE_LOG"] != nil
    static var handoffEnabled = ProcessInfo.processInfo.environment["TW_NO_HANDOFF"] == nil
    static let bakeTolerance: CGFloat = 0.05
    static let maxBakeDepth = 9

    static func bake(_ baked: WidgetAnimator.Baked, base: [String: String], box: CGSize, origin: CGPoint, resolve: (String?) -> String? = { $0 }) -> Track? {
        func eval(_ p: Double) -> (CGAffineTransform, Float)? {
            let value = baked.value(p)
            let raw = value.transform ?? base["transform"]
            let text = resolve(raw)
            guard raw == nil || text != nil, let transform = CSSTransform.parse(text, box: box) else {
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

    func update(document: LHDocument, animator: WidgetAnimator, dom: WidgetDOM, declarations: [Int: [String: String]], scale: CGFloat, space: CGColorSpace, extraExclusions: Set<Int> = []) {
        guard let host = host else {
            reset()
            return
        }
        currentDOM = dom
        let animated = animator.animatedIDs()
        let boxes = document.bounds(byAttribute: WidgetDOM.idAttribute)
        var wanted: [Int: (box: CGRect, origin: CGPoint, maxScale: CGFloat)] = [:]
        var reaches: [Int: [CGRect]] = [:]
        let log = WidgetCompositor.log
        for id in animated {
            guard animator.isCompositable(id) else {
                if log { print("COMPOSITE \(id) not compositable") }
                continue
            }
            guard let node = dom.node(id), !node.svg, let box = boxes[String(id)]?.rectValue, box.width > 0, box.height > 0, box.width * box.height < 2_000_000 else {
                if log { print("COMPOSITE \(id) no box") }
                continue
            }
            let origin = CSSTransform.origin(declarations[id]?["transform-origin"], box: box.size)
            var reach = box
            var maxScale: CGFloat = 1
            var ok = true
            for value in animator.reachableValues(id) {
                guard let transform = parse(value.transform, box: box.size, id: id) else {
                    if log { print("COMPOSITE \(id) unparsed \(value.transform ?? "nil") resolved \(resolved(value.transform, id: id) ?? "nil")") }
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
            if !ok {
                continue
            }
            wanted[id] = (box, origin, min(maxScale, 3))
            reaches[id] = [reach]
        }
        var scriptedWanted: [Int: (reach: CGRect, scale: CGFloat)] = [:]
        scriptedCandidates = scriptedCandidates.filter { dom.node($0) != nil }
        for id in scriptedCandidates where wanted[id] == nil && !animated.contains(id) && animator.base(id).isEmpty {
            guard let node = dom.node(id), !node.svg, let box = boxes[String(id)]?.rectValue, box.width > 0, box.height > 0, box.width * box.height < 2_000_000 else {
                continue
            }
            if wanted.keys.contains(where: { WidgetCompositor.isAncestor(id, of: $0, dom: dom) || WidgetCompositor.isAncestor($0, of: id, dom: dom) }) || scriptedCandidates.contains(where: { $0 != id && (WidgetCompositor.isAncestor(id, of: $0, dom: dom) || WidgetCompositor.isAncestor($0, of: id, dom: dom)) }) {
                continue
            }
            let style = WidgetCompositor.style(node)
            guard let transform = parse(style["transform"], box: box.size, id: id), style["opacity"].map({ Double($0.trimmingCharacters(in: .whitespaces)) != nil }) ?? true else {
                continue
            }
            let origin = CSSTransform.origin(style["transform-origin"], box: box.size)
            let current = CGRect(origin: .zero, size: box.size).applying(WidgetCompositor.anchored(transform, box: box, origin: origin))
            let rotated = abs(transform.b) > 0.0001 || abs(transform.c) > 0.0001
            let scale: CGFloat = min(3, max(rotated ? 2 : 1, sqrt(abs(transform.a * transform.d - transform.b * transform.c)) * 1.25))
            wanted[id] = (box, origin, scale)
            reaches[id] = [max(48, max(box.width, box.height) / 2), 16, 2].map { box.union(current).insetBy(dx: -$0, dy: -$0) }
            scriptedWanted[id] = (.null, scale)
        }
        for id in wanted.keys {
            var dx: CGFloat = 0, dy: CGFloat = 0
            for other in wanted.keys where other != id && WidgetCompositor.isAncestor(other, of: id, dom: dom) {
                if let box = wanted[other]?.box, let reach = reaches[other]?.first {
                    dx += max(box.minX - reach.minX, reach.maxX - box.maxX, 0)
                    dy += max(box.minY - reach.minY, reach.maxY - box.maxY, 0)
                }
            }
            if dx > 0 || dy > 0 {
                reaches[id] = reaches[id]?.map { $0.insetBy(dx: -dx, dy: -dy) }
            }
        }
        var clips: [Int: Clip] = [:]
        for id in wanted.keys {
            var rect = CGRect.zero
            var radii = [CGFloat](repeating: 0, count: 8)
            if document.clipOfElement(attribute: WidgetDOM.idAttribute, value: String(id), rect: &rect, radii: &radii) {
                clips[id] = Clip(rect: rect, radii: radii)
            }
        }
        for id in wanted.keys {
            for other in wanted.keys where other != id && WidgetCompositor.isAncestor(other, of: id, dom: dom) && clips[other] != clips[id] {
                wanted[id] = nil
            }
        }
        func unobscured(_ id: Int, _ reach: CGRect, _ ignoring: Set<String>) -> Bool {
            if let clip = clips[id] {
                let clipped = reach.intersection(clip.rect)
                return !clipped.isNull && document.isElement(attribute: WidgetDOM.idAttribute, value: String(id), unobscuredIn: clipped, ignoring: ignoring, clipped: true)
            }
            return document.isElement(attribute: WidgetDOM.idAttribute, value: String(id), unobscuredIn: reach, ignoring: ignoring)
        }
        var accepted = wanted
        for _ in 0..<4 {
            let ignoring = Set(accepted.keys.map { String($0) })
            var next: [Int: (box: CGRect, origin: CGPoint, maxScale: CGFloat)] = [:]
            for (id, want) in accepted {
                if animated.contains(where: { accepted[$0] == nil && WidgetCompositor.isAncestor($0, of: id, dom: dom) }) {
                    continue
                }
                if let reach = reaches[id]?.first(where: { unobscured(id, $0, ignoring) }) {
                    next[id] = want
                    if scriptedWanted[id] != nil {
                        scriptedWanted[id]?.reach = reach
                    }
                }
            }
            var budget = WidgetCompositor.maxPixels
            var admitted: [Int: (box: CGRect, origin: CGPoint, maxScale: CGFloat)] = [:]
            for (id, want) in next.sorted(by: { $0.value.box.width * $0.value.box.height < $1.value.box.width * $1.value.box.height }) {
                let factor = scale * want.maxScale
                let pixels = Int(((want.box.width + 96) * factor).rounded(.up) * ((want.box.height + 96) * factor).rounded(.up))
                if admitted.count >= WidgetCompositor.maxLayers || pixels > budget {
                    continue
                }
                budget -= pixels
                admitted[id] = want
            }
            if log { print("COMPOSITE round wanted=\(wanted.count) unobscured=\(next.count) admitted=\(admitted.count)") }
            let stable = Set(admitted.keys) == Set(accepted.keys)
            accepted = admitted
            if stable {
                break
            }
        }
        while let orphan = accepted.keys.first(where: { id in animated.contains(where: { accepted[$0] == nil && WidgetCompositor.isAncestor($0, of: id, dom: dom) }) }) {
            accepted[orphan] = nil
        }
        if accepted.count > 1 {
            let ignoring = Set(accepted.keys.map { String($0) })
            accepted = accepted.filter { entry in reaches[entry.key]?.contains(where: { unobscured(entry.key, $0, ignoring) }) ?? false }
        } else if let only = accepted.first, !(reaches[only.key]?.contains(where: { unobscured(only.key, $0, []) }) ?? false) {
            accepted = [:]
        }
        wanted = accepted
        scripted = scriptedWanted.filter { accepted[$0.key] != nil && !$0.value.reach.isNull }
        var order: [String: NSNumber] = [:]
        if Set(wanted.keys) != Set(promoted.keys) || wanted.count > 1 && zOrderStale {
            order = document.paintOrder(attribute: WidgetDOM.idAttribute, values: Set(wanted.keys.map { String($0) }))
            zOrderStale = false
        }
        for (id, item) in promoted where wanted[id] == nil {
            item.layer.removeFromSuperlayer()
            promoted[id] = nil
        }
        document.setPaintExclusions(Set(wanted.keys.map { String($0) }).union(extraExclusions.map { String($0) }), attribute: WidgetDOM.idAttribute)
        for (id, want) in wanted {
            let inner = wanted.keys.filter { $0 != id && WidgetCompositor.isAncestor(id, of: $0, dom: dom) }.sorted()
            let signature = WidgetCompositor.contentSignature(id, dom: dom, box: want.box, scale: scale * want.maxScale, styleKey: styleKey ^ inner.hashValue)
            if let signature = signature, let existing = promoted[id], rasterSignatures[id] == signature, existing.layer.superlayer != nil {
                if let index = order[String(id)] {
                    existing.layer.zPosition = 1 + CGFloat(index.intValue) * 1e-6
                }
                continue
            }
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
            layer.minificationFilter = want.maxScale > 1 && ProcessInfo.processInfo.environment["TW_LINEAR"] == nil ? .trilinear : .linear
            layer.bounds = CGRect(origin: .zero, size: ink.size)
            let anchor = CGPoint(x: border.minX + want.origin.x, y: border.minY + want.origin.y)
            layer.anchorPoint = CGPoint(x: (anchor.x - ink.minX) / ink.width, y: (anchor.y - ink.minY) / ink.height)
            layer.position = anchor
            layer.zPosition = 1 + CGFloat(order[String(id)]?.intValue ?? 0) * 1e-6
            CATransaction.commit()
            if promoted[id] == nil {
                promotions += 1
            }
            promoted[id] = Promoted(layer: layer, box: border, origin: want.origin, ink: ink)
            rasterSignatures[id] = signature
        }
        for id in rasterSignatures.keys where promoted[id] == nil {
            rasterSignatures[id] = nil
        }
        var usedContainers: Set<ObjectIdentifier> = []
        for container in containers.values {
            container.zPosition = 0
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for (id, item) in promoted.sorted(by: { WidgetDOM.depth(of: dom.node($0.key) ?? dom.body) < WidgetDOM.depth(of: dom.node($1.key) ?? dom.body) }) {
            let anchor = CGPoint(x: item.box.minX + item.origin.x, y: item.box.minY + item.origin.y)
            var parent: Promoted?
            var walk = dom.node(id)?.parent
            while let node = walk, parent == nil {
                parent = promoted[dom.publicId(node)]
                walk = node.parent
            }
            if let parent = parent {
                if item.layer.superlayer !== parent.layer {
                    parent.layer.addSublayer(item.layer)
                }
                item.layer.position = CGPoint(x: anchor.x - parent.ink.minX, y: anchor.y - parent.ink.minY)
            } else if let clip = clips[id] {
                let container = clipContainer(clip, host: host)
                usedContainers.insert(ObjectIdentifier(container))
                if item.layer.superlayer !== container {
                    container.addSublayer(item.layer)
                }
                item.layer.position = CGPoint(x: anchor.x - clip.rect.minX, y: anchor.y - clip.rect.minY)
                container.zPosition = min(container.zPosition == 0 ? item.layer.zPosition : container.zPosition, item.layer.zPosition)
            } else {
                if item.layer.superlayer !== host {
                    host.addSublayer(item.layer)
                }
                item.layer.position = anchor
            }
        }
        for (key, container) in containers where !usedContainers.contains(ObjectIdentifier(container)) {
            container.removeFromSuperlayer()
            containers[key] = nil
        }
        clipOf = clips.filter { promoted[$0.key] != nil }
        CATransaction.commit()
    }

    struct Clip: Equatable {
        let rect: CGRect
        let radii: [CGFloat]
    }

    private var containers: [String: CALayer] = [:]
    private var clipOf: [Int: Clip] = [:]

    private func clipContainer(_ clip: Clip, host: CALayer) -> CALayer {
        let key = "\(clip.rect)|\(clip.radii)"
        if let existing = containers[key] {
            return existing
        }
        let container = CALayer()
        container.anchorPoint = .zero
        container.frame = clip.rect
        container.masksToBounds = true
        if clip.radii.contains(where: { $0 > 0 }) {
            let mask = CAShapeLayer()
            mask.frame = CGRect(origin: .zero, size: clip.rect.size)
            mask.path = WidgetCompositor.roundedPath(CGRect(origin: .zero, size: clip.rect.size), clip.radii)
            container.mask = mask
        }
        host.addSublayer(container)
        containers[key] = container
        return container
    }

    static func roundedPath(_ rect: CGRect, _ r: [CGFloat]) -> CGPath {
        let path = CGMutablePath()
        guard rect.width > 0, rect.height > 0 else {
            return path
        }
        let k: CGFloat = 0.5522847498
        let hw = rect.width / 2, hh = rect.height / 2
        let tlx = min(max(r[0], 0), hw), tly = min(max(r[1], 0), hh)
        let trx = min(max(r[2], 0), hw), try_ = min(max(r[3], 0), hh)
        let brx = min(max(r[4], 0), hw), bry = min(max(r[5], 0), hh)
        let blx = min(max(r[6], 0), hw), bly = min(max(r[7], 0), hh)
        let x0 = rect.minX, y0 = rect.minY, x1 = rect.maxX, y1 = rect.maxY
        path.move(to: CGPoint(x: x0 + tlx, y: y0))
        path.addLine(to: CGPoint(x: x1 - trx, y: y0))
        if trx > 0 && try_ > 0 {
            path.addCurve(to: CGPoint(x: x1, y: y0 + try_), control1: CGPoint(x: x1 - trx + trx * k, y: y0), control2: CGPoint(x: x1, y: y0 + try_ - try_ * k))
        }
        path.addLine(to: CGPoint(x: x1, y: y1 - bry))
        if brx > 0 && bry > 0 {
            path.addCurve(to: CGPoint(x: x1 - brx, y: y1), control1: CGPoint(x: x1, y: y1 - bry + bry * k), control2: CGPoint(x: x1 - brx + brx * k, y: y1))
        }
        path.addLine(to: CGPoint(x: x0 + blx, y: y1))
        if blx > 0 && bly > 0 {
            path.addCurve(to: CGPoint(x: x0, y: y1 - bly), control1: CGPoint(x: x0 + blx - blx * k, y: y1), control2: CGPoint(x: x0, y: y1 - bly + bly * k))
        }
        path.addLine(to: CGPoint(x: x0, y: y0 + tly))
        if tlx > 0 && tly > 0 {
            path.addCurve(to: CGPoint(x: x0 + tlx, y: y0), control1: CGPoint(x: x0, y: y0 + tly - tly * k), control2: CGPoint(x: x0 + tlx - tlx * k, y: y0))
        }
        path.closeSubpath()
        return path
    }

    private weak var currentDOM: WidgetDOM?

    func resolved(_ text: String?, id: Int) -> String? {
        guard let text = text, text.contains("var(") || text.contains("calc(") else {
            return text
        }
        let node = currentDOM?.node(id)
        return CSSTransform.resolve(text) { name in
            var walk = node
            while let current = walk {
                if let value = current.declarations.last(where: { $0.name == name })?.value {
                    return value
                }
                walk = current.parent
            }
            return nil
        }
    }

    func parse(_ text: String?, box: CGSize, id: Int) -> CGAffineTransform? {
        guard let text = text else {
            return CSSTransform.parse(nil, box: box)
        }
        guard let resolved = resolved(text, id: id) else {
            return nil
        }
        return CSSTransform.parse(resolved, box: box)
    }

    private var rasterSignatures: [Int: Int] = [:]
    var styleKey = 0
    var zOrderStale = true

    static func contentSignature(_ id: Int, dom: WidgetDOM, box: CGRect, scale: CGFloat, styleKey: Int) -> Int? {
        var hasher = Hasher()
        hasher.combine(styleKey)
        hasher.combine(box.width)
        hasher.combine(box.height)
        hasher.combine(scale)
        guard let root = dom.node(id) else {
            return nil
        }
        var cacheable = true
        func visit(_ node: WidgetNode, top: Bool) {
            if node.tag == "canvas" {
                cacheable = false
            }
            hasher.combine(node.tag)
            hasher.combine(node.text)
            for attribute in node.attributes {
                hasher.combine(attribute.name)
                hasher.combine(attribute.value)
            }
            for declaration in node.declarations where !(top && (declaration.name == "transform" || declaration.name == "opacity")) {
                hasher.combine(declaration.name)
                hasher.combine(declaration.value)
            }
            hasher.combine(node.children.count)
            for child in node.children {
                visit(child, top: false)
            }
        }
        visit(root, top: true)
        var node = root
        while let parent = node.parent {
            hasher.combine(parent.children.firstIndex { $0 === node } ?? -1)
            hasher.combine(parent.children.count)
            hasher.combine(parent.tag)
            for attribute in parent.attributes {
                hasher.combine(attribute.name)
                hasher.combine(attribute.value)
            }
            for declaration in parent.declarations {
                hasher.combine(declaration.name)
                hasher.combine(declaration.value)
            }
            node = parent
        }
        return cacheable ? hasher.finalize() : nil
    }

    func documentPoint(for point: CGPoint) -> CGPoint? {
        for item in promoted.values {
            let shown = item.layer.presentation() ?? item.layer
            if shown.opacity <= 0 {
                continue
            }
            if let clip = clipOf.first(where: { promoted[$0.key]?.layer === item.layer })?.value, !clip.rect.contains(point) {
                continue
            }
            let local = shown.convert(point, from: host?.presentation() ?? host)
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
        let layers = Set(promoted.values.map { ObjectIdentifier($0.layer) })
        func paint(_ model: CALayer) {
            let layer = (model.animationKeys()?.isEmpty == false ? model.presentation() : nil) ?? model
            let size = layer.bounds.size
            ctx.saveGState()
            ctx.translateBy(x: layer.position.x, y: layer.position.y)
            ctx.concatenate(CATransform3DGetAffineTransform(layer.transform))
            ctx.translateBy(x: -layer.anchorPoint.x * size.width, y: -layer.anchorPoint.y * size.height)
            ctx.setAlpha(CGFloat(layer.opacity))
            ctx.beginTransparencyLayer(auxiliaryInfo: nil)
            if let contents = model.contents, CFGetTypeID(contents as CFTypeRef) == CGImage.typeID {
                ctx.saveGState()
                ctx.translateBy(x: 0, y: size.height)
                ctx.scaleBy(x: 1, y: -1)
                ctx.draw(contents as! CGImage, in: CGRect(origin: .zero, size: size))
                ctx.restoreGState()
            }
            for child in (model.sublayers ?? []).filter({ layers.contains(ObjectIdentifier($0)) }).sorted(by: { $0.zPosition < $1.zPosition }) {
                paint(child)
            }
            ctx.endTransparencyLayer()
            ctx.restoreGState()
        }
        for item in promoted.values.filter({ $0.layer.superlayer === host }).sorted(by: { $0.layer.zPosition < $1.layer.zPosition }) {
            paint(item.layer)
        }
        for container in containers.values.sorted(by: { $0.zPosition < $1.zPosition }) {
            ctx.saveGState()
            ctx.translateBy(x: container.frame.minX, y: container.frame.minY)
            let clip = clipOf.values.first { $0.rect == container.frame }
            ctx.addPath(WidgetCompositor.roundedPath(CGRect(origin: .zero, size: container.bounds.size), clip?.radii ?? [CGFloat](repeating: 0, count: 8)))
            ctx.clip()
            for child in (container.sublayers ?? []).filter({ layers.contains(ObjectIdentifier($0)) }).sorted(by: { $0.zPosition < $1.zPosition }) {
                paint(child)
            }
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
            rest[id] = nil
            if scripted[id] != nil {
                continue
            }
            let values = driven[id] != nil ? [] : overrides[id] ?? []
            let base = bases(id)
            let transformText = values.last { $0.0 == "transform" }?.1 ?? base["transform"]
            let opacityText = values.last { $0.0 == "opacity" }?.1 ?? base["opacity"]
            let transform = parse(transformText, box: item.box.size, id: id) ?? .identity
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
