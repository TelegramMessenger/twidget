import AppKit
import CoreVideo
import LiteHTML

public final class LiteHTMLWidgetRenderer {
    public let view: NSView
    public var onMessage: ((String) -> Void)? {
        didSet {
            flushBuffered()
        }
    }
    public var onDiag: ((String, String) -> Void)?
    public private(set) var layoutCount = 0
    public private(set) var layoutMilliseconds = 0.0

    private let documentView: WidgetDocumentView
    private let dom = WidgetDOM()
    private var theme: String
    private var document: LHDocument?
    private var buffered: [String] = []
    private var listened: Set<String> = []
    private var prevented: Set<String> = []
    private var lastSize = ""
    private var frozen = false
    private var destroyed = false
    private var needsRender = false
    private var frameRequested = false
    private var clock: WidgetFrameClock?
    private let timeOrigin = CACurrentMediaTime()
    private var pendingMoves: [String: [String: Any]] = [:]
    private var bounds: [Int: CGRect] = [:]
    private var boundsValid = false
    private var hoverId = 0
    private var pressId = 0
    private var lastPointer: CGPoint?
    private var clickCount = 0
    private var lastClick: (time: CFTimeInterval, point: CGPoint) = (0, .zero)
    private var observedResize: Set<Int> = []
    private let animator = WidgetAnimator()
    private var canvases: [Int: WidgetCanvas] = [:]
    private var canvasAttributeSizes: [Int: (Int, Int)] = [:]
    private var canvasURLs: [Int: String] = [:]
    private let uid = UUID().uuidString.lowercased()
    private var styleSheetKey = ""
    private var overrides: [Int: [(String, String)]] = [:]
    private var lastResize: [Int: CGSize] = [:]

    private static let alwaysSent: Set<String> = ["click", "input", "change", "submit", "reset", "load", "error", "toggle"]
    private static let coalesced: Set<String> = ["pointermove", "mousemove", "touchmove", "wheel", "scroll"]

    public init(theme: String, width: CGFloat) {
        self.theme = theme
        documentView = WidgetDocumentView(frame: CGRect(x: 0, y: 0, width: width, height: 300))
        view = documentView
        documentView.owner = self
        dom.onViolation = { [weak self] detail in
            self?.post(["type": "violation", "detail": detail])
        }
        dom.onCall = { [weak self] node, method, _ in
            self?.call(node, method)
        }
        dom.onCanvas = { [weak self] node, commands in
            self?.canvas(for: node).run(commands)
        }
        WidgetCanvasImages.install()
        animator.onEvent = { [weak self] type, id, _, _ in
            self?.dispatchAnimationEvent(type, id)
        }
    }

    public func load() {
        render()
        post(["type": "ready"])
    }

    public func setTheme(_ theme: String) {
        guard theme != self.theme else {
            return
        }
        self.theme = theme
        setNeedsRender()
        post(["type": "media"])
    }

    public func snapshot() -> NSImage? {
        guard let document = document else {
            return nil
        }
        let size = documentView.bounds.size
        let scale = documentView.window?.backingScaleFactor ?? 2
        guard let image = document.copyImage(maxWidth: size.width, rect: CGRect(origin: .zero, size: size), scale: scale) else {
            return nil
        }
        return NSImage(cgImage: image, size: size)
    }

    public func destroy() {
        destroyed = true
        for (id, url) in canvasURLs {
            WidgetCanvasImages.unregister(uid + "-" + String(id))
            LHImages.forgetImage(withURL: url)
        }
        canvasURLs.removeAll()
        clock?.stop()
        clock = nil
        documentView.owner = nil
        documentView.removeFromSuperview()
    }

