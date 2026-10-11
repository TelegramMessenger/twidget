import Foundation

struct CSSKeyframe {
    let offset: Double
    let declarations: [(name: String, value: String)]
    let timing: CSSTiming?
}

enum CSSKeyframes {
    static func parse(_ css: String, into rules: inout [String: [CSSKeyframe]]) {
        var rest = Substring(css)
        while let at = rest.range(of: "keyframes", options: .caseInsensitive) {
            let before = rest[..<at.lowerBound]
            guard before.hasSuffix("@") || before.hasSuffix("@-webkit-"), let open = rest[at.upperBound...].firstIndex(of: "{") else {
                rest = rest[at.upperBound...]
                continue
            }
            let name = rest[at.upperBound..<open].trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))
            guard let close = matchingBrace(rest, open) else {
                return
            }
            let body = rest[rest.index(after: open)..<close]
            rules[name] = frames(body)
            rest = rest[rest.index(after: close)...]
        }
    }

    private static func matchingBrace(_ text: Substring, _ open: Substring.Index) -> Substring.Index? {
        var depth = 0
        var index = open
        while index < text.endIndex {
            let ch = text[index]
            if ch == "{" {
                depth += 1
            } else if ch == "}" {
                depth -= 1
                if depth == 0 {
                    return index
                }
            }
            index = text.index(after: index)
        }
        return nil
    }

    private static func frames(_ body: Substring) -> [CSSKeyframe] {
        var result: [CSSKeyframe] = []
        var rest = body
        while let open = rest.firstIndex(of: "{"), let close = rest[open...].firstIndex(of: "}") {
            let selectors = rest[..<open].split(separator: ",").map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
            var declarations: [(name: String, value: String)] = []
            var timing: CSSTiming?
            for declaration in WidgetNode.parseDeclarations(String(rest[rest.index(after: open)..<close])) {
                if declaration.name == "animation-timing-function" {
                    timing = CSSTiming.parse(declaration.value)
                } else {
                    declarations.append((declaration.name, declaration.value))
                }
            }
            for selector in selectors {
                let offset: Double?
                switch selector {
                case "from":
                    offset = 0
                case "to":
                    offset = 1
                default:
                    offset = selector.hasSuffix("%") ? Double(selector.dropLast()).map { $0 / 100 } : nil
                }
                if let offset = offset {
                    result.append(CSSKeyframe(offset: offset, declarations: declarations, timing: timing))
                }
            }
            rest = rest[rest.index(after: close)...]
        }
        return result.sorted { $0.offset < $1.offset }
    }
}

struct CSSTiming {
    let function: (Double) -> Double

    static let linear = CSSTiming { $0 }
    static let ease = CSSTiming.bezier(0.25, 0.1, 0.25, 1)

    static func parse(_ text: String) -> CSSTiming? {
        let t = text.trimmingCharacters(in: .whitespaces).lowercased()
        switch t {
        case "linear":
            return .linear
        case "ease":
            return .ease
        case "ease-in":
            return bezier(0.42, 0, 1, 1)
        case "ease-out":
            return bezier(0, 0, 0.58, 1)
        case "ease-in-out":
            return bezier(0.42, 0, 0.58, 1)
        case "step-start":
            return steps(1, start: true)
        case "step-end":
            return steps(1, start: false)
        default:
            break
        }
        if t.hasPrefix("cubic-bezier("), t.hasSuffix(")") {
            let values = t.dropFirst(13).dropLast().split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
            if values.count == 4 {
                return bezier(values[0], values[1], values[2], values[3])
            }
        }
        if t.hasPrefix("steps("), t.hasSuffix(")") {
            let parts = t.dropFirst(6).dropLast().split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
            if let n = parts.first.flatMap({ Int($0) }), n > 0 {
                let position = parts.count > 1 ? parts[1] : "end"
                return steps(n, start: position == "start" || position == "jump-start")
            }
        }
        if t.hasPrefix("linear(") {
            return .linear
        }
        return nil
    }

