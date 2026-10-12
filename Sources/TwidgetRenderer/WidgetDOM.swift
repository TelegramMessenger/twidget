import Foundation

final class WidgetNode {
    let id: Int
    let tag: String
    let svg: Bool
    let isText: Bool
    var synthetic = false
    var text: String {
        didSet {
            cache = nil
            WidgetDOM.revision &+= 1
        }
    }
    var cache: [UInt8]?
    var cacheEpoch = 0
    var attributes: [(name: String, value: String)] = []
    var declarations: [(name: String, value: String, important: Bool)] = []
    var children: [WidgetNode] = []
    weak var parent: WidgetNode?

    init(id: Int, tag: String, svg: Bool) {
        self.id = id
        self.tag = tag
        self.svg = svg
        isText = false
        text = ""
    }

    init(id: Int, text: String) {
        self.id = id
        tag = "#text"
        svg = false
        isText = true
        self.text = text
    }

    func attribute(_ name: String) -> String? {
        let key = name.lowercased()
        return attributes.first { $0.name.lowercased() == key }?.value
    }

    func setAttribute(_ name: String, _ value: String) {
        cache = nil
        WidgetDOM.revision &+= 1
        if name.lowercased() == "style" {
            declarations = WidgetNode.parseDeclarations(value)
            return
        }
        let key = name.lowercased()
        if let index = attributes.firstIndex(where: { $0.name.lowercased() == key }) {
            attributes[index].value = value
        } else {
            attributes.append((name, value))
        }
    }

    func removeAttribute(_ name: String) {
        cache = nil
        WidgetDOM.revision &+= 1
        if name.lowercased() == "style" {
            declarations.removeAll()
            return
        }
        let key = name.lowercased()
        attributes.removeAll { $0.name.lowercased() == key }
    }

    func setProperty(_ name: String, _ value: String, important: Bool) {
        cache = nil
        WidgetDOM.revision &+= 1
        let key = name.hasPrefix("--") ? name : name.lowercased()
        if !WidgetNames.isProperty(key) {
            return
        }
        if value.isEmpty {
            declarations.removeAll { $0.name == key }
            return
        }
        if let index = declarations.firstIndex(where: { $0.name == key }) {
            declarations[index] = (key, value, important)
        } else {
            declarations.append((key, value, important))
        }
    }

    static func parseDeclarations(_ text: String) -> [(name: String, value: String, important: Bool)] {
        var result: [(name: String, value: String, important: Bool)] = []
        for part in splitTopLevel(text, separator: ";") {
            guard let colon = part.firstIndex(of: ":") else {
                continue
            }
            let name = part[..<colon].trimmingCharacters(in: .whitespacesAndNewlines)
            var value = part[part.index(after: colon)...].trimmingCharacters(in: .whitespacesAndNewlines)
            if name.isEmpty || value.isEmpty {
                continue
            }
            var important = false
            if let range = value.range(of: "!important", options: [.caseInsensitive, .backwards]), value[range.upperBound...].trimmingCharacters(in: .whitespaces).isEmpty {
                important = true
                value = value[..<range.lowerBound].trimmingCharacters(in: .whitespaces)
            }
            let key = name.hasPrefix("--") ? name : name.lowercased()
            if !WidgetNames.isProperty(key) {
                continue
            }
            result.removeAll { $0.name == key }
            result.append((key, value, important))
        }
        return result
    }

    static func splitTopLevel(_ text: String, separator: Character) -> [String] {
        var parts: [String] = []
        var current = ""
        var depth = 0
        var quote: Character?
        var escaped = false
        for ch in text {
            if escaped {
                escaped = false
                current.append(ch)
                continue
            }
            if ch == "\\" {
                escaped = true
                current.append(ch)
                continue
            }
            if let q = quote {
                if ch == q {
                    quote = nil
                }
                current.append(ch)
                continue
            }
            switch ch {
            case "\"", "'":
                quote = ch
            case "(":
                depth += 1
            case ")":
                depth = max(0, depth - 1)
            default:
                break
            }
            if ch == separator && depth == 0 {
                parts.append(current)
                current = ""
            } else {
                current.append(ch)
            }
        }
        if !current.isEmpty {
            parts.append(current)
        }
        return parts
    }
}

