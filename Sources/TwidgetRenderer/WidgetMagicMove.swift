import AppKit
import LiteHTML

final class WidgetMagicMove {
    struct Capture {
        let keyed: [String: Int]
        let ancestors: [String: Set<String>]
        let rects: [Int: CGRect]
        let presented: [String: CGRect]
        let signatures: [String: Int]
        var leaving: [MagicMoveStage.Snapshot] = []
        var changed: [String: MagicMoveStage.Snapshot] = [:]
    }

    static let attribute = "data-transition"

    private let stage: MagicMoveStage
    private(set) var excluded: Set<Int> = []
    private(set) var transitions = 0

    var onFinish: (() -> Void)? {
        get { stage.onFinish }
        set { stage.onFinish = newValue }
    }

    init(host: CALayer?) {
        stage = MagicMoveStage(host: host)
    }

    var isActive: Bool {
        return stage.isActive
    }

    static func isStructural(_ ops: [[String: Any]], dom: WidgetDOM) -> Bool {
        func inside(_ id: Any?) -> Bool? {
            guard let id = (id as? NSNumber)?.intValue, var node = dom.node(id) else {
                return nil
            }
            while true {
                if node.attribute(attribute) == "move" {
                    return true
                }
                guard let parent = node.parent else {
                    return false
                }
                node = parent
            }
        }
        for op in ops {
            guard let kind = op["op"] as? String else {
                continue
            }
            switch kind {
            case "ins":
                if inside(op["id"]) == true || inside(op["parent"]) ?? true {
                    return true
                }
            case "rm", "data", "text":
                if inside(op["id"]) == true {
                    return true
                }
            case "attr", "rattr":
                if (op["name"] as? String) == "class", inside(op["id"]) == true {
                    return true
                }
            default:
                break
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

    private static func rects(_ document: LHDocument, of ids: [Int]) -> [Int: CGRect] {
        var rects: [Int: CGRect] = [:]
        let wanted = Set(ids.map { String($0) })
        for (key, value) in document.bounds(byAttribute: WidgetDOM.idAttribute) where wanted.contains(key) {
            if let id = Int(key) {
                rects[id] = value.rectValue
            }
        }
        return rects
    }

    private static func snapshot(_ document: LHDocument, id: Int, scale: CGFloat) -> MagicMoveStage.Snapshot? {
        var ink = CGRect.zero, border = CGRect.zero
        guard let image = document.copyImageOfElement(attribute: WidgetDOM.idAttribute, value: String(id), scale: scale, inkBounds: &ink, borderBox: &border) else {
            return nil
        }
        return MagicMoveStage.Snapshot(image: image, ink: ink, border: border)
    }

    func capture(dom: WidgetDOM, document: LHDocument) -> Capture? {
        var signatures: [String: Int] = [:]
        let (keyed, ancestors) = WidgetMagicMove.keyedNodes(dom, signatures: &signatures)
        guard !keyed.isEmpty else {
            return nil
        }
        return Capture(keyed: keyed, ancestors: ancestors, rects: WidgetMagicMove.rects(document, of: Array(keyed.values)), presented: stage.presentedFrames(), signatures: signatures)
    }

    func collectLeaving(_ capture: inout Capture, dom: WidgetDOM, document: LHDocument, scale: CGFloat) {
        var signatures: [String: Int] = [:]
        let now = WidgetMagicMove.keyedNodes(dom, signatures: &signatures).keyed
        let gone = Set(capture.keyed.keys.filter { now[$0] == nil })
        let changed = Set(capture.signatures.compactMap { entry in signatures[entry.key].flatMap { $0 != entry.value ? entry.key : nil } })
        var pixels = 0
        var count = 0
        for key in (gone.sorted() + changed.sorted()) {
            let group = gone.contains(key) ? gone : changed
            if !(capture.ancestors[key] ?? []).isDisjoint(with: group) {
                continue
            }
            guard let id = capture.keyed[key], let rect = capture.rects[id], rect.width > 0, rect.height > 0 else {
                continue
            }
            pixels += Int(rect.width * scale * rect.height * scale)
            count += 1
            if count > MagicMoveStage.maxLayers / 2 || pixels > MagicMoveStage.maxPixels / 2 {
                break
            }
            if let snapshot = WidgetMagicMove.snapshot(document, id: id, scale: scale) {
                if gone.contains(key) {
                    capture.leaving.append(snapshot)
                } else {
                    capture.changed[key] = snapshot
                }
            }
        }
    }

    func run(_ capture: Capture, dom: WidgetDOM, document: LHDocument, skip: Set<Int>, scale: CGFloat) -> Bool {
        let (keyed, ancestors) = WidgetMagicMove.keyedNodes(dom)
        let rects = WidgetMagicMove.rects(document, of: Array(keyed.values))
        var candidates: [String: (id: Int, from: CGRect?, rect: CGRect)] = [:]
        for (key, id) in keyed {
            guard !skip.contains(id), let rect = rects[id], rect.width > 0, rect.height > 0 else {
                continue
            }
            if let old = capture.presented[key] ?? capture.keyed[key].flatMap({ capture.rects[$0] }) {
                if MagicMoveStage.close(old, rect) {
                    continue
                }
                candidates[key] = (id, old, rect)
            } else {
                candidates[key] = (id, nil, rect)
            }
        }
        let chosen = Set(candidates.keys)
        candidates = candidates.filter { (ancestors[$0.key] ?? []).isDisjoint(with: chosen) }
        if candidates.isEmpty && capture.leaving.isEmpty {
            return false
        }
        var budget = MagicMoveStage.maxPixels
        var items: [MagicMoveStage.Item] = capture.leaving.map { .leave($0) }
        var exclusions: Set<Int> = []
        for (key, item) in candidates.sorted(by: { $0.value.rect.width * $0.value.rect.height > $1.value.rect.width * $1.value.rect.height }) {
            let pixels = Int((item.rect.width + 32) * scale * (item.rect.height + 32) * scale)
            guard items.count < MagicMoveStage.maxLayers, pixels <= budget, let snapshot = WidgetMagicMove.snapshot(document, id: item.id, scale: scale) else {
                stage.cancel()
                excluded = []
                return true
            }
            budget -= pixels
            if let from = item.from {
                items.append(.move(key: key, from: from, to: snapshot, previous: capture.changed[key]))
            } else {
                items.append(.enter(key: key, snapshot))
            }
            exclusions.insert(item.id)
        }
        transitions += 1
        stage.play(items, scale: scale)
        excluded = exclusions
        return true
    }

    func removeLayers() {
        stage.clear()
        excluded = []
    }

    func cancel() {
        stage.cancel()
        excluded = []
    }

    func draw(into ctx: CGContext) {
        stage.draw(into: ctx)
    }
}