    static func steps(_ n: Int, start: Bool) -> CSSTiming {
        return CSSTiming { x in
            let step = start ? ceil(x * Double(n)) : floor(x * Double(n))
            return min(1, max(0, step / Double(n)))
        }
    }

    static func bezier(_ x1: Double, _ y1: Double, _ x2: Double, _ y2: Double) -> CSSTiming {
        let cx = 3 * x1, bx = 3 * (x2 - x1) - cx, ax = 1 - cx - bx
        let cy = 3 * y1, by = 3 * (y2 - y1) - cy, ay = 1 - cy - by
        func sampleX(_ t: Double) -> Double { return ((ax * t + bx) * t + cx) * t }
        func sampleY(_ t: Double) -> Double { return ((ay * t + by) * t + cy) * t }
        func derivativeX(_ t: Double) -> Double { return (3 * ax * t + 2 * bx) * t + cx }
        return CSSTiming { x in
            if x <= 0 {
                return 0
            }
            if x >= 1 {
                return 1
            }
            var t = x
            for _ in 0..<8 {
                let error = sampleX(t) - x
                if abs(error) < 1e-6 {
                    return sampleY(t)
                }
                let d = derivativeX(t)
                if abs(d) < 1e-6 {
                    break
                }
                t -= error / d
            }
            var lo = 0.0, hi = 1.0
            t = x
            for _ in 0..<30 {
                let value = sampleX(t)
                if abs(value - x) < 1e-6 {
                    break
                }
                if value < x {
                    lo = t
                } else {
                    hi = t
                }
                t = (lo + hi) / 2
            }
            return sampleY(t)
        }
    }
}

enum CSSTime {
    static func parse(_ text: String) -> Double? {
        let t = text.trimmingCharacters(in: .whitespaces).lowercased()
        if t.hasSuffix("ms"), let value = Double(t.dropLast(2)) {
            return value / 1000
        }
        if t.hasSuffix("s"), let value = Double(t.dropLast()) {
            return value
        }
        return nil
    }
}

struct CSSAnimationSpec: Equatable {
    var name = "none"
    var duration = 0.0
    var timingText = "ease"
    var delay = 0.0
    var iterations = 1.0
    var direction = "normal"
    var fill = "none"
    var paused = false

    var timing: CSSTiming {
        return CSSTiming.parse(timingText) ?? .ease
    }

    static func list(_ declarations: [String: String]) -> [CSSAnimationSpec] {
        var specs: [CSSAnimationSpec] = []
        if let shorthand = declarations["animation"] {
            for item in WidgetNode.splitTopLevel(shorthand, separator: ",") {
                specs.append(parseShorthand(item))
            }
        }
        func longhand(_ key: String, _ apply: (inout CSSAnimationSpec, String) -> Void) {
            guard let value = declarations[key] else {
                return
            }
            let items = WidgetNode.splitTopLevel(value, separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
            if specs.isEmpty {
                specs = items.map { _ in CSSAnimationSpec() }
            }
            for index in specs.indices where !items.isEmpty {
                apply(&specs[index], items[index % items.count])
            }
        }
        longhand("animation-name") { $0.name = $1 }
        longhand("animation-duration") { $0.duration = CSSTime.parse($1) ?? 0 }
        longhand("animation-timing-function") { $0.timingText = $1 }
        longhand("animation-delay") { $0.delay = CSSTime.parse($1) ?? 0 }
        longhand("animation-iteration-count") { $0.iterations = $1 == "infinite" ? .infinity : (Double($1) ?? 1) }
        longhand("animation-direction") { $0.direction = $1 }
        longhand("animation-fill-mode") { $0.fill = $1 }
        longhand("animation-play-state") { $0.paused = $1 == "paused" }
        return specs.filter { $0.name != "none" && !$0.name.isEmpty }
    }

    private static func parseShorthand(_ text: String) -> CSSAnimationSpec {
        var spec = CSSAnimationSpec()
        var sawDuration = false
        for token in WidgetNode.splitTopLevel(text.trimmingCharacters(in: .whitespaces), separator: " ") where !token.isEmpty {
            let t = token.lowercased()
            if let time = CSSTime.parse(t) {
                if sawDuration {
                    spec.delay = time
                } else {
                    spec.duration = time
                    sawDuration = true
                }
            } else if CSSTiming.parse(t) != nil {
                spec.timingText = t
            } else if t == "infinite" {
                spec.iterations = .infinity
            } else if let count = Double(t) {
                spec.iterations = count
            } else if ["normal", "reverse", "alternate", "alternate-reverse"].contains(t) {
                spec.direction = t
            } else if ["forwards", "backwards", "both"].contains(t) {
                spec.fill = t
            } else if t == "paused" || t == "running" {
                spec.paused = t == "paused"
            } else {
                spec.name = token
            }
        }
        return spec
    }
}

struct CSSTransitionSpec {
    var property: String
    var duration: Double
    var timing: CSSTiming
    var delay: Double

