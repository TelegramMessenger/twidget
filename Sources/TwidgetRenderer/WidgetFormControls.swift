import AppKit

public struct WidgetMenuItem: Equatable {
    public enum Kind: Equatable {
        case option
        case header
        case separator
    }

    public let kind: Kind
    public let title: String
    public let enabled: Bool
    public let selected: Bool

    public init(kind: Kind, title: String, enabled: Bool, selected: Bool) {
        self.kind = kind
        self.title = title
        self.enabled = enabled
        self.selected = selected
    }
}

public typealias WidgetMenuPresenter = (_ items: [WidgetMenuItem], _ anchor: CGRect, _ view: NSView, _ event: NSEvent?, _ completion: @escaping (Int?) -> Void) -> Void

public enum WidgetRendererEnvironment {
    public static var presentMenu: WidgetMenuPresenter?

    static func present(_ items: [WidgetMenuItem], anchor: CGRect, view: NSView, event: NSEvent?, completion: @escaping (Int?) -> Void) {
        if let presentMenu = presentMenu {
            presentMenu(items, anchor, view, event, completion)
        } else {
            WidgetNativeMenu.present(items, anchor: anchor, view: view, completion: completion)
        }
    }
}

final class WidgetNativeMenu: NSObject {
    private var chosen: Int?

    static func present(_ items: [WidgetMenuItem], anchor: CGRect, view: NSView, completion: @escaping (Int?) -> Void) {
        let target = WidgetNativeMenu()
        let menu = NSMenu()
        menu.autoenablesItems = false
        var selected: NSMenuItem?
        for (index, item) in items.enumerated() {
            switch item.kind {
            case .separator:
                menu.addItem(.separator())
            case .header:
                let header = NSMenuItem(title: item.title, action: nil, keyEquivalent: "")
                header.isEnabled = false
                menu.addItem(header)
            case .option:
                let entry = NSMenuItem(title: item.title, action: #selector(WidgetNativeMenu.choose(_:)), keyEquivalent: "")
                entry.target = target
                entry.tag = index
                entry.isEnabled = item.enabled
                entry.state = item.selected ? .on : .off
                menu.addItem(entry)
                if item.selected {
                    selected = entry
                }
            }
        }
        menu.minimumWidth = anchor.width
        let location = view.isFlipped ? CGPoint(x: anchor.minX, y: anchor.minY) : CGPoint(x: anchor.minX, y: anchor.maxY)
        menu.popUp(positioning: selected, at: location, in: view)
        completion(target.chosen)
    }

    @objc private func choose(_ sender: NSMenuItem) {
        chosen = sender.tag
    }
}

struct WidgetFormState {
    var focusedId = 0
    var generation = 0
    var initialValue = ""
    var lastActivation: CFTimeInterval = -.infinity
    var menuOpen = false
}

struct WidgetEditorSpec {
    let generation: Int
    let multiline: Bool
    let frame: CGRect
    let font: NSFont
    let color: NSColor
    let alignment: NSTextAlignment
    let text: String
    let placeholder: String
    let maxLength: Int
    let readOnly: Bool
    let numeric: Bool
}

extension LiteHTMLWidgetRenderer {
    static let editableInputTypes: Set<String> = ["", "text", "search", "number", "email", "url", "tel"]
    static let scriptFocusWindow: CFTimeInterval = 1

    private func formControl(from id: Int) -> WidgetNode? {
        var walk = dom.node(id)
        var depth = 0
        while let node = walk, depth < 6 {
            if !node.isText, !node.svg, ["select", "input", "textarea"].contains(node.tag.lowercased()) {
                return node
            }
            walk = node.parent
            depth += 1
        }
        return nil
    }

    private func isEditable(_ node: WidgetNode) -> Bool {
        guard !node.isText, !node.svg, node.attribute("disabled") == nil else {
            return false
        }
        switch node.tag.lowercased() {
        case "textarea":
            return true
        case "input":
            return LiteHTMLWidgetRenderer.editableInputTypes.contains((node.attribute("type") ?? "").lowercased().trimmingCharacters(in: .whitespaces))
        default:
            return false
        }
    }

    func formMouseDown(_ id: Int, event: NSEvent) {
        form.lastActivation = CACurrentMediaTime()
        guard !frozen, !destroyed, let node = formControl(from: id) else {
            return
        }
        let control = dom.publicId(node)
        if prevented.contains("\(control):mousedown") || prevented.contains("\(control):pointerdown") {
            return
        }
        if node.tag.lowercased() == "select" {
            openSelect(node, event: event)
        } else if isEditable(node) {
            focusControl(node)
        }
    }

