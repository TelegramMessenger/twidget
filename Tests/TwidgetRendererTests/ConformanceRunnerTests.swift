import XCTest
import AppKit
import WidgetSandbox
import WidgetSandboxProtocol
@testable import TwidgetRenderer

final class ConformanceProbe: Renderer, ScheduledRenderer {
    let inner: LiteHTMLWidgetRenderer
    private let lock = NSLock()
    private var htmlId: Int?
    private var latest: String?

    init(_ inner: LiteHTMLWidgetRenderer) {
        self.inner = inner
    }

    var published: [String: Any]? {
        lock.lock()
        let value = latest
        lock.unlock()
        guard let value = value, let object = try? JSONSerialization.jsonObject(with: Data(value.utf8)) as? [String: Any] else {
            return nil
        }
        return object
    }

    private func observe(element id: Int, adopt: String?, attributes: [(name: String, value: String)]) {
        guard adopt == "html" else {
            return
        }
        lock.lock()
        htmlId = id
        for attribute in attributes where attribute.name == "data-conformance" {
            latest = attribute.value
        }
        lock.unlock()
    }

    private func observe(attribute id: Int, name: String, value: String) {
        guard name == "data-conformance" else {
            return
        }
        lock.lock()
        if id == htmlId {
            latest = value
        }
        lock.unlock()
    }

    private func observe(json: String) {
        guard json.hasPrefix("{\"type\":\"ops\""), let message = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any], let ops = message["ops"] as? [[String: Any]] else {
            return
        }
        for op in ops {
            let id = (op["id"] as? NSNumber)?.intValue ?? -1
            switch op["op"] as? String {
            case "el":
                let attributes = ((op["attrs"] as? [[Any]]) ?? []).compactMap { pair -> (name: String, value: String)? in
                    guard pair.count > 1, let name = pair[0] as? String, let value = pair[1] as? String else { return nil }
                    return (name, value)
                }
                observe(element: id, adopt: op["adopt"] as? String, attributes: attributes)
            case "attr":
                if let name = op["name"] as? String, let value = op["value"] as? String {
                    observe(attribute: id, name: name, value: value)
                }
            default:
                break
            }
        }
    }

    private func observe(ops: [WidgetOp]) {
        for op in ops {
            switch op.kind {
            case .element:
                observe(element: op.id, adopt: op.adopt, attributes: op.attributes)
            case .attribute:
                observe(attribute: op.id, name: op.name, value: op.value)
            default:
                break
            }
        }
    }

    var view: PlatformView { inner.view }

    var onMessage: ((String) -> Void)? {
        get { inner.onMessage }
        set { inner.onMessage = newValue }
    }

    var onDiag: ((String, String) -> Void)? {
        get { inner.onDiag }
        set { inner.onDiag = newValue }
    }

    var schedule: (@escaping () -> Void) -> Void {
        get { inner.schedule }
        set { inner.schedule = newValue }
    }

    var scheduleAfter: (TimeInterval, @escaping () -> Void) -> Void {
        get { inner.scheduleAfter }
        set { inner.scheduleAfter = newValue }
    }

    var isOnExecutor: () -> Bool {
        get { inner.isOnExecutor }
        set { inner.isOnExecutor = newValue }
    }

    var scheduleUrgent: (@escaping () -> Void) -> Void {
        get { inner.scheduleUrgent }
        set { inner.scheduleUrgent = newValue }
    }

    var executorIsMain: () -> Bool {
        get { inner.executorIsMain }
        set { inner.executorIsMain = newValue }
    }

    func load() {
        inner.load()
    }

    func send(_ json: String) {
        observe(json: json)
        inner.send(json)
    }

    var acceptsTypedOps: Bool { inner.acceptsTypedOps }

    func send(ops: [WidgetOp]) {
        observe(ops: ops)
        inner.send(ops: ops)
    }

    func setTheme(_ theme: String) {
        inner.setTheme(theme)
    }

    func evaluate(_ script: String, completion: @escaping (String?) -> Void) {
        completion(nil)
    }

    func snapshot(completion: @escaping (PlatformImage?) -> Void) {
        completion(inner.snapshot())
    }

    func destroy() {
        inner.destroy()
    }
}