    static func list(_ declarations: [String: String]) -> [CSSTransitionSpec] {
        var specs: [CSSTransitionSpec] = []
        if let shorthand = declarations["transition"] {
            for item in WidgetNode.splitTopLevel(shorthand, separator: ",") {
                var spec = CSSTransitionSpec(property: "all", duration: 0, timing: .ease, delay: 0)
                var sawDuration = false
                for token in WidgetNode.splitTopLevel(item.trimmingCharacters(in: .whitespaces), separator: " ") where !token.isEmpty {
                    if let time = CSSTime.parse(token) {
                        if sawDuration {
                            spec.delay = time
                        } else {
                            spec.duration = time
                            sawDuration = true
                        }
                    } else if let timing = CSSTiming.parse(token) {
                        spec.timing = timing
                    } else {
                        spec.property = token.lowercased()
                    }
                }
                specs.append(spec)
            }
        }
        func items(_ key: String) -> [String] {
            return declarations[key].map { WidgetNode.splitTopLevel($0, separator: ",").map { $0.trimmingCharacters(in: .whitespaces) } } ?? []
        }
        let properties = items("transition-property")
        if !properties.isEmpty {
            let durations = items("transition-duration")
            let timings = items("transition-timing-function")
            let delays = items("transition-delay")
            specs = properties.enumerated().map { index, property in
                let base = index < specs.count ? specs[index] : CSSTransitionSpec(property: property, duration: 0, timing: .ease, delay: 0)
                return CSSTransitionSpec(
                    property: property.lowercased(),
                    duration: durations.isEmpty ? base.duration : (CSSTime.parse(durations[index % durations.count]) ?? 0),
                    timing: timings.isEmpty ? base.timing : (CSSTiming.parse(timings[index % timings.count]) ?? .ease),
                    delay: delays.isEmpty ? base.delay : (CSSTime.parse(delays[index % delays.count]) ?? 0))
            }
        } else {
            let durations = items("transition-duration")
            if !durations.isEmpty {
                for index in specs.indices {
                    specs[index].duration = CSSTime.parse(durations[index % durations.count]) ?? 0
                }
            }
        }
        return specs.filter { $0.duration > 0 && $0.property != "none" }
    }
}

enum CSSInterpolate {
    private static let identityValues: [String: String] = ["scale": "1", "scalex": "1", "scaley": "1", "scale3d": "1,1,1"]

    static func mix(_ a: String, _ b: String, _ t: Double) -> String {
        if t <= 0 {
            return a
        }
        if t >= 1 {
            return b
        }
        var from = normalizeColors(a)
        var to = normalizeColors(b)
        if from.lowercased() == "none", to.contains("(") {
            from = identity(of: to)
        } else if to.lowercased() == "none", from.contains("(") {
            to = identity(of: from)
        }
        let left = tokenize(from), right = tokenize(to)
        guard left.count == right.count else {
            return t < 0.5 ? a : b
        }
        var out = ""
        for (x, y) in zip(left, right) {
            switch (x, y) {
            case let (.number(p, unitP), .number(q, unitQ)) where unitP == unitQ || p == 0 || q == 0:
                let value = p + (q - p) * t
                out += format(value) + (unitP.isEmpty ? unitQ : unitP)
            case let (.text(p), .text(q)) where p == q:
                out += p
            default:
                return t < 0.5 ? a : b
            }
        }
        return out
    }