    private func postForm(_ type: String, _ id: Int, _ extra: [String: Any] = [:]) {
        guard !frozen, !destroyed else {
            return
        }
        guard ["input", "change", "submit", "focus", "blur"].contains(type) || listened.contains(type) else {
            return
        }
        var ev: [String: Any] = ["type": type, "targetId": id]
        for (key, value) in extra {
            ev[key] = value
        }
        post(["type": "event", "ev": ev])
    }

    func menuItems(for select: WidgetNode) -> (items: [WidgetMenuItem], options: [Int]) {
        let current = WidgetDOM.selectedIndex(of: select)
        var items: [WidgetMenuItem] = []
        var options: [Int] = []
        var index = 0
        func addOption(_ option: WidgetNode, groupDisabled: Bool) {
            let title = option.attribute("label") ?? WidgetDOM.textContent(option)
            items.append(WidgetMenuItem(kind: .option, title: title.isEmpty ? " " : title, enabled: !groupDisabled && option.attribute("disabled") == nil, selected: index == current))
            options.append(index)
            index += 1
        }
        for child in select.children where !child.isText {
            switch child.tag.lowercased() {
            case "option":
                addOption(child, groupDisabled: false)
            case "optgroup":
                if !items.isEmpty {
                    items.append(WidgetMenuItem(kind: .separator, title: "", enabled: false, selected: false))
                    options.append(-1)
                }
                items.append(WidgetMenuItem(kind: .header, title: child.attribute("label") ?? "", enabled: false, selected: false))
                options.append(-1)
                let disabled = child.attribute("disabled") != nil
                for option in child.children where !option.isText && option.tag.lowercased() == "option" {
                    addOption(option, groupDisabled: disabled)
                }
            default:
                break
            }
        }
        return (items, options)
    }

    private func openSelect(_ node: WidgetNode, event: NSEvent?) {
        guard node.attribute("disabled") == nil, !form.menuOpen else {
            return
        }
        endEditing()
        let id = dom.publicId(node)
        let (items, options) = menuItems(for: node)
        guard options.contains(where: { $0 >= 0 }), let anchor = elementBounds()[id] else {
            return
        }
        form.menuOpen = true
        let view = documentView
        DispatchQueue.main.async { [weak view] in
            guard let view = view, view.window != nil else {
                view?.toOwner { $0.form.menuOpen = false }
                return
            }
            WidgetRendererEnvironment.present(items, anchor: anchor, view: view, event: event) { [weak view] choice in
                let option = choice.flatMap { $0 >= 0 && $0 < options.count ? options[$0] : nil }
                view?.toOwner { owner in
                    owner.form.menuOpen = false
                    if let option = option, option >= 0 {
                        owner.chooseOption(selectId: id, index: option)
                    }
                }
            }
        }
    }

    func chooseOption(selectId id: Int, index: Int) {
        guard !frozen, !destroyed, let node = dom.node(id), node.tag.lowercased() == "select", node.attribute("disabled") == nil else {
            return
        }
        let options = WidgetDOM.options(of: node)
        guard index >= 0, index < options.count, options[index].attribute("disabled") == nil, options[index].parent.map({ $0.tag.lowercased() != "optgroup" || $0.attribute("disabled") == nil }) ?? true else {
            return
        }
        guard WidgetDOM.selectedIndex(of: node) != index else {
            return
        }
        dom.setFormProperty(node, "selectedIndex", NSNumber(value: index))
        setNeedsRender()
        let value = options[index].attribute("value") ?? WidgetDOM.textContent(options[index])
        postForm("input", id, ["selectedIndex": index, "value": value])
        postForm("change", id, ["selectedIndex": index, "value": value])
    }

    private func currentValue(_ node: WidgetNode) -> String {
        if let value = node.formValue {
            return value
        }
        if node.tag.lowercased() == "textarea" {
            return node.children.filter { $0.isText }.map { $0.text }.joined()
        }
        return node.attribute("value") ?? ""
    }

    func focusControl(_ node: WidgetNode) {
        guard isEditable(node), !frozen, !destroyed, isOnScreen else {
            return
        }
        let id = dom.publicId(node)
        if form.focusedId == id {
            return
        }
        endEditing()
        form.generation += 1
        form.focusedId = id
        form.initialValue = currentValue(node)
        dom.setEditing(node, true)
        setNeedsRender()
        postForm("focus", id)
        postForm("focusin", id)
        guard let spec = editorSpec(node, id: id) else {
            return
        }
        let view = documentView
        DispatchQueue.main.async { [weak view] in
            view?.beginEditing(spec)
        }
    }