final class ConformanceRunnerTests: XCTestCase {
    private static let width: CGFloat = 390
    private static let maximumHeight: CGFloat = 600
    private static let inputHeight: CGFloat = 24
    private static let actionGap: TimeInterval = 0.35

    private static let palettes: [String: [String: String]] = [
        "light": [
            "bg_color": "#ffffff", "text_color": "#000000", "hint_color": "#999999", "link_color": "#2481cc",
            "button_color": "#2481cc", "button_text_color": "#ffffff", "secondary_bg_color": "#efeff3",
            "accent_text_color": "#2481cc", "subtitle_text_color": "#999999", "destructive_text_color": "#ff3b30",
            "section_bg_color": "#ffffff", "section_header_text_color": "#6d6d72", "section_separator_color": "#c8c7cc",
            "header_bg_color": "#efeff3", "bottom_bar_bg_color": "#ffffff",
        ],
        "dark": [
            "bg_color": "#212121", "text_color": "#ffffff", "hint_color": "#aaaaaa", "link_color": "#8774e1",
            "button_color": "#8774e1", "button_text_color": "#ffffff", "secondary_bg_color": "#181818",
            "accent_text_color": "#8774e1", "subtitle_text_color": "#aaaaaa", "destructive_text_color": "#ff595a",
            "section_bg_color": "#212121", "section_header_text_color": "#8774e1", "section_separator_color": "#303030",
            "header_bg_color": "#212121", "bottom_bar_bg_color": "#212121",
        ],
    ]

    private static func theme(_ name: String) -> WidgetTheme {
        return WidgetTheme(colorScheme: name == "dark" ? .dark : .light, params: palettes[name] ?? [:])
    }

    private final class FlippedView: NSView {
        override var isFlipped: Bool { true }
    }

    private final class KeyWindow: NSWindow {
        override var canBecomeKey: Bool { true }
        override var canBecomeMain: Bool { true }
    }