enum WidgetNames {
    static func isTag(_ name: String) -> Bool {
        return valid(name, maxLength: 64, first: { $0.isASCIILetter }, rest: { $0.isASCIILetter || $0.isASCIIDigit || $0 == "-" })
    }

    static func isAttribute(_ name: String) -> Bool {
        return valid(name, maxLength: 64, first: { $0.isASCIILetter || $0 == "_" || $0 == ":" }, rest: { $0.isASCIILetter || $0.isASCIIDigit || $0 == "_" || $0 == ":" || $0 == "." || $0 == "-" })
    }

    static func isProperty(_ name: String) -> Bool {
        let body = name.hasPrefix("--") ? String(name.dropFirst(2)) : (name.hasPrefix("-") ? String(name.dropFirst()) : name)
        return valid(body, maxLength: 128, first: { $0.isASCIILetter || $0.isASCIIDigit || $0 == "-" || $0 == "_" }, rest: { $0.isASCIILetter || $0.isASCIIDigit || $0 == "-" || $0 == "_" })
    }

    private static func valid(_ name: String, maxLength: Int, first: (Unicode.Scalar) -> Bool, rest: (Unicode.Scalar) -> Bool) -> Bool {
        let scalars = name.unicodeScalars
        guard let head = scalars.first, scalars.count <= maxLength, first(head) else {
            return false
        }
        return scalars.dropFirst().allSatisfy(rest)
    }
}

private extension Unicode.Scalar {
    var isASCIILetter: Bool {
        return (value >= 65 && value <= 90) || (value >= 97 && value <= 122)
    }

    var isASCIIDigit: Bool {
        return value >= 48 && value <= 57
    }
}

final class WidgetDOM {
    static let maxNodes = 20000
    static let maxDepth = 256
    static let maxTextLength = 1 << 20
    static let blockedElements: Set<String> = ["script", "iframe", "frame", "frameset", "object", "embed", "link", "meta", "base", "portal", "noscript", "set", "template", "audio", "video", "source", "track"]
    static let blockedAttributes: Set<String> = ["srcdoc", "srcset", "action", "formaction", "ping", "background", "nonce", "target", "http-equiv"]
    static let idAttribute = "data-lh-id"

    let html: WidgetNode
    let head: WidgetNode
    let body: WidgetNode
    private(set) var nodes: [Int: WidgetNode] = [:]
    private(set) var usesTransitions = false
    var onViolation: ((String) -> Void)?
    var onCanvas: ((WidgetNode, [Any]) -> Void)?
    var onCall: ((WidgetNode, String, [Any]) -> Void)?
    var onProperty: ((WidgetNode, String, Any) -> Void)?

    init() {
        html = WidgetNode(id: -1, tag: "html", svg: false)
        head = WidgetNode(id: -2, tag: "head", svg: false)
        body = WidgetNode(id: -3, tag: "body", svg: false)
        head.parent = html
        body.parent = html
        html.children = [head, body]
    }

    func node(_ id: Int) -> WidgetNode? {
        return nodes[id]
    }

    private var rootIds: [ObjectIdentifier: Int] = [:]

    func publicId(_ node: WidgetNode) -> Int {
        if node.id >= 0 {
            return node.id
        }
        return rootIds[ObjectIdentifier(node)] ?? 0
    }

    private func register(_ id: Int, _ node: WidgetNode) {
        node.cache = nil
        WidgetDOM.revision &+= 1
        nodes[id] = node
        if node.id < 0 {
            rootIds[ObjectIdentifier(node)] = id
        }
    }

    private func unregister(_ node: WidgetNode) {
        WidgetDOM.revision &+= 1
        let key = publicId(node)
        if key != 0, nodes[key] === node {
            nodes[key] = nil
        }
        for child in node.children {
            unregister(child)
        }
    }

    private func isRoot(_ node: WidgetNode) -> Bool {
        return node === html || node === head || node === body
    }

    private func detach(_ node: WidgetNode) {
        guard let parent = node.parent else {
            return
        }
        parent.children.removeAll { $0 === node }
        node.parent = nil
    }

    private func violation(_ detail: String) {
        onViolation?(detail)
    }