    func scriptFocus(_ id: Int) {
        guard let node = dom.node(id), CACurrentMediaTime() - form.lastActivation < LiteHTMLWidgetRenderer.scriptFocusWindow else {
            return
        }
        focusControl(node)
    }

    func endEditing() {
        guard form.focusedId != 0 else {
            return
        }
        let id = form.focusedId
        form.focusedId = 0
        form.generation += 1
        if let node = dom.node(id) {
            dom.setEditing(node, false)
            if !destroyed {
                setNeedsRender()
            }
            let value = currentValue(node)
            if value != form.initialValue {
                postForm("change", id, ["value": value])
            }
            postForm("blur", id)
            postForm("focusout", id)
        }
        let view = documentView
        if Thread.isMainThread {
            view.endEditing()
        } else {
            DispatchQueue.main.async { [weak view] in
                view?.endEditing()
            }
        }
    }

    func editorChanged(generation: Int, text: String) {
        guard generation == form.generation, form.focusedId != 0, let node = dom.node(form.focusedId) else {
            return
        }
        form.lastActivation = CACurrentMediaTime()
        let previous = currentValue(node)
        guard previous != text else {
            return
        }
        node.formValue = WidgetDOM.clip(text)
        let inserted = text.count > previous.count && text.hasPrefix(previous) ? String(text.dropFirst(previous.count)) : nil
        var extra: [String: Any] = ["value": node.formValue ?? "", "inputType": text.count < previous.count ? "deleteContentBackward" : "insertText"]
        extra["data"] = inserted ?? NSNull()
        postForm("input", form.focusedId, extra)
    }

    func editorCommand(generation: Int, command: String) {
        guard generation == form.generation, form.focusedId != 0, let node = dom.node(form.focusedId) else {
            return
        }
        let id = form.focusedId
        form.lastActivation = CACurrentMediaTime()
        switch command {
        case "Enter":
            for type in ["keydown", "keypress", "keyup"] {
                postForm(type, id, ["key": "Enter", "code": "Enter", "repeat": false, "modifiers": 0])
            }
            guard !prevented.contains("\(id):keydown:Enter"), !prevented.contains("\(id):keypress:Enter") else {
                return
            }
            var walk = node.parent
            while let parent = walk {
                if parent.tag.lowercased() == "form" {
                    postForm("submit", dom.publicId(parent))
                    return
                }
                walk = parent.parent
            }
        case "Escape":
            postForm("keydown", id, ["key": "Escape", "code": "Escape", "repeat": false, "modifiers": 0])
            endEditing()
        case "Tab", "ShiftTab":
            var editable: [WidgetNode] = []
            func collect(_ parent: WidgetNode) {
                for child in parent.children where !child.isText {
                    if isEditable(child) {
                        editable.append(child)
                    }
                    collect(child)
                }
            }
            collect(dom.body)
            guard let index = editable.firstIndex(where: { $0 === node }), editable.count > 1 else {
                endEditing()
                return
            }
            let next = editable[(index + (command == "Tab" ? 1 : editable.count - 1)) % editable.count]
            focusControl(next)
        default:
            break
        }
    }

    func editorEnded(generation: Int) {
        guard generation == form.generation else {
            return
        }
        endEditing()
    }

    func formValueSet(_ node: WidgetNode) {
        guard form.focusedId != 0, dom.publicId(node) == form.focusedId else {
            return
        }
        let text = currentValue(node)
        let generation = form.generation
        let view = documentView
        DispatchQueue.main.async { [weak view] in
            view?.setEditorText(text, generation: generation)
        }
    }

    func syncEditor() {
        guard form.focusedId != 0 else {
            return
        }
        guard let node = dom.node(form.focusedId), dom.isAttached(node), isEditable(node), isOnScreen else {
            endEditing()
            return
        }
        guard let spec = editorSpec(node, id: form.focusedId) else {
            return
        }
        let view = documentView
        DispatchQueue.main.async { [weak view] in
            view?.placeEditor(spec)
        }
    }