    private func pump(_ seconds: TimeInterval, until done: () -> Bool = { false }) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done() && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
    }

    private final class Session {
        let window: NSWindow
        let field: NSTextField
        let host: WidgetHost
        let probe: ConformanceProbe
        let features: Set<String>
        private var pointer: CGPoint?

        init(html: String, theme: String, features: Set<String>) throws {
            _ = NSApplication.shared
            self.features = features
            let width = ConformanceRunnerTests.width
            let height = ConformanceRunnerTests.maximumHeight + ConformanceRunnerTests.inputHeight + 16
            window = KeyWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height), styleMask: [.borderless], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let content = FlippedView(frame: NSRect(x: 0, y: 0, width: width, height: height))
            window.contentView = content
            field = NSTextField(frame: NSRect(x: 0, y: height - ConformanceRunnerTests.inputHeight - 8, width: width, height: ConformanceRunnerTests.inputHeight))
            field.placeholderString = "chat input"
            content.addSubview(field)
            window.orderFrontRegardless()
            window.makeKey()
            window.makeFirstResponder(field)
            var options = WidgetHost.Options()
            options.policy = .strict
            options.features = features
            options.maximumHeight = ConformanceRunnerTests.maximumHeight
            var made: ConformanceProbe?
            host = try WidgetHost(html: html, theme: ConformanceRunnerTests.theme(theme), width: width, options: options, rendererFactory: { _, scheme, width in
                let probe = ConformanceProbe(LiteHTMLWidgetRenderer(theme: scheme, width: width))
                made = probe
                return probe
            })
            probe = try XCTUnwrap(made)
            host.view.frame = NSRect(x: 0, y: 0, width: width, height: ConformanceRunnerTests.maximumHeight)
            content.addSubview(host.view)
            host.onChange = { [weak self] state in
                self?.fit(state.height)
            }
        }

        private func fit(_ height: CGFloat) {
            let clamped = min(max(height, 50), ConformanceRunnerTests.maximumHeight)
            if abs(host.view.frame.height - clamped) > 0.5 {
                host.view.frame = NSRect(x: 0, y: 0, width: ConformanceRunnerTests.width, height: clamped)
            }
        }

        func close() {
            host.destroy()
            window.orderOut(nil)
            window.close()
        }

        var chatFocused: Bool {
            if window.firstResponder === field {
                return true
            }
            if let editor = window.firstResponder as? NSTextView, editor.isFieldEditor, editor.delegate === field {
                return true
            }
            return false
        }

        var focusHolder: String {
            guard let responder = window.firstResponder else {
                return "none"
            }
            if chatFocused {
                return "chat input"
            }
            return String(describing: type(of: responder))
        }

        var status: String? {
            return probe.published?["status"] as? String
        }

        func target(_ value: Any?) -> CGPoint? {
            if let pair = value as? [Any], pair.count >= 2, let x = (pair[0] as? NSNumber)?.doubleValue, let y = (pair[1] as? NSNumber)?.doubleValue {
                return CGPoint(x: x, y: y)
            }
            guard let name = value as? String, let targets = probe.published?["targets"] as? [String: Any], let box = targets[name] as? [NSNumber], box.count == 4 else {
                return nil
            }
            return CGPoint(x: box[0].doubleValue + box[2].doubleValue / 2, y: box[1].doubleValue + box[3].doubleValue / 2)
        }

        private func mouse(_ type: NSEvent.EventType, _ point: CGPoint, clicks: Int = 1) {
            let view = probe.view
            let location = view.convert(point, to: nil)
            let pressed = type == .leftMouseDown || type == .leftMouseDragged || type == .rightMouseDown
            guard let event = NSEvent.mouseEvent(with: type, location: location, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: clicks, pressure: pressed ? 1 : 0) else {
                return
            }
            switch type {
            case .leftMouseDown: view.mouseDown(with: event)
            case .leftMouseUp: view.mouseUp(with: event)
            case .leftMouseDragged: view.mouseDragged(with: event)
            case .rightMouseDown: view.rightMouseDown(with: event)
            case .rightMouseUp: view.rightMouseUp(with: event)
            default: view.mouseMoved(with: event)
            }
            pointer = point
        }

        func move(to point: CGPoint, steps: Int, pump: (TimeInterval) -> Void) {
            let from = pointer ?? CGPoint(x: point.x, y: max(0, point.y - 20))
            for step in 1...max(1, steps) {
                let t = CGFloat(step) / CGFloat(max(1, steps))
                mouse(.mouseMoved, CGPoint(x: from.x + (point.x - from.x) * t, y: from.y + (point.y - from.y) * t))
                pump(0.016)
            }
        }

        func tap(_ point: CGPoint, pump: (TimeInterval) -> Void) {
            move(to: point, steps: 1, pump: pump)
            mouse(.leftMouseDown, point)
            pump(0.05)
            mouse(.leftMouseUp, point)
        }

        func doubleTap(_ point: CGPoint, pump: (TimeInterval) -> Void) {
            move(to: point, steps: 1, pump: pump)
            mouse(.leftMouseDown, point, clicks: 1)
            pump(0.04)
            mouse(.leftMouseUp, point, clicks: 1)
            pump(0.08)
            mouse(.leftMouseDown, point, clicks: 2)
            pump(0.04)
            mouse(.leftMouseUp, point, clicks: 2)
        }

        func rightClick(_ point: CGPoint, pump: (TimeInterval) -> Void) {
            move(to: point, steps: 1, pump: pump)
            mouse(.rightMouseDown, point)
            pump(0.05)
            mouse(.rightMouseUp, point)
        }

        func press(_ point: CGPoint, ms: Double, pump: (TimeInterval) -> Void) {
            move(to: point, steps: 1, pump: pump)
            mouse(.leftMouseDown, point)
            pump(ms / 1000)
            mouse(.leftMouseUp, point)
        }

        func drag(_ from: CGPoint, to: CGPoint, steps: Int, pump: (TimeInterval) -> Void) {
            move(to: from, steps: 1, pump: pump)
            mouse(.leftMouseDown, from)
            pump(0.03)
            for step in 1...max(1, steps) {
                let t = CGFloat(step) / CGFloat(max(1, steps))
                mouse(.leftMouseDragged, CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t))
                pump(0.02)
            }
            mouse(.leftMouseUp, to)
        }

        func key(_ name: String, pump: (TimeInterval) -> Void) {
            let (characters, code): (String, UInt16) = {
                switch name {
                case "Enter": return ("\r", 36)
                case "Space", " ": return (" ", 49)
                case "Escape": return ("\u{1b}", 53)
                case "Tab": return ("\t", 48)
                default: return (name.lowercased(), 0)
                }
            }()
            for type in [NSEvent.EventType.keyDown, .keyUp] {
                guard let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, characters: characters, charactersIgnoringModifiers: characters, isARepeat: false, keyCode: code) else {
                    continue
                }
                window.sendEvent(event)
                pump(0.03)
            }
        }

        func wheel(_ point: CGPoint, dy: Double, pump: (TimeInterval) -> Void) {
            move(to: point, steps: 1, pump: pump)
            let view = probe.view
            let inWindow = view.convert(point, to: nil)
            let onScreen = window.convertPoint(toScreen: inWindow)
            let screenHeight = NSScreen.screens.first?.frame.height ?? 0
            let chunks = 4
            for _ in 0..<chunks {
                guard let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: Int32(-dy / Double(chunks)), wheel2: 0, wheel3: 0) else {
                    return
                }
                cg.location = CGPoint(x: onScreen.x, y: screenHeight - onScreen.y)
                guard let event = NSEvent(cgEvent: cg) else {
                    return
                }
                view.scrollWheel(with: event)
                pump(0.02)
            }
        }

        func setVisible(_ visible: Bool) {
            host.setVisible(visible)
            probe.inner.setVisible(visible)
        }

        func setTheme(_ name: String) {
            host.setTheme(ConformanceRunnerTests.theme(name))
        }

        func crop(_ rect: [Double], into url: URL) -> Bool {
            guard rect.count == 4, let image = probe.inner.snapshot(), let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
                return false
            }
            let scale = CGFloat(cg.width) / image.size.width
            let source = CGRect(x: rect[0] * Double(scale), y: rect[1] * Double(scale), width: rect[2] * Double(scale), height: rect[3] * Double(scale)).integral
            guard let part = cg.cropping(to: source) else {
                return false
            }
            let w = Int(rect[2] * 2), h = Int(rect[3] * 2)
            guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
                return false
            }
            ctx.interpolationQuality = .high
            ctx.draw(part, in: CGRect(x: 0, y: 0, width: w, height: h))
            guard let out = ctx.makeImage(), let png = NSBitmapImageRep(cgImage: out).representation(using: .png, properties: [:]) else {
                return false
            }
            return (try? png.write(to: url)) != nil
        }
    }

    private static func git(_ directory: String, _ arguments: [String]) -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["-C", directory] + arguments
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else {
            return ""
        }
        process.waitUntilExit()
        return String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static var machine: String {
        var size = 0
        sysctlbyname("hw.model", nil, &size, nil, 0)
        var model = [CChar](repeating: 0, count: max(size, 1))
        sysctlbyname("hw.model", &model, &size, nil, 0)
        return String(cString: model)
    }

    private func run(_ test: [String: Any], theme: String, root: URL, out: URL, features: Set<String>) throws -> [String: Any] {
        let id = test["id"] as? String ?? ""
        let file = test["file"] as? String ?? ""
        let actions = (test["actions"] as? [[String: Any]]) ?? []
        let html = try String(contentsOf: root.appendingPathComponent(file), encoding: .utf8)
        let session = try Session(html: html, theme: theme, features: features)
        defer { session.close() }
        let idle: (TimeInterval) -> Void = { [unowned self] seconds in self.pump(seconds) }
        let host = session.host
        pump(30, until: { host.diagnostics.loaded || host.diagnostics.dead != nil })
        let wanted: Set<String> = actions.isEmpty ? ["done"] : ["done", "awaiting"]
        pump(20, until: { wanted.contains(session.status ?? "") || host.diagnostics.dead != nil })
        let chatFocused = session.chatFocused
        let focusHolder = session.focusHolder
        var skipped: [Int] = []
        if !actions.isEmpty, session.probe.published != nil {
            for (index, action) in actions.enumerated() {
                if host.diagnostics.dead != nil {
                    break
                }
                if let value = action["tap"], let point = session.target(value) {
                    session.tap(point, pump: idle)
                } else if let value = action["dblclick"], let point = session.target(value) {
                    session.doubleTap(point, pump: idle)
                } else if let value = action["rightclick"], let point = session.target(value) {
                    session.rightClick(point, pump: idle)
                } else if let value = action["hover"], let point = session.target(value) {
                    session.move(to: point, steps: 4, pump: idle)
                } else if let value = action["press"], let point = session.target(value) {
                    session.press(point, ms: (action["ms"] as? NSNumber)?.doubleValue ?? 400, pump: idle)
                } else if let value = action["drag"], let from = session.target(value) {
                    let steps = (action["steps"] as? NSNumber)?.intValue ?? 10
                    var to: CGPoint?
                    if (action["by"] as? Bool) == true, let delta = session.target(action["to"]) {
                        to = CGPoint(x: from.x + delta.x, y: from.y + delta.y)
                    } else {
                        to = session.target(action["to"])
                    }
                    if let to = to {
                        session.drag(from, to: to, steps: steps, pump: idle)
                    } else {
                        skipped.append(index)
                    }
                } else if let key = action["key"] as? String {
                    session.key(key, pump: idle)
                } else if let value = action["wheel"], let point = session.target(value) {
                    session.wheel(point, dy: (action["dy"] as? NSNumber)?.doubleValue ?? 60, pump: idle)
                } else if let name = action["theme"] as? String {
                    session.setTheme(name)
                } else if let visible = action["visible"] as? Bool {
                    session.setVisible(visible)
                } else if let ms = action["wait"] as? NSNumber {
                    idle(ms.doubleValue / 1000)
                } else {
                    skipped.append(index)
                }
                idle(ConformanceRunnerTests.actionGap)
            }
            pump(10, until: { session.status == "done" || host.diagnostics.dead != nil })
        }
        pump(0.25)
        let published = session.probe.published
        let state = host.diagnostics
        let stem = (file as NSString).lastPathComponent.replacingOccurrences(of: ".html", with: "")
        var stages: [[String: Any]] = []
        let crops = out.appendingPathComponent("crops")
        try FileManager.default.createDirectory(at: crops, withIntermediateDirectories: true)
        for stage in (test["stages"] as? [[String: Any]]) ?? [] {
            guard let name = stage["name"] as? String, ((stage["themes"] as? [String]) ?? []).contains(theme) else {
                continue
            }
            let rect = ((stage["rect"] as? [NSNumber]) ?? []).map { $0.doubleValue }
            let relative = "crops/\(stem).\(theme).\(name).png"
            if session.crop(rect, into: out.appendingPathComponent(relative)) {
                stages.append(["name": name, "crop": relative])
            } else {
                stages.append(["name": name, "status": "skip", "diff": NSNull(), "detail": "no snapshot"])
            }
        }
        let bridge: [[String: Any]] = state.bridge.map { call in
            let args = (try? JSONSerialization.jsonObject(with: Data(call.argsJson.utf8), options: [.fragmentsAllowed])) ?? []
            return ["method": call.method, "args": (args as? [Any]) ?? [args]]
        }
        let errors = state.logs.filter { $0.level == "error" }.map { String(($0.text.components(separatedBy: "\n").first ?? "").prefix(200)) }
        let checks = (published?["checks"] as? [[Any]]) ?? []
        let counts = Dictionary(grouping: checks.compactMap { $0.count > 2 ? $0[2] as? String : nil }, by: { $0 }).mapValues { $0.count }
        print("CONFORMANCE \(id) \(theme) \(published?["status"] as? String ?? "none") \(counts.sorted { $0.key < $1.key }.map { "\($0.key) \($0.value)" }.joined(separator: " ")) chatFocused=\(chatFocused) typed=\(session.field.stringValue.debugDescription) height=\(state.height)\(state.dead.map { " DEAD \($0)" } ?? "")")
        return [
            "id": id, "theme": theme, "width": Double(ConformanceRunnerTests.width),
            "loaded": state.loaded, "dead": state.dead ?? NSNull(), "status": published?["status"] as? String ?? "none",
            "checks": checks, "stages": stages,
            "observed": ["bridge": bridge, "height": Double(state.height), "chatFocused": chatFocused, "focusHolder": focusHolder, "syntheticInput": false] as [String: Any],
            "skippedActions": skipped, "errors": errors,
        ]
    }

    func testAutomaticSet() throws {
        let environment = ProcessInfo.processInfo.environment
        guard environment["TW_CONFORMANCE"] == "1" else {
            throw XCTSkip("set TW_CONFORMANCE=1 to run the widget-sandbox conformance set")
        }
        let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let sandbox = environment["TWIDGET_WIDGET_SANDBOX"].map { URL(fileURLWithPath: $0) } ?? repository.deletingLastPathComponent().appendingPathComponent("widget-sandbox")
        let root = sandbox.appendingPathComponent("packages/conformance")
        let out = URL(fileURLWithPath: environment["TW_CONFORMANCE_OUT"] ?? NSTemporaryDirectory() + "twidget-conformance")
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let features = Set((environment["TW_CONFORMANCE_FEATURES"] ?? "webAnimations").split(separator: ",").map(String.init))
        let only = environment["TW_CONFORMANCE_ONLY"].map { Set($0.split(separator: ",").map(String.init)) }
        let manifest = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("manifest.json"))) as? [String: Any])
        let tests = ((manifest["tests"] as? [[String: Any]]) ?? []).filter { test in
            guard test["set"] as? String == "auto", let id = test["id"] as? String else { return false }
            return only.map { $0.contains(id) || $0.contains(String(id.dropFirst("auto/".count))) } ?? true
        }
        XCTAssertFalse(tests.isEmpty)
        var runs: [[String: Any]] = []
        for test in tests {
            for theme in (test["themes"] as? [String]) ?? ["light"] {
                runs.append(try autoreleasepool { try run(test, theme: theme, root: root, out: out, features: features) })
            }
        }
        let version = ProcessInfo.processInfo.operatingSystemVersion
        let tag = ConformanceRunnerTests.git(sandbox.path, ["describe", "--tags", "--always"])
        let file: [String: Any] = [
            "platform": "macos",
            "renderer": "twidget-litehtml",
            "engine": "macOS \(version.majorVersion).\(version.minorVersion).\(version.patchVersion), \(ConformanceRunnerTests.machine), DPR \(NSScreen.main?.backingScaleFactor ?? 1)",
            "framework": "\(tag) (\(ConformanceRunnerTests.git(sandbox.path, ["rev-parse", "--short", "HEAD"])))",
            "commit": ConformanceRunnerTests.git(repository.path, ["rev-parse", "--short", "HEAD"]),
            "policy": "strict",
            "features": features.sorted(),
            "date": ISO8601DateFormatter().string(from: Date()),
            "runs": runs,
        ]
        let data = try JSONSerialization.data(withJSONObject: file, options: [.prettyPrinted, .sortedKeys])
        let path = out.appendingPathComponent("macos-twidget-litehtml.run.json")
        try data.write(to: path)
        print("CONFORMANCE run file \(path.path)")
    }
}