    private func setAttribute(_ node: WidgetNode, _ name: String, _ value: String) {
        if !WidgetNames.isAttribute(name) {
            violation("attribute name")
            return
        }
        let key = name.lowercased()
        if key.hasPrefix("on") || WidgetDOM.blockedAttributes.contains(key) {
            violation("attribute \(name)")
            return
        }
        if key.hasPrefix("data-lh-") {
            return
        }
        if key == "data-transition" {
            usesTransitions = true
        }
        node.setAttribute(name, value)
    }

    func apply(_ op: [String: Any]) {
        WidgetDOM.revision &+= 1
        guard let kind = op["op"] as? String else {
            return
        }
        let id = (op["id"] as? NSNumber)?.intValue ?? 0
        switch kind {
        case "el":
            let tag = (op["tag"] as? String) ?? "span"
            if !WidgetNames.isTag(tag) || id <= 0 {
                violation("element name")
                return
            }
            if nodes[id] == nil && nodes.count >= WidgetDOM.maxNodes {
                violation("too many nodes")
                return
            }
            if WidgetDOM.blockedElements.contains(tag.lowercased()) {
                violation("element <\(tag)>")
                return
            }
            let node: WidgetNode
            switch op["adopt"] as? String {
            case "html":
                node = html
            case "head":
                node = head
            case "body":
                node = body
            default:
                let svg = (op["ns"] as? String) == "svg"
                node = WidgetNode(id: id, tag: svg ? tag : tag.lowercased(), svg: svg)
            }
            register(id, node)
            for pair in (op["attrs"] as? [[Any]]) ?? [] where pair.count == 2 {
                if let name = pair[0] as? String, let value = pair[1] as? String {
                    setAttribute(node, name, value)
                }
            }
            return
        case "text":
            if id <= 0 || (nodes[id] == nil && nodes.count >= WidgetDOM.maxNodes) {
                violation("too many nodes")
                return
            }
            register(id, WidgetNode(id: id, text: WidgetDOM.clip((op["data"] as? String) ?? "")))
            return
        case "comment":
            return
        default:
            break
        }
        guard let node = nodes[id] else {
            return
        }
        switch kind {
        case "attr":
            if let name = op["name"] as? String, let value = op["value"] as? String {
                setAttribute(node, name, value)
            }
        case "rattr":
            if let name = op["name"] as? String {
                node.removeAttribute(name)
            }
        case "data":
            node.text = WidgetDOM.clip((op["data"] as? String) ?? "")
        case "ins":
            if isRoot(node) {
                return
            }
            let parentId = (op["parent"] as? NSNumber)?.intValue ?? 0
            if parentId == 0 {
                detach(node)
                return
            }
            guard let parent = nodes[parentId], !parent.isText else {
                return
            }
            var ancestor: WidgetNode? = parent
            while let current = ancestor {
                if current === node {
                    return
                }
                ancestor = current.parent
            }
            if WidgetDOM.depth(of: parent) + WidgetDOM.height(of: node) >= WidgetDOM.maxDepth {
                violation("tree too deep")
                return
            }
            let before = (op["before"] as? NSNumber).flatMap { nodes[$0.intValue] }
            detach(node)
            node.parent = parent
            if let before = before, before.parent === parent, let index = parent.children.firstIndex(where: { $0 === before }) {
                parent.children.insert(node, at: index)
            } else {
                parent.children.append(node)
            }
        case "rm":
            if isRoot(node) {
                return
            }
            detach(node)
            unregister(node)
        case "style":
            if let name = op["name"] as? String {
                node.setProperty(name, (op["value"] as? String) ?? "", important: (op["important"] as? Bool) ?? false)
            }
        case "css":
            if node.tag == "style" {
                for child in node.children {
                    child.parent = nil
                }
                let text = WidgetNode(id: -4, text: WidgetDOM.clip((op["text"] as? String) ?? ""))
                text.synthetic = true
                text.parent = node
                node.children = [text]
            }
        case "prop":
            if let name = op["name"] as? String {
                onProperty?(node, name, op["value"] ?? NSNull())
            }
        case "call":
            if let method = op["method"] as? String {
                onCall?(node, method, (op["args"] as? [Any]) ?? [])
            }
        case "canvas":
            if node.tag == "canvas" {
                onCanvas?(node, (op["cmds"] as? [Any]) ?? [])
            }
        default:
            break
        }
    }