    private func editorSpec(_ node: WidgetNode, id: Int) -> WidgetEditorSpec? {
        guard let document = document, let box = elementBounds()[id], box.width > 0, box.height > 0 else {
            return nil
        }
        let style = document.computedStyle(attribute: WidgetDOM.idAttribute, value: String(id))
        func px(_ name: String) -> CGFloat {
            guard let value = style[name], let number = Double(value.replacingOccurrences(of: "px", with: "")), number.isFinite else {
                return 0
            }
            return CGFloat(number)
        }
        let size = max(1, px("font-size"))
        let family = style["font-family"].flatMap { $0.isEmpty ? nil : $0 } ?? "-apple-system"
        let css = "\(style["font-style"] == "italic" ? "italic " : "")\(style["font-weight"] ?? "400") \(size)px \(family)"
        let font = CSSFont.make(css) as NSFont
        let multiline = node.tag.lowercased() == "textarea"
        let type = (node.attribute("type") ?? "").lowercased()
        var content = CGRect(x: box.minX + px("padding-left") + px("border-left-width"), y: box.minY + px("padding-top") + px("border-top-width"), width: box.width - px("padding-left") - px("padding-right") - px("border-left-width") - px("border-right-width"), height: box.height - px("padding-top") - px("padding-bottom") - px("border-top-width") - px("border-bottom-width"))
        if type == "number" {
            content.size.width -= 10
        }
        if !multiline {
            let line = ceil(font.ascender - font.descender + font.leading)
            content.origin.y += (content.height - line) / 2
            content.size.height = line
        }
        let alignment: NSTextAlignment
        switch style["text-align"] ?? "" {
        case "center":
            alignment = .center
        case "right", "end":
            alignment = .right
        default:
            alignment = .left
        }
        return WidgetEditorSpec(generation: form.generation, multiline: multiline, frame: content.integral.width > 0 ? content : box, font: font, color: WidgetCSSColor.parse(style["color"] ?? "") ?? .textColor, alignment: alignment, text: currentValue(node), placeholder: node.attribute("placeholder") ?? "", maxLength: Int(node.attribute("maxlength") ?? "") ?? 0, readOnly: node.attribute("readonly") != nil, numeric: type == "number" || ["numeric", "decimal"].contains((node.attribute("inputmode") ?? "").lowercased()))
    }
}

enum WidgetCSSColor {
    static func parse(_ text: String) -> NSColor? {
        guard let open = text.firstIndex(of: "("), let close = text.lastIndex(of: ")") else {
            return nil
        }
        let parts = text[text.index(after: open)..<close].split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
        guard parts.count >= 3 else {
            return nil
        }
        return NSColor(srgbRed: CGFloat(parts[0]) / 255, green: CGFloat(parts[1]) / 255, blue: CGFloat(parts[2]) / 255, alpha: parts.count > 3 ? CGFloat(parts[3]) : 1)
    }
}

final class WidgetTextEditor: NSObject, NSTextFieldDelegate, NSTextViewDelegate {
    let generation: Int
    let view: NSView
    let field: NSTextField?
    let textView: NSTextView?
    private let maxLength: Int
    private let numeric: Bool
    private var lastText: String
    private var removing = false
    var onChange: ((String) -> Void)?
    var onCommand: ((String) -> Void)?
    var onEnd: (() -> Void)?

    init(_ spec: WidgetEditorSpec) {
        generation = spec.generation
        maxLength = spec.maxLength
        numeric = spec.numeric
        lastText = spec.text
        if spec.multiline {
            let scroll = NSScrollView(frame: spec.frame)
            scroll.drawsBackground = false
            scroll.hasVerticalScroller = false
            scroll.hasHorizontalScroller = false
            scroll.borderType = .noBorder
            let text = NSTextView(frame: CGRect(origin: .zero, size: spec.frame.size))
            text.drawsBackground = false
            text.isRichText = false
            text.allowsUndo = true
            text.font = spec.font
            text.textColor = spec.color
            text.insertionPointColor = spec.color
            text.alignment = spec.alignment
            text.textContainerInset = .zero
            text.textContainer?.lineFragmentPadding = 0
            text.textContainer?.widthTracksTextView = true
            text.isVerticallyResizable = true
            text.autoresizingMask = [.width]
            text.isEditable = !spec.readOnly
            text.string = spec.text
            text.isAutomaticQuoteSubstitutionEnabled = false
            text.isAutomaticDashSubstitutionEnabled = false
            scroll.documentView = text
            textView = text
            field = nil
            view = scroll
        } else {
            let input = NSTextField(frame: spec.frame)
            input.isBordered = false
            input.isBezeled = false
            input.drawsBackground = false
            input.focusRingType = .none
            input.font = spec.font
            input.textColor = spec.color
            input.alignment = spec.alignment
            input.isEditable = !spec.readOnly
            input.isSelectable = true
            input.usesSingleLineMode = true
            input.cell?.wraps = false
            input.cell?.isScrollable = true
            input.stringValue = spec.text
            if !spec.placeholder.isEmpty {
                input.placeholderAttributedString = NSAttributedString(string: spec.placeholder, attributes: [.font: spec.font, .foregroundColor: NSColor(srgbRed: 0xa9 / 255.0, green: 0xa9 / 255.0, blue: 0xa9 / 255.0, alpha: 1)])
            }
            field = input
            textView = nil
            view = input
        }
        super.init()
        field?.delegate = self
        textView?.delegate = self
        place(spec.frame)
    }