    private enum Token {
        case number(Double, String)
        case text(String)
    }

    private static func tokenize(_ text: String) -> [Token] {
        var tokens: [Token] = []
        let scalars = Array(text.unicodeScalars)
        var i = 0
        var pending = ""
        func flush() {
            if !pending.isEmpty {
                tokens.append(.text(pending))
                pending = ""
            }
        }
        while i < scalars.count {
            let c = scalars[i]
            let startsNumber = CharacterSet.decimalDigits.contains(c) || (c == "." && i + 1 < scalars.count && CharacterSet.decimalDigits.contains(scalars[i + 1])) || ((c == "-" || c == "+") && i + 1 < scalars.count && (CharacterSet.decimalDigits.contains(scalars[i + 1]) || scalars[i + 1] == "."))
            let previous: Unicode.Scalar? = i > 0 ? scalars[i - 1] : nil
            let identPart = previous.map { CharacterSet.letters.contains($0) || $0 == "-" && i > 1 && CharacterSet.letters.contains(scalars[i - 2]) } ?? false
            if startsNumber && !(identPart && c != "-") && !(previous == "#") {
                var j = i + 1
                while j < scalars.count, CharacterSet.decimalDigits.contains(scalars[j]) || scalars[j] == "." || ((scalars[j] == "e" || scalars[j] == "E") && j + 1 < scalars.count && (CharacterSet.decimalDigits.contains(scalars[j + 1]) || scalars[j + 1] == "-")) {
                    if scalars[j] == "e" || scalars[j] == "E" {
                        j += 1
                    }
                    j += 1
                }
                let number = String(String.UnicodeScalarView(scalars[i..<j]))
                var k = j
                while k < scalars.count, CharacterSet.letters.contains(scalars[k]) || scalars[k] == "%" {
                    k += 1
                }
                let unit = String(String.UnicodeScalarView(scalars[j..<k]))
                if let value = Double(number) {
                    flush()
                    tokens.append(.number(value, unit))
                    i = k
                    continue
                }
            }
            pending.unicodeScalars.append(c)
            i += 1
        }
        flush()
        return tokens
    }

    private static func format(_ value: Double) -> String {
        if value == value.rounded(), abs(value) < 1e9 {
            return String(Int(value))
        }
        return String(format: "%.4f", value).replacingOccurrences(of: "0+$", with: "", options: .regularExpression)
    }

    private static func identity(of transform: String) -> String {
        var out = ""
        var rest = Substring(transform)
        while let open = rest.firstIndex(of: "("), let close = rest[open...].firstIndex(of: ")") {
            let name = rest[..<open].trimmingCharacters(in: .whitespaces)
            let args = WidgetNode.splitTopLevel(String(rest[rest.index(after: open)..<close]), separator: ",")
            let identity = identityValues[name.lowercased()].map { value in args.map { _ in value }.joined(separator: ",") } ?? args.map { arg -> String in
                let unit = arg.trimmingCharacters(in: .whitespaces).drop { "0123456789.-+".contains($0) }
                return "0" + unit
            }.joined(separator: ",")
            out += (out.isEmpty ? "" : " ") + name + "(" + identity + ")"
            rest = rest[rest.index(after: close)...]
        }
        return out.isEmpty ? transform : out
    }

    private static let named: [String: (Double, Double, Double, Double)] = [
        "transparent": (0, 0, 0, 0), "white": (255, 255, 255, 1), "black": (0, 0, 0, 1), "red": (255, 0, 0, 1), "green": (0, 128, 0, 1),
        "blue": (0, 0, 255, 1), "gray": (128, 128, 128, 1), "grey": (128, 128, 128, 1), "orange": (255, 165, 0, 1), "yellow": (255, 255, 0, 1),
    ]