    public func send(_ json: String) {
        guard !destroyed, let data = json.data(using: .utf8), let message = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any], let type = message["type"] as? String else {
            return
        }
        switch type {
        case "ops":
            for case let op as [String: Any] in (message["ops"] as? [Any]) ?? [] {
                dom.apply(op)
            }
            render()
        case "query":
            let qid = message["qid"] ?? NSNull()
            let q = (message["query"] as? [String: Any]) ?? [:]
            if let binary = binaryQuery(q) {
                post(["type": "result", "qid": qid, "result": binary.result, "binaryBase64": binary.data.base64EncodedString()])
            } else {
                post(["type": "result", "qid": qid, "result": query(q)])
            }
        case "listen":
            listened = Set((message["types"] as? [String]) ?? [])
        case "preventDefault":
            prevented = Set((message["keys"] as? [String]) ?? [])
        case "requestFrame":
            if frozen || frameRequested {
                return
            }
            frameRequested = true
            startClock()
        case "frozen":
            freeze(reason: (message["reason"] as? String) ?? "", fatal: (message["fatal"] as? Bool) ?? false)
        default:
            break
        }
    }

    private func post(_ message: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: message), let json = String(data: data, encoding: .utf8) else {
            return
        }
        if let onMessage = onMessage {
            onMessage(json)
        } else {
            buffered.append(json)
        }
    }

    private func flushBuffered() {
        guard let onMessage = onMessage, !buffered.isEmpty else {
            return
        }
        let queued = buffered
        buffered = []
        queued.forEach(onMessage)
    }

    private func setNeedsRender() {
        if needsRender {
            return
        }
        needsRender = true
        DispatchQueue.main.async { [weak self] in
            guard let self = self, self.needsRender else {
                return
            }
            self.render()
        }
    }

    private var width: CGFloat {
        return max(1, documentView.bounds.width)
    }

    private func render() {
        needsRender = false
        let now = CACurrentMediaTime()
        let sheets = dom.styleSheets()
        let key = sheets.joined(separator: "\u{1}")
        if key != styleSheetKey {
            styleSheetKey = key
            animator.setStyleSheets(sheets.map { CSSTheme.resolve($0, theme: theme) })
        }
        layout(overrides)
        var declarations: [Int: [String: String]] = [:]
        for (key, values) in document?.declarations(WidgetAnimator.watched, byAttribute: WidgetDOM.idAttribute, requiringAny: WidgetAnimator.entryProperties) ?? [:] {
            if let id = Int(key) {
                declarations[id] = values
            }
        }
        animator.update(declarations: declarations, now: now)
        let sampled = animator.sample(now: now)
        if !LiteHTMLWidgetRenderer.same(sampled, overrides) {
            overrides = sampled
            layout(overrides)
        }
        if animator.isActive && !frozen {
            startClock()
        }
    }

    private func renderAnimationFrame() {
        overrides = animator.sample(now: CACurrentMediaTime())
        layout(overrides)
    }

    private func canvas(for node: WidgetNode) -> WidgetCanvas {
        let id = dom.publicId(node)
        if let canvas = canvases[id] {
            return canvas
        }
        let size = canvasAttributeSize(node)
        let canvas = WidgetCanvas(width: size.0, height: size.1)
        canvasAttributeSizes[id] = size
        canvas.resolveImage = { [weak self] other in
            self?.image(forNode: other)
        }
        canvases[id] = canvas
        WidgetCanvasImages.register(uid + "-" + String(id), canvas)
        return canvas
    }

    private func canvasAttributeSize(_ node: WidgetNode) -> (Int, Int) {
        return (node.attribute("width").flatMap { Int($0.trimmingCharacters(in: .whitespaces)) } ?? 300, node.attribute("height").flatMap { Int($0.trimmingCharacters(in: .whitespaces)) } ?? 150)
    }

    private func image(forNode id: Int) -> CGImage? {
        if let canvas = canvases[id] {
            return canvas.image
        }
        guard let node = dom.node(id), node.tag == "img", let src = node.attribute("src"), src.hasPrefix("data:"), let comma = src.firstIndex(of: ",") else {
            return nil
        }
        let header = src[..<comma]
        let payload = String(src[src.index(after: comma)...])
        let data = header.contains(";base64") ? Data(base64Encoded: payload, options: .ignoreUnknownCharacters) : payload.removingPercentEncoding?.data(using: .utf8)
        guard let bytes = data, let source = CGImageSourceCreateWithData(bytes as CFData, nil) else {
            return nil
        }
        return CGImageSourceCreateImageAtIndex(source, 0, nil)
    }

    private func syncCanvases() {
        for (id, canvas) in canvases {
            guard let node = dom.node(id), node.tag == "canvas" else {
                canvases[id] = nil
                canvasAttributeSizes[id] = nil
                WidgetCanvasImages.unregister(uid + "-" + String(id))
                if let url = canvasURLs.removeValue(forKey: id) {
                    LHImages.forgetImage(withURL: url)
                }
                continue
            }
            let size = canvasAttributeSize(node)
            if let known = canvasAttributeSizes[id], known != size {
                canvasAttributeSizes[id] = size
                if size.0 != canvas.width || size.1 != canvas.height {
                    canvas.resize(size.0, size.1)
                }
            }
        }
    }

    private func canvasSource(_ node: WidgetNode) -> String? {
        let canvas = self.canvas(for: node)
        let id = dom.publicId(node)
        let url = "twcanvas:" + uid + "-" + String(id) + "-" + String(canvas.version)
        if let previous = canvasURLs[id], previous != url {
            retiredURLs.append(previous)
        }
        canvasURLs[id] = url
        return url
    }

    private var retiredURLs: [String] = []

    private func layout(_ overrides: [Int: [(String, String)]]) {
        let start = CACurrentMediaTime()
        defer {
            layoutCount += 1
            layoutMilliseconds += (CACurrentMediaTime() - start) * 1000
        }
        syncCanvases()
        let html = dom.serialize(theme: theme, overrides: overrides, canvasSource: { [unowned self] node in self.canvasSource(node) })
        let next = LHDocument(html: html, css: nil, previous: document)
        _ = next.layout(maxWidth: width, preferredWidth: width)
        document = next
        for url in retiredURLs {
            LHImages.forgetImage(withURL: url)
        }
        retiredURLs.removeAll()
        boundsValid = false
        documentView.document = next
        documentView.needsDisplay = true
        reportSize()
        reportResize()
    }

    private static func same(_ a: [Int: [(String, String)]], _ b: [Int: [(String, String)]]) -> Bool {
        guard a.count == b.count else {
            return false
        }
        for (key, list) in a {
            guard let other = b[key], other.count == list.count else {
                return false
            }
            for (x, y) in zip(list, other) where x.0 != y.0 || x.1 != y.1 {
                return false
            }
        }
        return true
    }

    private func dispatchAnimationEvent(_ type: String, _ id: Int) {
        if frozen || !listened.contains(type) {
            return
        }
        DispatchQueue.main.async { [weak self] in
            self?.post(["type": "event", "ev": ["type": type, "targetId": id]])
        }
    }

    private func reportSize() {
        guard let document = document else {
            return
        }
        let message: [String: Any] = [
            "type": "size",
            "width": Double(width),
            "height": Double(ceil(document.size.height)),
            "innerWidth": Double(width),
            "innerHeight": Double(documentView.bounds.height),
            "dpr": Double(documentView.window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2),
        ]
        let key = "\(message["height"]!)|\(message["width"]!)|\(message["innerHeight"]!)|\(message["dpr"]!)"
        if key != lastSize {
            lastSize = key
            post(message)
        }
    }

    func viewResized() {
        guard document != nil else {
            return
        }
        render()
    }

    private func elementBounds() -> [Int: CGRect] {
        if boundsValid {
            return bounds
        }
        var result: [Int: CGRect] = [:]
        for (key, value) in document?.bounds(byAttribute: WidgetDOM.idAttribute) ?? [:] {
            if let id = Int(key) {
                result[id] = value.rectValue
            }
        }
        bounds = result
        boundsValid = true
        return result
    }

    private func reportResize() {
        guard !observedResize.isEmpty else {
            return
        }
        let boxes = elementBounds()
        var entries: [[String: Any]] = []
        for id in observedResize {
            let size = boxes[id]?.size ?? .zero
            if lastResize[id] != size {
                lastResize[id] = size
                entries.append(["id": id, "width": Double(size.width), "height": Double(size.height)])
            }
        }
        if !entries.isEmpty {
            post(["type": "resize", "entries": entries])
        }
    }

    private func call(_ node: WidgetNode, _ method: String) {
        let id = dom.publicId(node)
        switch method {
        case "observeResize":
            observedResize.insert(id)
            lastResize[id] = nil
            DispatchQueue.main.async { [weak self] in
                self?.reportResize()
            }
        case "unobserveResize":
            observedResize.remove(id)
            lastResize[id] = nil
        default:
            break
        }
    }

    private static func rectResult(_ rect: CGRect) -> [String: Any] {
        return ["x": Double(rect.minX), "y": Double(rect.minY), "width": Double(rect.width), "height": Double(rect.height), "top": Double(rect.minY), "left": Double(rect.minX), "right": Double(rect.maxX), "bottom": Double(rect.maxY)]
    }

    private func viewport() -> [String: Any] {
        return ["innerWidth": Double(width), "innerHeight": Double(documentView.bounds.height), "dpr": Double(documentView.window?.backingScaleFactor ?? 2)]
    }

    private func query(_ q: [String: Any]) -> Any {
        let kind = (q["q"] as? String) ?? ""
        var rect: CGRect?
        if let id = (q["id"] as? NSNumber)?.intValue {
            guard let node = dom.node(id), !node.isText else {
                return ["error": "no such element"]
            }
            rect = elementBounds()[id] ?? .zero
        }
        switch kind {
        case "rect":
            return LiteHTMLWidgetRenderer.rectResult(rect ?? .zero)
        case "rects":
            return [LiteHTMLWidgetRenderer.rectResult(rect ?? .zero)]
        case "box":
            let r = rect ?? .zero
            return ["offsetWidth": Double(r.width), "offsetHeight": Double(r.height), "offsetTop": Double(r.minY), "offsetLeft": Double(r.minX), "offsetParent": 0, "clientWidth": Double(r.width), "clientHeight": Double(r.height), "clientTop": 0, "clientLeft": 0, "scrollWidth": Double(r.width), "scrollHeight": Double(r.height), "scrollTop": 0, "scrollLeft": 0]
        case "elementFromPoint":
            let x = (q["x"] as? NSNumber)?.doubleValue ?? 0
            let y = (q["y"] as? NSNumber)?.doubleValue ?? 0
            return target(at: CGPoint(x: x, y: y))
        case "matchMedia":
            return ["matches": matchMedia((q["query"] as? String) ?? "")]
        case "viewport":
            return viewport()
        case "measureText":
            return measureText((q["text"] as? String) ?? "", font: (q["font"] as? String) ?? "10px sans-serif")
        case "canvasCall":
            return canvasCall(q)
        case "cs":
            let id = (q["id"] as? NSNumber)?.intValue ?? 0
            let style = document?.computedStyle(attribute: WidgetDOM.idAttribute, value: String(id)) ?? [:]
            if let prop = q["prop"] as? String {
                return style[prop] ?? LiteHTMLWidgetRenderer.computedDefault(prop)
            }
            return style
        default:
            return ["error": "unsupported"]
        }
    }

    private func binaryQuery(_ q: [String: Any]) -> (result: [String: Any], data: Data)? {
        guard (q["q"] as? String) == "imageData", let id = (q["id"] as? NSNumber)?.intValue, let node = dom.node(id), node.tag == "canvas" else {
            return nil
        }
        let canvas = self.canvas(for: node)
        let w = WidgetCanvas.int(q["w"] ?? 0), h = WidgetCanvas.int(q["h"] ?? 0)
        return (["width": w, "height": h], canvas.pixels(x: WidgetCanvas.int(q["x"] ?? 0), y: WidgetCanvas.int(q["y"] ?? 0), w: w, h: h))
    }

    private func canvasCall(_ q: [String: Any]) -> Any {
        guard let id = (q["id"] as? NSNumber)?.intValue, let node = dom.node(id), node.tag == "canvas" else {
            return ["error": "no context"]
        }
        let canvas = self.canvas(for: node)
        let args = (q["args"] as? [Any]) ?? []
        switch (q["method"] as? String) ?? "" {
        case "isPointInPath", "isPointInStroke":
            return canvas.isPointInPath(args)
        case "getTransform":
            let t = canvas.currentTransform
            let matrix: [String: Double] = ["a": Double(t.a), "b": Double(t.b), "c": Double(t.c), "d": Double(t.d), "e": Double(t.tx), "f": Double(t.ty)]
            return matrix
        case "getLineDash":
            return canvas.lineDash()
        case "toDataURL":
            guard let image = canvas.image, let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
                return "data:,"
            }
            return "data:image/png;base64," + png.base64EncodedString()
        default:
            return ["error": "unsupported"]
        }
    }

    private static func computedDefault(_ property: String) -> String {
        switch property {
        case let p where p.hasPrefix("padding") || p.hasPrefix("margin") || (p.hasPrefix("border") && p.hasSuffix("width")):
            return "0px"
        case "max-width", "max-height":
            return "none"
        case "box-sizing":
            return "content-box"
        default:
            return ""
        }
    }

    private func matchMedia(_ query: String) -> Bool {
        let q = query.lowercased().replacingOccurrences(of: " ", with: "")
        if q.contains("prefers-color-scheme:dark") {
            return theme == "dark"
        }
        if q.contains("prefers-color-scheme:light") {
            return theme != "dark"
        }
        if q.contains("prefers-reduced-motion:reduce") {
            return NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        }
        if let range = q.range(of: "min-width:"), let value = Double(q[range.upperBound...].prefix { $0.isNumber || $0 == "." }) {
            return Double(width) >= value
        }
        if let range = q.range(of: "max-width:"), let value = Double(q[range.upperBound...].prefix { $0.isNumber || $0 == "." }) {
            return Double(width) <= value
        }
        return false
    }

    private func measureText(_ text: String, font: String) -> [String: Any] {
        let ctFont = CSSFont.make(font)
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): ctFont]))
        var ascent: CGFloat = 0, descent: CGFloat = 0, leading: CGFloat = 0
        let width = CTLineGetTypographicBounds(line, &ascent, &descent, &leading)
        let ink = CTLineGetBoundsWithOptions(line, .useGlyphPathBounds)
        return [
            "width": width,
            "actualBoundingBoxLeft": Double(-ink.minX),
            "actualBoundingBoxRight": Double(ink.maxX),
            "actualBoundingBoxAscent": Double(ink.maxY),
            "actualBoundingBoxDescent": Double(-ink.minY),
            "fontBoundingBoxAscent": Double(CTFontGetAscent(ctFont)),
            "fontBoundingBoxDescent": Double(CTFontGetDescent(ctFont)),
        ]
    }

    private func target(at point: CGPoint) -> Int {
        guard let value = document?.attribute(WidgetDOM.idAttribute, at: point), let id = Int(value) else {
            return dom.publicId(dom.html)
        }
        return id
    }

    private func freeze(reason: String, fatal: Bool) {
        if frozen {
            return
        }
        frozen = true
        clock?.stop()
        frameRequested = false
        pendingMoves.removeAll()
        documentView.badge = reason
        documentView.badgeFatal = fatal
        documentView.needsDisplay = true
    }

    private func startClock() {
        if clock == nil {
            clock = WidgetFrameClock { [weak self] in
                self?.tick()
            }
        }
        clock?.start()
    }

    private func tick() {
        if !pendingMoves.isEmpty {
            let moves = pendingMoves.values
            pendingMoves.removeAll()
            for event in moves {
                post(["type": "event", "ev": event])
            }
        }
        if animator.isActive && !frozen {
            renderAnimationFrame()
        }
        if frameRequested {
            frameRequested = false
            post(["type": "frame", "time": (CACurrentMediaTime() - timeOrigin) * 1000])
        }
        if !frameRequested && pendingMoves.isEmpty && !animator.isActive {
            clock?.stop()
        }
    }

    private func modifiers(_ flags: NSEvent.ModifierFlags) -> Int {
        return (flags.contains(.option) ? 1 : 0) | (flags.contains(.control) ? 2 : 0) | (flags.contains(.command) ? 4 : 0) | (flags.contains(.shift) ? 8 : 0)
    }

    private func mouseEvent(_ type: String, target: Int, point: CGPoint, event: NSEvent, buttons: Int) -> [String: Any] {
        let box = elementBounds()[target] ?? .zero
        var movement = CGPoint.zero
        if let last = lastPointer {
            movement = CGPoint(x: point.x - last.x, y: point.y - last.y)
        }
        return [
            "type": type,
            "targetId": target,
            "clientX": Double(point.x),
            "clientY": Double(point.y),
            "offsetX": Double(point.x - box.minX),
            "offsetY": Double(point.y - box.minY),
            "pageX": Double(point.x),
            "pageY": Double(point.y),
            "movementX": Double(movement.x),
            "movementY": Double(movement.y),
            "button": 0,
            "buttons": buttons,
            "detail": type.hasSuffix("click") ? clickCount : 0,
            "pointerId": 1,
            "pointerType": "mouse",
            "isPrimary": true,
            "pressure": buttons != 0 ? 0.5 : 0,
            "modifiers": modifiers(event.modifierFlags),
        ]
    }

    private func dispatch(_ type: String, target: Int, point: CGPoint, event: NSEvent, buttons: Int) {
        if frozen || target == 0 {
            return
        }
        if !LiteHTMLWidgetRenderer.alwaysSent.contains(type) && !listened.contains(type) {
            return
        }
        let payload = mouseEvent(type, target: target, point: point, event: event, buttons: buttons)
        if LiteHTMLWidgetRenderer.coalesced.contains(type) {
            pendingMoves[type + ":1"] = payload
            startClock()
            return
        }
        if !pendingMoves.isEmpty {
            let moves = pendingMoves.values
            pendingMoves.removeAll()
            for move in moves {
                post(["type": "event", "ev": move])
            }
        }
        post(["type": "event", "ev": payload])
    }

    func handleMouseDown(_ point: CGPoint, event: NSEvent) {
        let id = target(at: point)
        pressId = id
        let now = CACurrentMediaTime()
        if now - lastClick.time < NSEvent.doubleClickInterval, hypot(point.x - lastClick.point.x, point.y - lastClick.point.y) < 5 {
            clickCount += 1
        } else {
            clickCount = 1
        }
        lastClick = (now, point)
        if !frozen {
            post(["type": "gesture", "wants": prevented.contains("\(id):pointerdown") || prevented.contains("\(id):touchstart")])
        }
        if document?.mouseDown(point) == true {
            setNeedsRender()
        }
        dispatch("pointerdown", target: id, point: point, event: event, buttons: 1)
        dispatch("mousedown", target: id, point: point, event: event, buttons: 1)
        lastPointer = point
    }

    func handleMouseDragged(_ point: CGPoint, event: NSEvent) {
        dispatch("pointermove", target: pressId != 0 ? target(at: point) : 0, point: point, event: event, buttons: 1)
        dispatch("mousemove", target: target(at: point), point: point, event: event, buttons: 1)
        lastPointer = point
    }

    func handleMouseUp(_ point: CGPoint, event: NSEvent) {
        let id = target(at: point)
        if document?.mouseUp(point) == true {
            setNeedsRender()
        }
        dispatch("pointerup", target: id, point: point, event: event, buttons: 0)
        dispatch("mouseup", target: id, point: point, event: event, buttons: 0)
        if id == pressId || isAncestor(pressId, of: id) || isAncestor(id, of: pressId) {
            let clickTarget = isAncestor(id, of: pressId) ? id : (isAncestor(pressId, of: id) ? pressId : id)
            dispatch("click", target: clickTarget, point: point, event: event, buttons: 0)
            if clickCount == 2 {
                dispatch("dblclick", target: clickTarget, point: point, event: event, buttons: 0)
            }
        }
        pressId = 0
        lastPointer = point
    }

    func handleMouseMoved(_ point: CGPoint, event: NSEvent) {
        let id = target(at: point)
        if document?.mouseMoved(point) == true {
            setNeedsRender()
        }
        if id != hoverId {
            let previous = hoverId
            hoverId = id
            if previous != 0 {
                dispatch("pointerout", target: previous, point: point, event: event, buttons: 0)
                dispatch("mouseout", target: previous, point: point, event: event, buttons: 0)
                dispatch("pointerleave", target: previous, point: point, event: event, buttons: 0)
                dispatch("mouseleave", target: previous, point: point, event: event, buttons: 0)
            }
            dispatch("pointerover", target: id, point: point, event: event, buttons: 0)
            dispatch("mouseover", target: id, point: point, event: event, buttons: 0)
            dispatch("pointerenter", target: id, point: point, event: event, buttons: 0)
            dispatch("mouseenter", target: id, point: point, event: event, buttons: 0)
        }
        dispatch("pointermove", target: id, point: point, event: event, buttons: 0)
        dispatch("mousemove", target: id, point: point, event: event, buttons: 0)
        lastPointer = point
    }

    func handleMouseExited(_ event: NSEvent) {
        if document?.mouseExited() == true {
            setNeedsRender()
        }
        if hoverId != 0, let point = lastPointer {
            dispatch("pointerleave", target: hoverId, point: point, event: event, buttons: 0)
            dispatch("mouseleave", target: hoverId, point: point, event: event, buttons: 0)
        }
        hoverId = 0
    }

    func handleKey(_ type: String, event: NSEvent) {
        if frozen {
            return
        }
        let id = dom.publicId(dom.body)
        let key = WidgetKeys.key(event)
        if !LiteHTMLWidgetRenderer.alwaysSent.contains(type) && !listened.contains(type) {
            return
        }
        post(["type": "event", "ev": ["type": type, "targetId": id, "key": key, "code": WidgetKeys.code(event), "repeat": event.isARepeat, "modifiers": modifiers(event.modifierFlags)]])
    }

    private func isAncestor(_ ancestor: Int, of id: Int) -> Bool {
        guard ancestor != 0, var node = dom.node(id) else {
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

final class WidgetDocumentView: NSView {
    weak var owner: LiteHTMLWidgetRenderer?
    var document: LHDocument?
    var badge: String?
    var badgeFatal = false
    private var tracking: NSTrackingArea?

    override var isFlipped: Bool {
        return true
    }

    override var acceptsFirstResponder: Bool {
        return true
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
    }

    required init?(coder: NSCoder) {
        fatalError()
    }

    override func setFrameSize(_ newSize: NSSize) {
        let widthChanged = newSize.width != frame.width
        super.setFrameSize(newSize)
        if widthChanged {
            owner?.viewResized()
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else {
            return
        }
        document?.draw(in: ctx, origin: .zero, clip: dirtyRect)
        if let badge = badge, !badge.isEmpty {
            if badgeFatal {
                NSColor(white: 0.5, alpha: 0.25).setFill()
                bounds.fill()
            }
            let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.white]
            let text = NSAttributedString(string: badge, attributes: attributes)
            let size = text.size()
            let box = NSRect(x: bounds.maxX - size.width - 20, y: bounds.maxY - size.height - 10, width: size.width + 14, height: size.height + 4)
            NSColor(white: 0.24, alpha: 0.75).setFill()
            NSBezierPath(roundedRect: box, xRadius: 9, yRadius: 9).fill()
            text.draw(at: NSPoint(x: box.minX + 7, y: box.minY + 2))
        }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking = tracking {
            removeTrackingArea(tracking)
        }
        let area = NSTrackingArea(rect: bounds, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    private func point(_ event: NSEvent) -> CGPoint {
        return convert(event.locationInWindow, from: nil)
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        owner?.handleMouseDown(point(event), event: event)
    }

    override func mouseDragged(with event: NSEvent) {
        owner?.handleMouseDragged(point(event), event: event)
    }

    override func mouseUp(with event: NSEvent) {
        owner?.handleMouseUp(point(event), event: event)
    }

    override func mouseMoved(with event: NSEvent) {
        owner?.handleMouseMoved(point(event), event: event)
    }

    override func mouseExited(with event: NSEvent) {
        owner?.handleMouseExited(event)
    }

    override func keyDown(with event: NSEvent) {
        owner?.handleKey("keydown", event: event)
    }

    override func keyUp(with event: NSEvent) {
        owner?.handleKey("keyup", event: event)
    }
}

enum WidgetKeys {
    static func key(_ event: NSEvent) -> String {
        switch event.keyCode {
        case 36, 76: return "Enter"
        case 48: return "Tab"
        case 49: return " "
        case 51: return "Backspace"
        case 53: return "Escape"
        case 117: return "Delete"
        case 123: return "ArrowLeft"
        case 124: return "ArrowRight"
        case 125: return "ArrowDown"
        case 126: return "ArrowUp"
        case 115: return "Home"
        case 119: return "End"
        case 116: return "PageUp"
        case 121: return "PageDown"
        default: return event.characters ?? ""
        }
    }

    static func code(_ event: NSEvent) -> String {
        switch event.keyCode {
        case 36, 76: return "Enter"
        case 48: return "Tab"
        case 49: return "Space"
        case 51: return "Backspace"
        case 53: return "Escape"
        case 123: return "ArrowLeft"
        case 124: return "ArrowRight"
        case 125: return "ArrowDown"
        case 126: return "ArrowUp"
        default:
            let chars = (event.charactersIgnoringModifiers ?? "").uppercased()
            if let ch = chars.first, ch.isLetter {
                return "Key\(ch)"
            }
            if let ch = chars.first, ch.isNumber {
                return "Digit\(ch)"
            }
            return ""
        }
    }
}

enum CSSFont {
    static func make(_ css: String) -> CTFont {
        var size: CGFloat = 10
        var bold = false
        var italic = false
        var families: [String] = []
        let tokens = WidgetNode.splitTopLevel(css, separator: " ").filter { !$0.isEmpty }
        var familyStart = tokens.count
        for (index, token) in tokens.enumerated() {
            let t = token.lowercased()
            if t == "bold" || t == "bolder" || (Int(t) ?? 0) >= 600 {
                bold = true
            } else if t == "italic" || t == "oblique" {
                italic = true
            } else if let px = t.split(separator: "/").first, px.hasSuffix("px"), let value = Double(px.dropLast(2)) {
                size = CGFloat(value)
                familyStart = index + 1
                break
            }
        }
        if familyStart < tokens.count {
            families = tokens[familyStart...].joined(separator: " ").split(separator: ",").map { $0.trimmingCharacters(in: CharacterSet(charactersIn: " \"'")) }
        }
        var font: CTFont = CTFontCreateUIFontForLanguage(.system, size, nil) ?? CTFontCreateWithName("Helvetica" as CFString, size, nil)
        for family in families {
            let lower = family.lowercased()
            if ["sans-serif", "system-ui", "-apple-system", "blinkmacsystemfont"].contains(lower) {
                break
            }
            if lower == "monospace" {
                font = CTFontCreateWithName("Menlo" as CFString, size, nil)
                break
            }
            if lower == "serif" {
                font = CTFontCreateWithName("Times" as CFString, size, nil)
                break
            }
            let candidate = CTFontCreateWithName(family as CFString, size, nil)
            if (CTFontCopyFamilyName(candidate) as String).lowercased() == lower {
                font = candidate
                break
            }
        }
        var traits: CTFontSymbolicTraits = []
        if bold {
            traits.insert(.traitBold)
        }
        if italic {
            traits.insert(.traitItalic)
        }
        if !traits.isEmpty, let styled = CTFontCreateCopyWithSymbolicTraits(font, size, nil, traits, traits) {
            font = styled
        }
        return font
    }
}

final class WidgetFrameClock {
    private var link: CVDisplayLink?
    private let tick: () -> Void
    private var running = false
    private var scheduled = false
    private let lock = NSLock()

    init(tick: @escaping () -> Void) {
        self.tick = tick
        CVDisplayLinkCreateWithActiveCGDisplays(&link)
        if let link = link {
            CVDisplayLinkSetOutputHandler(link) { [weak self] _, _, _, _, _ in
                self?.fire()
                return kCVReturnSuccess
            }
        }
    }

    deinit {
        if let link = link {
            CVDisplayLinkStop(link)
        }
    }

    private func fire() {
        lock.lock()
        if scheduled {
            lock.unlock()
            return
        }
        scheduled = true
        lock.unlock()
        DispatchQueue.main.async { [weak self] in
            guard let self = self else {
                return
            }
            self.lock.lock()
            self.scheduled = false
            self.lock.unlock()
            if self.running {
                self.tick()
            }
        }
    }

    func start() {
        if running {
            return
        }
        running = true
        if let link = link {
            CVDisplayLinkStart(link)
        }
    }

    func stop() {
        if !running {
            return
        }
        running = false
        if let link = link {
            CVDisplayLinkStop(link)
        }
    }
}
