import AppKit
import Foundation
import JavaScriptCore
import HTMLSandbox

public protocol HTMLScriptHost: AnyObject {
    func scriptDidChange(_ script: HTMLScript)
}

public final class HTMLScript {
    private static let maxTimers = 16
    private static let maxHTMLLength = 256 * 1024
    private static let maxLogLines = 200
    private static let minTimerInterval: TimeInterval = 0.050
    private static let transitionWindow: CFTimeInterval = 0.8
    private static let transitions = ["fade", "slide-left", "slide-right", "slide-up", "slide-down", "pop", "none"]

    public weak var host: HTMLScriptHost?
    public static var librarySource: ((String) -> String?)?
    public let scene3D: HTML3DSession?

    public static func setDisplay(_ display: CGDirectDisplayID) {
        let engine = HTMLScriptEngine.shared
        engine.perform {
            engine.setDisplay(display)
        }
    }

    private let engine = HTMLScriptEngine.shared
    private let source: String
    private let libraries: [String]
    private let staticHTML: String
    private let styles: NSString

    private let lock = NSLock()
    private var publishedHTML: String
    private var publishedTransition: String?
    private var publishedStopped = false
    private var publishedError: String?
    private var publishedLog: [String] = []
    private var publishedLastCall: Double = 0
    private var publishedTotal: Double = 0
    private var publishedFrames = 0
    private var visible = false

    private var context: JSContext?
    private var hooks: JSValue?
    private var exception: JSValue?
    private var started = false
    private var starting = false
    private var stopped = false
    private var error: String?
    private var pendingError: String?
    private var pendingTransition: String?
    private var pendingTransitionTime: CFTimeInterval = 0
    private var rendered: NSString?
    private var html: NSString
    private var log: [String] = []
    private var dueTimeouts: [Int] = []
    private var framesWanted = false
    private var framePending = false
    private var needsRender = false
    private var renderScheduled = false
    private var lastRenderTime: CFAbsoluteTime = 0
    private var handlingAction = false
    private var budget = CpuBudget(cpuLimitMs: HTMLScriptEngine.callLimit * 1000, cpuQuotaMs: 200, cpuQuotaWindowMs: 1000, cpuKillAfterMs: 0)
    private let clockOrigin = CACurrentMediaTime()

    public static func make(html: String) -> HTMLScript? {
        let ns = html as NSString
        let blocks = scriptBlockRegex.matches(in: html, range: NSRange(location: 0, length: ns.length))
        if blocks.isEmpty {
            return nil
        }
        let source = blocks.map { ns.substring(with: $0.range(at: 1)) }.joined(separator: "\n")
        let libraries = libraryRegex.matches(in: html, range: NSRange(location: 0, length: ns.length)).map { ns.substring(with: $0.range(at: 1)).lowercased() }
        return HTMLScript(source: source, libraries: libraries, staticHTML: stripScripts(html))
    }

    private init(source: String, libraries: [String], staticHTML: String) {
        self.source = source
        self.libraries = libraries
        self.scene3D = libraries.contains("three") ? HTML3DSession() : nil
        self.staticHTML = staticHTML.trimmingCharacters(in: .whitespacesAndNewlines)
        self.styles = HTMLScript.styleBlocks(self.staticHTML) as NSString
        self.html = self.staticHTML as NSString
        self.publishedHTML = self.staticHTML
    }

    public var currentHTML: String {
        if !isStarted {
            engine.performAndWait { [self] in
                if !started {
                    start()
                }
            }
        }
        lock.lock()
        defer { lock.unlock() }
        return publishedHTML
    }

    public var transition: String? {
        lock.lock()
        defer { lock.unlock() }
        return publishedTransition
    }

    public var isStopped: Bool {
        lock.lock()
        defer { lock.unlock() }
        return publishedStopped
    }

    public var errorText: String? {
        lock.lock()
        defer { lock.unlock() }
        return publishedError
    }

    public var logLines: [String] {
        lock.lock()
        defer { lock.unlock() }
        return publishedLog
    }

    public var lastCallMilliseconds: Double {
        lock.lock()
        defer { lock.unlock() }
        return publishedLastCall
    }

    public var totalMilliseconds: Double {
        lock.lock()
        defer { lock.unlock() }
        return publishedTotal
    }