    static func clip(_ text: String) -> String {
        return text.utf8.count > maxTextLength ? String(text.prefix(maxTextLength / 4)) : text
    }

    static func depth(of node: WidgetNode) -> Int {
        var depth = 0
        var current: WidgetNode? = node
        while let next = current?.parent {
            depth += 1
            current = next
        }
        return depth
    }

    static func height(of node: WidgetNode) -> Int {
        var best = 0
        var stack: [(WidgetNode, Int)] = [(node, 1)]
        while let (current, level) = stack.popLast() {
            best = max(best, level)
            if level > maxDepth {
                return level
            }
            for child in current.children {
                stack.append((child, level + 1))
            }
        }
        return best
    }

    private var epoch = 0
    private var epochTheme = ""
    static var revision: UInt64 = 0

    static func invalidatingCaches(_ dom: WidgetDOM) -> WidgetDOM {
        dom.epoch += 1
        WidgetDOM.revision &+= 1
        return dom
    }

    func serialize(theme: String, overrides: [Int: [(String, String)]] = [:], canvasSource: (WidgetNode) -> String? = { _ in nil }) -> String {
        if theme != epochTheme {
            epochTheme = theme
            epoch += 1
        }
        var out: [UInt8] = []
        out.reserveCapacity(lastSize + 1024)
        out.append(contentsOf: Array("<!DOCTYPE html>".utf8))
        write(html, into: &out, theme: theme, overrides: overrides, canvasSource: canvasSource)
        lastSize = out.count
        return String(decoding: out, as: UTF8.self)
    }

    private var lastSize = 16 * 1024

    func styleSheets() -> [String] {
        var texts: [String] = []
        func visit(_ node: WidgetNode) {
            if node.tag == "style" {
                texts.append(node.children.map { $0.text }.joined())
                return
            }
            for child in node.children where !child.isText {
                visit(child)
            }
        }
        visit(html)
        return texts
    }

    private static let voidElements: Set<String> = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"]

    private func write(_ node: WidgetNode, into out: inout [UInt8], theme: String, overrides: [Int: [(String, String)]], canvasSource: (WidgetNode) -> String?) {
        if node.isText {
            if node.cacheEpoch != epoch || node.cache == nil {
                var bytes: [UInt8] = []
                if let parent = node.parent, parent.tag == "style" {
                    bytes.append(contentsOf: Array(CSSTheme.resolve(node.text, theme: theme).replacingOccurrences(of: "</", with: "<\\/").utf8))
                } else {
                    WidgetDOM.escape(node.text, into: &bytes, attribute: false)
                }
                node.cache = bytes
                node.cacheEpoch = epoch
            }
            out.append(contentsOf: node.cache!)
            return
        }
        if !WidgetNames.isTag(node.tag) {
            return
        }
        let id = publicId(node)
        let canvas = node.tag == "canvas" && !node.svg ? canvasSource(node) : nil
        let animated = overrides[id]
        if canvas == nil && animated == nil && node.cacheEpoch == epoch, let cached = node.cache {
            out.append(contentsOf: cached)
        } else {
            var open: [UInt8] = []
            writeOpenTag(node, id: id, canvas: canvas, animated: animated, theme: theme, into: &open)
            if canvas == nil && animated == nil {
                node.cache = open
                node.cacheEpoch = epoch
            } else {
                node.cache = nil
            }
            out.append(contentsOf: open)
        }
        if canvas != nil {
            out.append(contentsOf: Array("</canvas>".utf8))
            return
        }
        if !node.svg && WidgetDOM.voidElements.contains(node.tag) {
            return
        }
        for child in node.children {
            write(child, into: &out, theme: theme, overrides: overrides, canvasSource: canvasSource)
        }
        out.append(contentsOf: [60, 47])
        WidgetDOM.append(node.tag, to: &out)
        out.append(62)
    }

