import Foundation

final class WidgetNode {
    let id: Int
    let tag: String
    let svg: Bool
    let isText: Bool
    var text: String
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
        if name.lowercased() == "style" {
            declarations.removeAll()
            return
        }
        let key = name.lowercased()
        attributes.removeAll { $0.name.lowercased() == key }
    }

    func setProperty(_ name: String, _ value: String, important: Bool) {
        let key = name.hasPrefix("--") ? name : name.lowercased()
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

final class WidgetDOM {
    static let blockedElements: Set<String> = ["script", "iframe", "frame", "frameset", "object", "embed", "link", "meta", "base", "portal", "noscript", "set", "template", "audio", "video", "source", "track"]
    static let blockedAttributes: Set<String> = ["srcdoc", "srcset", "action", "formaction", "ping", "background", "nonce", "target", "http-equiv"]
    static let idAttribute = "data-lh-id"

    let html: WidgetNode
    let head: WidgetNode
    let body: WidgetNode
    private(set) var nodes: [Int: WidgetNode] = [:]
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
        nodes[id] = node
        if node.id < 0 {
            rootIds[ObjectIdentifier(node)] = id
        }
    }

    private func unregister(_ node: WidgetNode) {
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
        let key = name.lowercased()
        if key.hasPrefix("on") || WidgetDOM.blockedAttributes.contains(key) {
            violation("attribute \(name)")
            return
        }
        if key.hasPrefix("data-lh-") {
            return
        }
        node.setAttribute(name, value)
    }

    func apply(_ op: [String: Any]) {
        guard let kind = op["op"] as? String else {
            return
        }
        let id = (op["id"] as? NSNumber)?.intValue ?? 0
        switch kind {
        case "el":
            let tag = (op["tag"] as? String) ?? "span"
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
            register(id, WidgetNode(id: id, text: (op["data"] as? String) ?? ""))
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
            node.text = (op["data"] as? String) ?? ""
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
                let text = WidgetNode(id: Int.min, text: (op["text"] as? String) ?? "")
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

    func serialize(theme: String, overrides: [Int: [(String, String)]] = [:], canvasSource: (WidgetNode) -> String? = { _ in nil }) -> String {
        var out = "<!DOCTYPE html>"
        out.reserveCapacity(16 * 1024)
        write(html, into: &out, theme: theme, overrides: overrides, canvasSource: canvasSource)
        return out
    }

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

    private func write(_ node: WidgetNode, into out: inout String, theme: String, overrides: [Int: [(String, String)]], canvasSource: (WidgetNode) -> String?) {
        if node.isText {
            if let parent = node.parent, parent.tag == "style" {
                out += CSSTheme.resolve(node.text, theme: theme).replacingOccurrences(of: "</", with: "<\\/")
            } else {
                WidgetDOM.escape(node.text, into: &out, attribute: false)
            }
            return
        }
        let canvas = node.tag == "canvas" && !node.svg ? canvasSource(node) : nil
        out += "<"
        out += canvas != nil ? "img" : node.tag
        out += " data-lh-id=\""
        out += String(publicId(node))
        out += "\""
        if let source = canvas {
            out += " src=\""
            out += source
            out += "\""
        }
        for attribute in node.attributes where canvas == nil || attribute.name.lowercased() != "src" {
            out += " "
            out += attribute.name
            out += "=\""
            WidgetDOM.escape(attribute.value, into: &out, attribute: true)
            out += "\""
        }
        let animated = overrides[publicId(node)]
        if !node.declarations.isEmpty || animated != nil {
            out += " style=\""
            for declaration in node.declarations where declaration.name != "--lh-anim" {
                out += declaration.name
                out += ":"
                WidgetDOM.escape(CSSTheme.resolve(declaration.value, theme: theme), into: &out, attribute: true)
                if declaration.important {
                    out += " !important"
                }
                out += ";"
            }
            if let animated = animated {
                out += "--lh-anim:1;"
                for (name, value) in animated {
                    out += name
                    out += ":"
                    WidgetDOM.escape(CSSTheme.resolve(value, theme: theme), into: &out, attribute: true)
                    out += ";"
                }
            }
            out += "\""
        }
        out += ">"
        if canvas != nil || !node.svg && WidgetDOM.voidElements.contains(node.tag) {
            return
        }
        for child in node.children {
            write(child, into: &out, theme: theme, overrides: overrides, canvasSource: canvasSource)
        }
        out += "</"
        out += node.tag
        out += ">"
    }

    static func escape(_ text: String, into out: inout String, attribute: Bool) {
        for scalar in text.unicodeScalars {
            switch scalar {
            case "&":
                out += "&amp;"
            case "<":
                out += "&lt;"
            case ">":
                out += "&gt;"
            case "\"" where attribute:
                out += "&quot;"
            case "\u{a0}":
                out += "&nbsp;"
            default:
                out.unicodeScalars.append(scalar)
            }
        }
    }
}

enum CSSTheme {
    static func resolve(_ css: String, theme: String) -> String {
        guard css.range(of: "light-dark(", options: .caseInsensitive) != nil else {
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
            out += resolve(chosen.trimmingCharacters(in: .whitespaces), theme: theme)
            rest = index < rest.endIndex ? rest[rest.index(after: index)...] : ""
        }
        out += rest
        return out
    }
}