    public var frameCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return publishedFrames
    }

    private var isStarted: Bool {
        lock.lock()
        defer { lock.unlock() }
        return startedFlag
    }
    private var startedFlag = false

    public func setVisible(_ visible: Bool) {
        lock.lock()
        let changed = self.visible != visible
        self.visible = visible
        lock.unlock()
        if changed, visible {
            engine.perform { [weak self] in
                self?.becameVisible()
            }
        }
    }

    private var isVisible: Bool {
        lock.lock()
        defer { lock.unlock() }
        return visible
    }

    public func performAction(_ name: String, value: String?) {
        engine.perform { [weak self] in
            self?.runAction(name, value: value)
        }
    }

    public func performInput(_ type: String, _ a: Any, _ b: Double) {
        engine.perform { [weak self] in
            self?.runInput(type, a, b)
        }
    }

    private func start() {
        assert(engine.thread.isCurrent)
        started = true
        starting = true
        lock.lock()
        startedFlag = true
        lock.unlock()
        let context = JSContext(virtualMachine: engine.machine)!
        context.name = "HTML message"
        context.exceptionHandler = { [weak self] _, exception in
            self?.exception = exception
        }
        let native = JSValue(newObjectIn: context)!
        let schedule: @convention(block) (Int, Double, Bool) -> Void = { [weak self] id, ms, repeats in
            self?.scheduleTimer(id, milliseconds: ms, repeats: repeats)
        }
        let cancel: @convention(block) (Int) -> Void = { [weak self] id in
            self?.cancelTimer(id)
        }
        let update: @convention(block) () -> Void = { [weak self] in
            self?.setNeedsRender()
        }
        let requestFrame: @convention(block) () -> Void = { [weak self] in
            self?.requestFrame()
        }
        let log: @convention(block) (String, String) -> Void = { [weak self] level, text in
            self?.appendLog(text, level: level)
        }
        let transition: @convention(block) (String) -> Void = { [weak self] kind in
            self?.requestTransition(kind)
        }
        let copy: @convention(block) (String) -> Void = { [weak self] text in
            self?.copyText(text)
        }
        native.setObject(schedule, forKeyedSubscript: "schedule" as NSString)
        native.setObject(cancel, forKeyedSubscript: "cancel" as NSString)
        native.setObject(update, forKeyedSubscript: "update" as NSString)
        native.setObject(requestFrame, forKeyedSubscript: "requestFrame" as NSString)
        native.setObject(log, forKeyedSubscript: "log" as NSString)
        native.setObject(transition, forKeyedSubscript: "transition" as NSString)
        native.setObject(copy, forKeyedSubscript: "copy" as NSString)
        self.context = context
        hooks = call("boot") {
            context.evaluateScript(htmlScriptPrelude)?.call(withArguments: [native, HTMLScript.maxTimers, HTMLScript.maxHTMLLength, HTMLScript.transitions])
        }
        if let scene3D = scene3D, let library = HTMLScript.librarySource?("three") {
            let bridge = scene3D.install(in: context)
            let engine = self.engine
            _ = call("boot") {
                engine.withCallLimit(HTMLScriptEngine.libraryLimit) {
                    context.evaluateScript(library, withSourceURL: URL(string: "three.js"))
                }
            }
            _ = call("boot") {
                context.evaluateScript(html3DThreeAdapter)?.call(withArguments: [bridge])
            }
        }
        let source = self.source
        _ = call("boot") {
            context.evaluateScript(source, withSourceURL: URL(string: "message.js"))
        }
        renderNow()
        starting = false
    }

    private func now() -> Double {
        return (CACurrentMediaTime() - clockOrigin) * 1000
    }

    @discardableResult
    private func call(_ kind: String, _ body: () -> JSValue?) -> JSValue? {
        if stopped {
            return nil
        }
        engine.beginCall()
        exception = nil
        let t0 = CFAbsoluteTimeGetCurrent()
        let result = body()
        let elapsed = CFAbsoluteTimeGetCurrent() - t0
        let memoryBefore = engine.footprintBaseline
        let memoryAfter = elapsed > 0.002 ? HTMLScriptEngine.footprint() : 0
        budget.record(kind, cpuMs: elapsed * 1000, now: now())
        lock.lock()
        publishedLastCall = elapsed * 1000
        publishedTotal += elapsed * 1000
        lock.unlock()

        let thrown = exception
        exception = nil
        if engine.terminated {
            stop(reason: String(format: "a call ran longer than %.0f ms", HTMLScriptEngine.callLimit * 1000))
            return nil
        }
        if memoryAfter > memoryBefore, memoryAfter - memoryBefore > HTMLScriptEngine.memoryLimitBytes {
            stop(reason: "it used more than \(HTMLScriptEngine.memoryLimitBytes >> 20) MB of memory in one call")
            return nil
        }
        if let thrown = thrown {
            var message = hooks?.invokeMethod("describe", withArguments: [thrown])?.toString() ?? thrown.toString() ?? ""
            if engine.terminated || exception != nil || message.isEmpty {
                message = "Uncaught exception"
            }
            exception = nil
            pendingError = message
            appendLog(message, level: "error")
            return nil
        }
        return result
    }

    private func stop(reason: String) {
        stopped = true
        error = "Script stopped: " + reason
        appendLog(error!, level: "error")
        engine.cancelTimers(for: self)
        dueTimeouts.removeAll()
        framesWanted = false
        engine.cancelFrames(for: self)
        hooks = nil
        context = nil
        lock.lock()
        publishedStopped = true
        lock.unlock()
        publish()
    }

    private func renderNow() {
        needsRender = false
        lastRenderTime = CFAbsoluteTimeGetCurrent()
        let out = call("frame") { hooks?.invokeMethod("callRender", withArguments: []) }
        if let out = out, out.isString {
            rendered = out.toObject() as? NSString
        }
        if !stopped {
            error = pendingError
        }
        pendingError = nil
        publish()
    }

    private func publish() {
        var html: NSString = rendered.map { styles.appending($0 as String) as NSString } ?? staticHTML as NSString
        if let error = error {
            html = html.appending("\n<div class=\"lc-script-error\">⚠︎ \(HTMLScript.escape(error))\(stopped ? " · <a href=\"chat:restart-script\">↻ Restart</a>" : "")</div>") as NSString
        }
        if pendingTransition != nil, CACurrentMediaTime() - pendingTransitionTime > HTMLScript.transitionWindow {
            pendingTransition = nil
        }
        lock.lock()
        publishedError = error
        lock.unlock()
        if html.isEqual(to: self.html as String) {
            return
        }
        let transition = starting ? nil : pendingTransition
        pendingTransition = nil
        self.html = html
        lock.lock()
        publishedHTML = html as String
        publishedTransition = transition
        lock.unlock()
        if !starting {
            engine.scriptChanged(self)
        }
    }

    func deliverChange() {
        host?.scriptDidChange(self)
    }

    private func requestTransition(_ kind: String) {
        if stopped || starting {
            return
        }
        pendingTransition = kind == "none" ? nil : kind
        pendingTransitionTime = CACurrentMediaTime()
    }

    private func setNeedsRender() {
        if stopped {
            return
        }
        needsRender = true
        if renderScheduled {
            return
        }
        renderScheduled = true
        let wait = max(0, lastRenderTime + engine.frameInterval - CFAbsoluteTimeGetCurrent())
        let render: () -> Void = { [weak self] in
            guard let self = self else {
                return
            }
            self.renderScheduled = false
            if self.needsRender, self.isVisible {
                self.renderNow()
            }
        }
        if wait > 0 {
            engine.perform(after: wait, render)
        } else {
            engine.perform(render)
        }
    }

    private func runAction(_ name: String, value: String?) {
        if !started {
            start()
        }
        guard !stopped, let context = context else {
            return
        }
        let argument: Any = value ?? JSValue(undefinedIn: context) as Any
        handlingAction = true
        _ = call("event") { hooks?.invokeMethod("callAction", withArguments: [name, argument]) }
        handlingAction = false
        renderNow()
    }

    private func runInput(_ type: String, _ a: Any, _ b: Double) {
        if !started {
            start()
        }
        guard !stopped else {
            return
        }
        handlingAction = true
        _ = call("event") { hooks?.invokeMethod("callInput", withArguments: [type, a, b]) }
        handlingAction = false
        renderNow()
    }

    private func copyText(_ text: String) {
        guard handlingAction, !text.isEmpty else {
            return
        }
        DispatchQueue.main.async {
            let board = NSPasteboard.general
            board.clearContents()
            board.setString(text, forType: .string)
        }
    }

    private func scheduleTimer(_ id: Int, milliseconds: Double, repeats: Bool) {
        if stopped {
            return
        }
        let interval = max(HTMLScript.minTimerInterval, milliseconds / 1000)
        engine.scheduleTimer(for: self, id: id, interval: interval, repeats: repeats)
    }

    private func cancelTimer(_ id: Int) {
        engine.cancelTimer(for: self, id: id)
        dueTimeouts.removeAll { $0 == id }
    }

    func timerFired(_ id: Int, repeats: Bool) {
        if !isVisible {
            if !repeats, !dueTimeouts.contains(id) {
                dueTimeouts.append(id)
            }
            return
        }
        if case let .defer_(retryAt) = budget.decide("timer", now: now()) {
            if !repeats {
                engine.perform(after: max(0, retryAt - now()) / 1000) { [weak self] in
                    self?.timerFired(id, repeats: false)
                }
            }
            return
        }
        call("timer") { hooks?.invokeMethod("fire", withArguments: [id]) }
        setNeedsRender()
    }

    private func requestFrame() {
        if stopped {
            return
        }
        framesWanted = true
        if !isVisible {
            framePending = true
            return
        }
        engine.requestFrame(for: self)
    }

    func frameTick() {
        guard framesWanted, !stopped else {
            return
        }
        if !isVisible {
            framePending = true
            return
        }
        if case let .defer_(retryAt) = budget.decide("frame", now: now()) {
            engine.perform(after: max(0, retryAt - now()) / 1000) { [weak self] in
                guard let self = self, self.framesWanted else {
                    return
                }
                self.engine.requestFrame(for: self)
            }
            return
        }
        framesWanted = false
        let more = call("frame") { hooks?.invokeMethod("frame", withArguments: []) }
        lock.lock()
        publishedFrames += 1
        lock.unlock()
        if let more = more, more.toBool() {
            framesWanted = true
            engine.requestFrame(for: self)
        }
        renderNow()
    }

    private func becameVisible() {
        guard started, !stopped, isVisible else {
            return
        }
        let due = dueTimeouts
        dueTimeouts.removeAll()
        for id in due {
            call("timer") { hooks?.invokeMethod("fire", withArguments: [id]) }
            needsRender = true
        }
        if needsRender {
            renderNow()
        }
        if framePending {
            framePending = false
            requestFrame()
        }
    }

    private func appendLog(_ text: String, level: String) {
        let line = level == "log" ? text : "[\(level)] \(text)"
        log.append(line)
        if log.count > HTMLScript.maxLogLines {
            log.removeFirst(log.count - HTMLScript.maxLogLines)
        }
        lock.lock()
        publishedLog = log
        lock.unlock()
    }

    private static let scriptBlockRegex = try! NSRegularExpression(pattern: "<script\\b[^>]*>([\\s\\S]*?)</script\\s*>", options: [.caseInsensitive])
    private static let libraryRegex = try! NSRegularExpression(pattern: "<script\\b[^>]*\\bsrc\\s*=\\s*[\"']app:([a-zA-Z0-9-]+)[\"'][^>]*>", options: [.caseInsensitive])
    private static let scriptTagRegex = try! NSRegularExpression(pattern: "</?script\\b[^>]*>", options: [.caseInsensitive])
    private static let styleBlockRegex = try! NSRegularExpression(pattern: "<style\\b[^>]*>[\\s\\S]*?</style\\s*>", options: [.caseInsensitive])

    private static func stripScripts(_ html: String) -> String {
        let range = NSRange(location: 0, length: (html as NSString).length)
        let withoutBlocks = scriptBlockRegex.stringByReplacingMatches(in: html, range: range, withTemplate: "")
        return scriptTagRegex.stringByReplacingMatches(in: withoutBlocks, range: NSRange(location: 0, length: (withoutBlocks as NSString).length), withTemplate: "")
    }

    private static func styleBlocks(_ html: String) -> String {
        let ns = html as NSString
        return styleBlockRegex.matches(in: html, range: NSRange(location: 0, length: ns.length)).map { ns.substring(with: $0.range) + "\n" }.joined()
    }

    private static func escape(_ text: String) -> String {
        return text.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;").replacingOccurrences(of: "\n", with: "<br>")
    }
}