    private func writeOpenTag(_ node: WidgetNode, id: Int, canvas: String?, animated: [(String, String)]?, theme: String, into out: inout [UInt8]) {
        func put(_ text: String) {
            WidgetDOM.append(text, to: &out)
        }
        put("<")
        put(node.tag)
        put(" data-lh-id=\"")
        put(String(id))
        put("\"")
        if let source = canvas {
            put(" src=\"")
            WidgetDOM.escape(source, into: &out, attribute: true)
            put("\"")
        }
        for attribute in node.attributes where (canvas == nil || attribute.name.lowercased() != "src") && WidgetNames.isAttribute(attribute.name) {
            put(" ")
            put(attribute.name)
            put("=\"")
            WidgetDOM.escape(attribute.value, into: &out, attribute: true)
            put("\"")
        }
        if !node.declarations.isEmpty || animated != nil {
            put(" style=\"")
            WidgetDOM.writeStyle(node, animated: animated, theme: theme, into: &out)
            put("\"")
        }
        put(">")
    }

    static func styleValue(_ node: WidgetNode, animated: [(String, String)]?, theme: String) -> String? {
        if node.isText || node.declarations.isEmpty && animated == nil {
            return nil
        }
        var out: [UInt8] = []
        writeStyle(node, animated: animated, theme: theme, into: &out)
        return String(decoding: out, as: UTF8.self)
    }

    private static func writeStyle(_ node: WidgetNode, animated: [(String, String)]?, theme: String, into out: inout [UInt8]) {
        func put(_ text: String) {
            WidgetDOM.append(text, to: &out)
        }
        for declaration in node.declarations where declaration.name != "-lh-anim" && WidgetNames.isProperty(declaration.name) {
            put(declaration.name)
            put(":")
            WidgetDOM.escape(CSSTheme.resolve(declaration.value, theme: theme), into: &out, attribute: true)
            if declaration.important {
                put(" !important")
            }
            put(";")
        }
        if let animated = animated {
            put("-lh-anim:1;")
            for (name, value) in animated where WidgetNames.isProperty(name) {
                put(name)
                put(":")
                WidgetDOM.escape(CSSTheme.resolve(value, theme: theme), into: &out, attribute: true)
                put(";")
            }
        }
    }

    static func append(_ text: String, to out: inout [UInt8]) {
        var text = text
        text.withUTF8 { bytes in
            out.append(contentsOf: bytes)
        }
    }

    static func escape(_ text: String, into out: inout [UInt8], attribute: Bool) {
        var text = text
        text.withUTF8 { bytes in
            escape(bytes, into: &out, attribute: attribute)
        }
    }

    private static func escape(_ bytes: UnsafeBufferPointer<UInt8>, into out: inout [UInt8], attribute: Bool) {
        if !bytes.contains(where: { $0 == 38 || $0 == 60 || $0 == 62 || ($0 == 34 && attribute) }) {
            out.append(contentsOf: bytes)
            return
        }
        for byte in bytes {
            switch byte {
            case 38:
                out.append(contentsOf: [38, 97, 109, 112, 59])
            case 60:
                out.append(contentsOf: [38, 108, 116, 59])
            case 62:
                out.append(contentsOf: [38, 103, 116, 59])
            case 34 where attribute:
                out.append(contentsOf: [38, 113, 117, 111, 116, 59])
            default:
                out.append(byte)
            }
        }
    }
}

enum CSSTheme {
    static func resolve(_ css: String, theme: String, depth: Int = 0) -> String {
        guard depth < 8, css.range(of: "light-dark(", options: .caseInsensitive) != nil else {
            return css
        }
        let dark = theme == "dark"
        var out = ""
        var rest = Substring(css)
        while let range = rest.range(of: "light-dark(", options: .caseInsensitive) {
            out += rest[..<range.lowerBound]
            var depth = 1
            var index = range.upperBound
            while index < rest.endIndex, depth > 0 {
                let ch = rest[index]
                if ch == "(" {
                    depth += 1
                } else if ch == ")" {
                    depth -= 1
                }
                if depth > 0 {
                    index = rest.index(after: index)
                }
            }
            let inner = String(rest[range.upperBound..<index])
            let parts = WidgetNode.splitTopLevel(inner, separator: ",")
            let chosen = parts.count == 2 ? parts[dark ? 1 : 0] : (parts.first ?? "")
            out += resolve(chosen.trimmingCharacters(in: .whitespaces), theme: theme, depth: depth + 1)
            rest = index < rest.endIndex ? rest[rest.index(after: index)...] : ""
        }
        out += rest
        return out
    }
}