    static func normalizeColors(_ text: String) -> String {
        var out = ""
        let scalars = Array(text.unicodeScalars)
        var i = 0
        while i < scalars.count {
            if scalars[i] == "#" {
                var j = i + 1
                while j < scalars.count, CharacterSet(charactersIn: "0123456789abcdefABCDEF").contains(scalars[j]) {
                    j += 1
                }
                let hex = String(String.UnicodeScalarView(scalars[(i + 1)..<j]))
                if let rgba = parseHex(hex) {
                    out += rgbaString(rgba)
                    i = j
                    continue
                }
            }
            if CharacterSet.letters.contains(scalars[i]), i == 0 || !CharacterSet.alphanumerics.contains(scalars[i - 1]) && scalars[i - 1] != "-" {
                var j = i
                while j < scalars.count, CharacterSet.letters.contains(scalars[j]) || scalars[j] == "-" {
                    j += 1
                }
                let word = String(String.UnicodeScalarView(scalars[i..<j]))
                if j < scalars.count, scalars[j] == "(" {
                    let lower = word.lowercased()
                    if lower == "rgb" || lower == "rgba", let close = scalars[j...].firstIndex(of: ")") {
                        let inner = String(String.UnicodeScalarView(scalars[(j + 1)..<close]))
                        let parts = inner.split(whereSeparator: { $0 == "," || $0 == " " || $0 == "/" }).map(String.init)
                        let values = parts.compactMap { part -> Double? in
                            part.hasSuffix("%") ? Double(part.dropLast()).map { $0 / 100 } : Double(part)
                        }
                        if values.count >= 3 {
                            out += rgbaString((values[0], values[1], values[2], values.count > 3 ? values[3] : 1))
                            i = close + 1
                            continue
                        }
                    }
                } else if let rgba = named[word.lowercased()] {
                    out += rgbaString(rgba)
                    i = j
                    continue
                }
                out += word
                i = j
                continue
            }
            out.unicodeScalars.append(scalars[i])
            i += 1
        }
        return out
    }

    private static func parseHex(_ hex: String) -> (Double, Double, Double, Double)? {
        let chars = Array(hex)
        func value(_ s: String) -> Double? { return UInt8(s, radix: 16).map(Double.init) }
        switch chars.count {
        case 3, 4:
            let parts = chars.map { String([$0, $0]) }
            guard let r = value(parts[0]), let g = value(parts[1]), let b = value(parts[2]) else { return nil }
            return (r, g, b, chars.count == 4 ? (value(parts[3]) ?? 255) / 255 : 1)
        case 6, 8:
            let parts = stride(from: 0, to: chars.count, by: 2).map { String(chars[$0..<$0 + 2]) }
            guard let r = value(parts[0]), let g = value(parts[1]), let b = value(parts[2]) else { return nil }
            return (r, g, b, chars.count == 8 ? (value(parts[3]) ?? 255) / 255 : 1)
        default:
            return nil
        }
    }

    private static func rgbaString(_ c: (Double, Double, Double, Double)) -> String {
        return "rgba(\(format(c.0)), \(format(c.1)), \(format(c.2)), \(format(c.3)))"
    }
}

final class WidgetAnimator {
    private struct Running {
        let spec: CSSAnimationSpec
        let start: CFTimeInterval
        var iteration = 0
        var ended = false
    }

    private struct Transition {
        let from: String
        let to: String
        let start: CFTimeInterval
        let spec: CSSTransitionSpec
    }

    static let initialValues: [String: String] = ["opacity": "1", "transform": "none", "translate": "0px", "scale": "1", "rotate": "0deg", "filter": "none", "letter-spacing": "0px"]

    private(set) var keyframes: [String: [CSSKeyframe]] = [:]
    private var running: [Int: [Running]] = [:]
    private var transitions: [Int: [String: Transition]] = [:]
    private var bases: [Int: [String: String]] = [:]
    private var lastOverrides: [Int: [(String, String)]] = [:]
    var onEvent: ((String, Int, String, Double) -> Void)?

    static let entryProperties = ["animation", "animation-name", "transition", "transition-property"]

