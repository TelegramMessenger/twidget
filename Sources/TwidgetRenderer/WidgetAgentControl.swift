import Cocoa

public struct WidgetAgentKey {
    let down: WidgetKeyEvent
    let up: WidgetKeyEvent

    private static let keyCodes: [String: UInt16] = [
        "ArrowLeft": 123, "ArrowRight": 124, "ArrowDown": 125, "ArrowUp": 126,
        "Enter": 36, " ": 49, "Space": 49, "Escape": 53, "Backspace": 51, "Tab": 48, "Delete": 117,
    ]

    public init?(key: String) {
        precondition(Thread.isMainThread)
        let code = WidgetAgentKey.keyCodes[key]
        let characters: String
        switch key {
        case "Enter":
            characters = "\r"
        case "Space":
            characters = " "
        case "Escape":
            characters = "\u{1b}"
        case "Backspace":
            characters = "\u{7f}"
        case "Tab":
            characters = "\t"
        default:
            characters = code == nil ? key : ""
        }
        guard code != nil || key.count == 1 else {
            return nil
        }
        func event(_ type: NSEvent.EventType) -> NSEvent? {
            return NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: 0, context: nil, characters: characters, charactersIgnoringModifiers: characters, isARepeat: false, keyCode: code ?? 0)
        }
        guard let down = event(.keyDown), let up = event(.keyUp) else {
            return nil
        }
        self.down = WidgetKeyEvent(down)
        self.up = WidgetKeyEvent(up)
    }
}

extension LiteHTMLWidgetRenderer {
    private static let agentInteractiveTags: Set<String> = ["a", "button", "input", "select", "textarea", "summary", "label", "canvas", "details", "option"]
    private static let agentInteractiveRoles: Set<String> = ["button", "link", "checkbox", "radio", "tab", "menuitem", "switch", "slider", "option", "gridcell"]
    private static let agentSkippedTags: Set<String> = ["script", "style", "head", "template", "noscript"]
    private static let agentBlockTags: Set<String> = ["div", "p", "li", "ul", "ol", "section", "article", "header", "footer", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "table", "br", "button", "label", "pre", "blockquote", "nav", "main", "aside", "details", "summary", "form", "fieldset"]

    public func agentState(maxText: Int = 6000, maxElements: Int = 120) -> [String: Any] {
        let bounds = elementBounds()
        var text = ""
        var elements: [[String: Any]] = []

        func ownText(_ node: WidgetNode, limit: Int) -> String {
            var parts = ""
            func walk(_ node: WidgetNode) {
                guard parts.count < limit, !LiteHTMLWidgetRenderer.agentSkippedTags.contains(node.tag.lowercased()) else {
                    return
                }
                if node.isText {
                    parts += node.text
                    return
                }
                node.children.forEach(walk)
            }
            walk(node)
            return String(parts.split(whereSeparator: \.isWhitespace).joined(separator: " ").prefix(limit))
        }

        func visit(_ node: WidgetNode) {
            let tag = node.tag.lowercased()
            if LiteHTMLWidgetRenderer.agentSkippedTags.contains(tag) {
                return
            }
            if node.isText {
                if text.count < maxText {
                    text += node.text
                }
                return
            }
            if LiteHTMLWidgetRenderer.agentBlockTags.contains(tag), text.count < maxText, !text.hasSuffix("\n") {
                text += "\n"
            }
            let id = dom.publicId(node)
            let role = node.attribute("role")?.lowercased()
            let interactive = LiteHTMLWidgetRenderer.agentInteractiveTags.contains(tag) || role.map { LiteHTMLWidgetRenderer.agentInteractiveRoles.contains($0) } == true || node.attribute("tabindex") != nil || node.attribute("onclick") != nil || node.attribute("data-action") != nil || node.attribute("id") != nil
            if interactive, elements.count < maxElements, let rect = bounds[id], rect.width > 0, rect.height > 0 {
                var element: [String: Any] = ["id": id, "tag": tag, "rect": [Int(rect.minX.rounded()), Int(rect.minY.rounded()), Int(rect.width.rounded()), Int(rect.height.rounded())]]
                let label = ownText(node, limit: 80)
                if !label.isEmpty {
                    element["text"] = label
                }
                for name in ["id", "class", "role", "aria-label", "title", "alt", "placeholder", "type", "value", "href", "data-action"] {
                    if let value = node.attribute(name), !value.isEmpty {
                        element[name == "id" ? "elementId" : name] = String(value.prefix(80))
                    }
                }
                if let value = node.formValue {
                    element["value"] = String(value.prefix(80))
                }
                if let checked = node.formChecked {
                    element["checked"] = checked
                }
                elements.append(element)
            }
            node.children.forEach(visit)
        }
        visit(dom.body)

        let lines = text.split(separator: "\n").map { $0.split(whereSeparator: \.isWhitespace).joined(separator: " ") }.filter { !$0.isEmpty }
        let content = bounds[dom.publicId(dom.body)] ?? bounds[dom.publicId(dom.html)] ?? .zero
        return [
            "text": String(lines.joined(separator: "\n").prefix(maxText)),
            "elements": elements,
            "width": Int(content.width.rounded()),
            "height": Int(content.height.rounded()),
            "listens": Array(listened).sorted(),
            "frozen": frozen,
        ]
    }

    public func agentPoint(element id: Int) -> CGPoint? {
        guard let rect = elementBounds()[id], rect.width > 0, rect.height > 0 else {
            return nil
        }
        return CGPoint(x: rect.midX, y: rect.midY)
    }

    public func agentClick(at point: CGPoint) -> Bool {
        guard !frozen, let down = NSEvent.mouseEvent(with: .leftMouseDown, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 1), let up = NSEvent.mouseEvent(with: .leftMouseUp, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 0) else {
            return false
        }
        handleMouseDown(point, event: down)
        handleMouseUp(point, event: up)
        return true
    }

    public func agentPress(_ key: WidgetAgentKey) -> Bool {
        guard !frozen else {
            return false
        }
        handleKey("keydown", event: key.down)
        handleKey("keyup", event: key.up)
        return true
    }
}