    var responder: NSResponder? {
        return textView ?? field
    }

    func place(_ frame: CGRect) {
        if field != nil {
            view.frame = frame.insetBy(dx: -2, dy: 0)
        } else {
            view.frame = frame
        }
    }

    func setText(_ text: String) {
        guard text != current else {
            return
        }
        lastText = text
        if let field = field {
            field.stringValue = text
        } else {
            textView?.string = text
        }
    }

    func remove() {
        removing = true
        if let window = view.window, let responder = window.firstResponder as? NSView, responder === textView || responder.isDescendant(of: view) || (field?.currentEditor() != nil && responder === field?.currentEditor()) {
            window.makeFirstResponder(view.superview)
        }
        view.removeFromSuperview()
    }

    private var current: String {
        return field?.stringValue ?? textView?.string ?? ""
    }

    private var composing: Bool {
        if let textView = textView {
            return textView.hasMarkedText()
        }
        return (field?.currentEditor() as? NSTextView)?.hasMarkedText() ?? false
    }

    private func accepts(_ text: String) -> Bool {
        if maxLength > 0, text.utf16.count > maxLength {
            return false
        }
        if numeric, !text.allSatisfy({ "0123456789+-.,eE".contains($0) }) {
            return false
        }
        return true
    }

    private func changed() {
        guard !removing, !composing else {
            return
        }
        let text = current
        guard accepts(text) else {
            setText(lastText)
            NSSound.beep()
            return
        }
        guard text != lastText else {
            return
        }
        lastText = text
        onChange?(text)
    }

    func controlTextDidChange(_ obj: Notification) {
        changed()
    }

    func textDidChange(_ notification: Notification) {
        changed()
    }

    func controlTextDidEndEditing(_ obj: Notification) {
        if !removing {
            onEnd?()
        }
    }

    func textDidEndEditing(_ notification: Notification) {
        if !removing {
            onEnd?()
        }
    }

    private func command(_ selector: Selector, multiline: Bool) -> Bool {
        switch selector {
        case #selector(NSResponder.insertNewline(_:)) where !multiline:
            onCommand?("Enter")
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            onCommand?("Escape")
            return true
        case #selector(NSResponder.insertTab(_:)):
            onCommand?("Tab")
            return true
        case #selector(NSResponder.insertBacktab(_:)):
            onCommand?("ShiftTab")
            return true
        default:
            return false
        }
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        return command(commandSelector, multiline: false)
    }

    func textView(_ textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        return command(commandSelector, multiline: true)
    }
}

extension WidgetDocumentView {
    func beginEditing(_ spec: WidgetEditorSpec) {
        endEditing()
        guard owner != nil, window != nil else {
            toOwner { $0.editorEnded(generation: spec.generation) }
            return
        }
        let editor = WidgetTextEditor(spec)
        let generation = spec.generation
        editor.onChange = { [weak self] text in
            self?.toOwner { $0.editorChanged(generation: generation, text: text) }
        }
        editor.onCommand = { [weak self] command in
            self?.toOwner { $0.editorCommand(generation: generation, command: command) }
        }
        editor.onEnd = { [weak self] in
            DispatchQueue.main.async {
                self?.toOwner { $0.editorEnded(generation: generation) }
            }
        }
        formEditor = editor
        addSubview(editor.view)
        if let responder = editor.responder {
            window?.makeFirstResponder(responder)
        }
    }

    func endEditing() {
        formEditor?.remove()
        formEditor = nil
    }

    func placeEditor(_ spec: WidgetEditorSpec) {
        guard let editor = formEditor, editor.generation == spec.generation else {
            return
        }
        editor.place(spec.frame)
    }

    func setEditorText(_ text: String, generation: Int) {
        guard let editor = formEditor, editor.generation == generation else {
            return
        }
        editor.setText(text)
    }
}