    static let watched = [
        "animation", "animation-name", "animation-duration", "animation-timing-function", "animation-delay", "animation-iteration-count",
        "animation-direction", "animation-fill-mode", "animation-play-state", "transition", "transition-property", "transition-duration",
        "transition-timing-function", "transition-delay", "transform", "opacity", "color", "background-color", "background", "width", "height",
        "top", "left", "right", "bottom", "box-shadow", "border-color", "max-height", "stroke-dashoffset", "fill", "stroke", "translate",
        "scale", "rotate", "filter", "letter-spacing", "font-size", "margin-top", "margin-left", "flex-grow",
    ]

    var isActive: Bool {
        return !transitions.isEmpty || running.values.contains { list in list.contains { !$0.ended && !$0.spec.paused && keyframes[$0.spec.name] != nil } }
    }

    func setStyleSheets(_ texts: [String]) {
        var rules: [String: [CSSKeyframe]] = [:]
        for text in texts {
            CSSKeyframes.parse(text, into: &rules)
        }
        keyframes = rules
    }

    func update(declarations: [Int: [String: String]], now: CFTimeInterval) {
        var nextRunning: [Int: [Running]] = [:]
        for (id, values) in declarations {
            let specs = CSSAnimationSpec.list(values)
            if !specs.isEmpty {
                let previous = running[id] ?? []
                nextRunning[id] = specs.map { spec in
                    if let existing = previous.first(where: { $0.spec.name == spec.name }) {
                        if existing.spec == spec {
                            return existing
                        }
                        return Running(spec: spec, start: existing.start, iteration: existing.iteration, ended: false)
                    }
                    if keyframes[spec.name] != nil {
                        onEvent?("animationstart", id, spec.name, 0)
                    }
                    return Running(spec: spec, start: now)
                }
            }
            let transitionSpecs = CSSTransitionSpec.list(values)
            var base = bases[id] ?? [:]
            for property in WidgetAnimator.watched where !property.hasPrefix("animation") && !property.hasPrefix("transition") {
                let newValue = values[property]
                let oldValue = base[property]
                defer { base[property] = newValue }
                guard let old = oldValue, let new = newValue, old != new else {
                    if newValue == nil {
                        transitions[id]?[property] = nil
                    }
                    continue
                }
                guard let spec = transitionSpecs.last(where: { $0.property == property || $0.property == "all" }) else {
                    transitions[id]?[property] = nil
                    continue
                }
                let current = currentTransitionValue(id, property, now) ?? old
                transitions[id, default: [:]][property] = Transition(from: current, to: new, start: now, spec: spec)
            }
            bases[id] = base
            if transitions[id]?.isEmpty == true {
                transitions[id] = nil
            }
        }
        for id in bases.keys where declarations[id] == nil {
            bases[id] = nil
            transitions[id] = nil
        }
        running = nextRunning
    }

    private func currentTransitionValue(_ id: Int, _ property: String, _ now: CFTimeInterval) -> String? {
        guard let transition = transitions[id]?[property] else {
            return nil
        }
        let elapsed = now - transition.start - transition.spec.delay
        if elapsed <= 0 {
            return transition.from
        }
        let progress = min(1, elapsed / transition.spec.duration)
        return CSSInterpolate.mix(transition.from, transition.to, transition.spec.timing.function(progress))
    }

    func sample(now: CFTimeInterval) -> [Int: [(String, String)]] {
        var overrides: [Int: [(String, String)]] = [:]
        for (id, list) in running {
            var updated = list
            for index in updated.indices {
                let item = updated[index]
                guard let frames = keyframes[item.spec.name], !frames.isEmpty else {
                    continue
                }
                if let values = sampleAnimation(item, frames: frames, now: now, id: id, state: &updated[index]) {
                    for value in values {
                        overrides[id, default: []].removeAll { $0.0 == value.0 }
                        overrides[id, default: []].append(value)
                    }
                }
            }
            running[id] = updated
        }
        for (id, list) in transitions {
            for (property, transition) in list {
                let elapsed = now - transition.start - transition.spec.delay
                if elapsed >= transition.spec.duration {
                    transitions[id]?[property] = nil
                    onEvent?("transitionend", id, property, transition.spec.duration)
                    continue
                }
                let progress = max(0, elapsed) / transition.spec.duration
                let value = elapsed <= 0 ? transition.from : CSSInterpolate.mix(transition.from, transition.to, transition.spec.timing.function(progress))
                overrides[id, default: []].removeAll { $0.0 == property }
                overrides[id, default: []].append((property, value))
            }
            if transitions[id]?.isEmpty == true {
                transitions[id] = nil
            }
        }
        lastOverrides = overrides
        return overrides
    }

