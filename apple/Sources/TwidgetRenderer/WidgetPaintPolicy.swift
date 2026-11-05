import Foundation
import WidgetSandbox

struct WidgetBoxPaint: Equatable {
    var heavy: Bool
    var backdrop: Bool
}

enum WidgetPaintPolicy {
    static func word(_ value: Any?) -> String {
        return (value as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    static func layers(_ value: String) -> [String] {
        var depth = 0
        var current = ""
        var result: [String] = []
        for character in value {
            if character == "(" { depth += 1 }
            if character == ")" { depth = max(0, depth - 1) }
            if character == ",", depth == 0 {
                result.append(current.trimmingCharacters(in: .whitespacesAndNewlines))
                current = ""
            } else {
                current.append(character)
            }
        }
        result.append(current.trimmingCharacters(in: .whitespacesAndNewlines))
        return result.filter { !$0.isEmpty }
    }

    static func blurred(_ layer: String) -> Bool {
        let tokens = layer.split(whereSeparator: { $0.isWhitespace }).map(String.init)
        var lengths: [Double] = []
        var depth = 0
        for token in tokens {
            if depth == 0 {
                if ["calc(", "min(", "max(", "clamp("].contains(where: { token.lowercased().hasPrefix($0) }) {
                    lengths.append(.nan)
                } else if token.range(of: "^[+-]?(?:[0-9]+\\.?[0-9]*|\\.[0-9]+)(?:e[+-]?[0-9]+)?[a-z%]*$", options: [.regularExpression, .caseInsensitive]) != nil {
                    let number = token.replacingOccurrences(of: "[a-z%]+$", with: "", options: [.regularExpression, .caseInsensitive])
                    lengths.append(Double(number) ?? .nan)
                }
            }
            depth += token.filter { $0 == "(" }.count - token.filter { $0 == ")" }.count
            depth = max(0, depth)
        }
        return lengths.count >= 3 && !(lengths[2] <= 0)
    }

    static func shadowBlurred(_ value: String) -> Bool {
        return layers(value).contains(where: blurred)
    }

    static func withoutBlur(_ value: String) -> String {
        let kept = layers(value).filter { $0.lowercased() != "none" && !blurred($0) }
        return kept.isEmpty ? "none" : kept.joined(separator: ", ")
    }

    static func box(_ style: [String: Any], pseudo: Bool = false) -> WidgetBoxPaint? {
        let display = word(style["display"])
        if display == "none" || (!pseudo && display == "contents") { return nil }
        if pseudo && ["", "none", "normal"].contains(word(style["content"])) { return nil }
        let backdrop = !["", "none"].contains(word(style["backdropFilter"]))
        let filtered = !["", "none"].contains(word(style["filter"]))
        let opacity = style["opacity"].map { String(describing: $0) } ?? "1"
        let alpha = Double(opacity.replacingOccurrences(of: "%", with: "")) ?? 1
        let dimmed = !pseudo && ["disabled", "pressed"].contains(word(style["button"]))
        return WidgetBoxPaint(heavy: backdrop || filtered || (!dimmed && alpha / (opacity.hasSuffix("%") ? 100 : 1) < 1) || shadowBlurred(style["boxShadow"] as? String ?? ""), backdrop: backdrop)
    }

    static func animation(_ frames: [[String: Any]]) -> WidgetBoxPaint? {
        var heavy = false, backdrop = false, shadow = false, blur = false, first = false, last = false
        for frame in frames {
            let offset = (frame["computedOffset"] as? NSNumber)?.doubleValue ?? (frame["offset"] as? NSNumber)?.doubleValue
            for (raw, value) in frame {
                let name = raw.replacingOccurrences(of: "([A-Z])", with: "-$1", options: .regularExpression).lowercased().replacingOccurrences(of: "^-(webkit|moz|ms|o)-", with: "", options: .regularExpression)
                switch name {
                case "backdrop-filter": heavy = true; backdrop = true
                case "filter", "opacity": heavy = true
                case "box-shadow":
                    shadow = true
                    blur = blur || ((value as? String).map(shadowBlurred) ?? true)
                    first = first || offset == 0
                    last = last || offset == 1
                default: break
                }
            }
        }
        heavy = heavy || (shadow && (blur || !first || !last))
        return heavy ? WidgetBoxPaint(heavy: true, backdrop: backdrop) : nil
    }
}

final class WidgetPaintCounter {
    private var boxes: [String: WidgetBoxPaint] = [:]
    private var animations: [String: [String: [WidgetBoxPaint]]] = [:]
    private(set) var stop: (code: WidgetPaintLimit, count: Int)?

    func set(_ key: String, _ paint: WidgetBoxPaint?) { boxes[key] = paint }
    func remove(_ key: String) { boxes[key] = nil; animations[key] = nil }
    func start(_ key: String, id: String, paint: WidgetBoxPaint) { animations[key, default: [:]][id, default: []].append(paint) }
    func end(_ key: String, id: String) {
        guard var list = animations[key]?[id], !list.isEmpty else { return }
        list.removeLast()
        animations[key]?[id] = list.isEmpty ? nil : list
    }
    var counts: (heavy: Int, backdrop: Int) {
        var heavy = 0, backdrop = 0
        for (key, box) in boxes {
            let running = animations[key]?.values.flatMap { $0 } ?? []
            if box.heavy || !running.isEmpty { heavy += 1 }
            if box.backdrop || running.contains(where: { $0.backdrop }) { backdrop += 1 }
        }
        return (heavy, backdrop)
    }
    func check() -> (code: WidgetPaintLimit, count: Int)? {
        if stop == nil {
            let count = counts
            if count.heavy > WidgetPaintLimit.maxHeavyBoxes { stop = (.paint, count.heavy) }
            else if count.backdrop > WidgetPaintLimit.maxBackdropBoxes { stop = (.backdrop, count.backdrop) }
        }
        return stop
    }
}