    private func sampleAnimation(_ item: Running, frames: [CSSKeyframe], now: CFTimeInterval, id: Int, state: inout Running) -> [(String, String)]? {
        let spec = item.spec
        let local = now - item.start - spec.delay
        if local < 0 {
            if spec.fill == "backwards" || spec.fill == "both" {
                return values(frames: frames, base: bases[id] ?? [:], progress: spec.direction.hasPrefix("reverse") || spec.direction == "alternate-reverse" ? 1 : 0, spec: spec)
            }
            return nil
        }
        let duration = max(spec.duration, 0.0001)
        let total = spec.iterations * duration
        if local >= total {
            if !state.ended {
                state.ended = true
                onEvent?("animationend", id, spec.name, total)
            }
            if spec.fill == "forwards" || spec.fill == "both" {
                let lastIteration = max(0, Int(ceil(min(spec.iterations, 1e9))) - 1)
                var progress = spec.iterations.truncatingRemainder(dividingBy: 1) == 0 ? 1.0 : spec.iterations.truncatingRemainder(dividingBy: 1)
                if reversed(spec.direction, lastIteration) {
                    progress = 1 - progress
                }
                return values(frames: frames, base: bases[id] ?? [:], progress: progress, spec: spec)
            }
            return nil
        }
        let iteration = Int(min(local / duration, 1e12))
        if iteration != state.iteration {
            state.iteration = iteration
            onEvent?("animationiteration", id, spec.name, local)
        }
        var progress = (local - Double(iteration) * duration) / duration
        if reversed(spec.direction, iteration) {
            progress = 1 - progress
        }
        return values(frames: frames, base: bases[id] ?? [:], progress: progress, spec: spec)
    }

    private func reversed(_ direction: String, _ iteration: Int) -> Bool {
        switch direction {
        case "reverse":
            return true
        case "alternate":
            return iteration % 2 == 1
        case "alternate-reverse":
            return iteration % 2 == 0
        default:
            return false
        }
    }

    private func values(frames: [CSSKeyframe], base: [String: String], progress: Double, spec: CSSAnimationSpec) -> [(String, String)] {
        var properties: [String] = []
        for frame in frames {
            for declaration in frame.declarations where !properties.contains(declaration.name) {
                properties.append(declaration.name)
            }
        }
        var result: [(String, String)] = []
        for property in properties {
            var stops = frames.compactMap { frame -> (Double, String, CSSTiming?)? in
                guard let value = frame.declarations.last(where: { $0.name == property })?.value else {
                    return nil
                }
                return (frame.offset, value, frame.timing)
            }
            if let own = base[property] ?? WidgetAnimator.initialValues[property] {
                if let first = stops.first, first.0 > 0 {
                    stops.insert((0, own, frames.first?.timing), at: 0)
                }
                if let last = stops.last, last.0 < 1 {
                    stops.append((1, own, nil))
                }
            }
            guard let first = stops.first, let last = stops.last else {
                continue
            }
            if progress <= first.0 {
                result.append((property, first.1))
                continue
            }
            if progress >= last.0 {
                result.append((property, last.1))
                continue
            }
            for index in 0..<(stops.count - 1) {
                let a = stops[index], b = stops[index + 1]
                if progress >= a.0 && progress <= b.0 {
                    let span = max(b.0 - a.0, 0.000001)
                    let local = (progress - a.0) / span
                    let timing = a.2 ?? spec.timing
                    result.append((property, CSSInterpolate.mix(a.1, b.1, timing.function(local))))
                    break
                }
            }
        }
        return result
    }
}
