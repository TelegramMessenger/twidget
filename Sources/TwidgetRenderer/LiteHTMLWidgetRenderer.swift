import AppKit
import IOSurface
import CoreVideo
import LiteHTML
import TwidgetProtocol

public final class LiteHTMLWidgetRenderer {
    public let view: NSView
    public var onMessage: ((String) -> Void)? {
        didSet {
            flushBuffered()
        }
    }
    public var onDiag: ((String, String) -> Void)?
    public var schedule: (@escaping () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) }
    public var scheduleAfter: (TimeInterval, @escaping () -> Void) -> Void = { DispatchQueue.main.asyncAfter(deadline: .now() + $0, execute: $1) }
    public var isOnExecutor: () -> Bool = { Thread.isMainThread }
    public lazy var scheduleUrgent: (@escaping () -> Void) -> Void = { [weak self] block in self?.schedule(block) }
    public var executorIsMain: () -> Bool = { true }

    func onExecutor(_ block: @escaping () -> Void) {
        if isOnExecutor() {
            block()
        } else {
            schedule(block)
        }
    }
    public var onSerialized: ((String, Bool) -> Void)?
    public private(set) var layoutCount = 0
    public private(set) var layoutMilliseconds = 0.0
    public private(set) var layoutMaxMilliseconds = 0.0
    public private(set) var stageMilliseconds: [String: Double] = [:]
    public var drawMilliseconds: Double {
        return documentView.drawMilliseconds
    }
    public var drawCount: Int {
        return documentView.drawCount
    }
    public var drawMaxMilliseconds: Double {
        return documentView.drawMaxMilliseconds
    }
    public var verification: (frames: Int, mismatched: Int) {
        return (documentView.verifiedFrames, documentView.mismatchedFrames)
    }

    private let documentView: WidgetDocumentView
    let dom = WidgetDOM()
    private var theme: String
    private(set) var document: LHDocument?
    private var buffered: [String] = []
    private var listened: Set<String> = []
    private var prevented: Set<String> = []
    private var lastSize = ""
    private var sizeTransition = false
    private var frozen = false
    private var destroyed = false
    private var needsRender = false
    private var frameRequested = false
    private var idleTicks = 0
    private var controlsActive = false
    private var lastFramePost: CFTimeInterval = 0
    private static let frameInterval = ProcessInfo.processInfo.environment["TW_RAF_FPS"].flatMap { Double($0) }.map { 1 / $0 - 0.002 } ?? 1.0 / 60 - 0.002
    private var clock: WidgetFrameClock?
    private let timeOrigin = CACurrentMediaTime()
    private var hiddenSince: CFTimeInterval? = CACurrentMediaTime()
    private var hiddenTotal: CFTimeInterval = 0

    private var animationNow: CFTimeInterval {
        return (hiddenSince ?? CACurrentMediaTime()) - hiddenTotal
    }

    private func updateAnimationClock() {
        let onScreen = isOnScreen
        if onScreen, let since = hiddenSince {
            hiddenTotal += CACurrentMediaTime() - since
            hiddenSince = nil
        } else if !onScreen, hiddenSince == nil {
            hiddenSince = CACurrentMediaTime()
        }
    }
    private var pendingMoves: [String: [String: Any]] = [:]
    private var bounds: [Int: CGRect] = [:]
    private var boundsValid = false
    private var hoverId = 0
    private var pressId = 0
    private var lastPointer: CGPoint?
    private var clickCount = 0
    private var lastClick: (time: CFTimeInterval, point: CGPoint) = (0, .zero)
    private var observedResize: Set<Int> = []
    private var displayList: LHDisplayList?
    private let animator = WidgetAnimator()
    private lazy var compositor = WidgetCompositor(host: documentView.contentLayer)
    let scrolling = WidgetScrolling()
    private var lastScrollExclusions: Set<Int> = []
    private lazy var magic: WidgetMagicMove = {
        let magic = WidgetMagicMove(host: documentView.contentLayer)
        magic.onFinish = { [weak self] in
            self?.finishMagicMove()
        }
        return magic
    }()
    public var magicMoveCount: Int {
        return magic.transitions
    }
    public var tileLayerCount: Int {
        return documentView.tileCount
    }
    private let statsLock = NSLock()
    private var publishedComposited = 0

    public var compositedCount: Int {
        if isOnExecutor() {
            return compositor.ids.count
        }
        statsLock.lock()
        defer { statsLock.unlock() }
        return publishedComposited
    }
    public var promotionCount: Int {
        return compositor.promotions
    }
    public var rereachCount: Int {
        return compositor.rereaches
    }
    private(set) var canvases: [Int: WidgetCanvas] = [:]
    private var canvasAttributeSizes: [Int: (Int, Int)] = [:]
    private var canvasURLs: [Int: String] = [:]
    private let uid = UUID().uuidString.lowercased()
    private var styleSheetKey = ""
    private var overrides: [Int: [(String, String)]] = [:]
    private var lastResize: [Int: CGSize] = [:]
    private var webUnderlying: [Int: [String: String]] = [:]

    static let maxCanvases = 64
    static let maxCanvasPixels = 4096 * 4096
    static let maxSurfaces = 8
    static let surfaceAttribute = "data-tw3d"

    public var surfaceLayer: ((String) -> CALayer?)? {
        didSet {
            placeSurfaces()
        }
    }
    public var surfaceLayout: ((String, CGSize, CGFloat, Bool) -> Void)?
    private var surfaceContainer: CALayer?
    private var placedSurfaces: [String: CALayer] = [:]
    private var surfaceTokens: [String] = []
    private var surfaceRevision: UInt64 = .max

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
        dom.onProperty = { [weak self] node, name, value in
            self?.setProperty(node, name, value)
        }
        dom.onCall = { [weak self] node, method, args in
            self?.call(node, method, args)
        }
        dom.onCanvas = { [weak self] node, commands in
            self?.canvas(for: node).run(commands)
        }
        WidgetCanvasImages.install()
        animator.onEvent = { [weak self] type, id, _, _ in
            self?.dispatchAnimationEvent(type, id)
        }
        animator.underlying = { [weak self] id, property in
            self?.webUnderlying[id]?[property]
        }
        animator.web.isAlive = { [weak self] id in
            self?.dom.node(id) != nil
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
        let size = documentView.size
        let scale = documentView.scale
        guard let image = document.copyImage(maxWidth: size.width, rect: CGRect(origin: .zero, size: size), scale: scale) else {
            return nil
        }
        if compositor.ids.isEmpty && !magic.isActive && presenters.isEmpty && scrolling.layers.isEmpty {
            return NSImage(cgImage: image, size: size)
        }
        guard let ctx = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: 0, space: image.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue) else {
            return NSImage(cgImage: image, size: size)
        }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        ctx.translateBy(x: 0, y: CGFloat(image.height))
        ctx.scaleBy(x: scale, y: -scale)
        scrolling.draw(into: ctx, document: document, scale: scale)
        for (_, presenter) in presenters.sorted(by: { $0.key < $1.key }) {
            presenter.draw(into: ctx)
        }
        compositor.draw(into: ctx)
        magic.draw(into: ctx)
        return ctx.makeImage().map { NSImage(cgImage: $0, size: size) } ?? NSImage(cgImage: image, size: size)
    }

    public func destroy() {
        destroyed = true
        magic.cancel()
        for (id, url) in canvasURLs {
            WidgetCanvasImages.unregister(uid + "-" + String(id))
            LHImages.forgetImage(withURL: url)
        }
        canvasURLs.removeAll()
        for presenter in presenters.values {
            presenter.remove()
        }
        presenters.removeAll()
        for token in placedSurfaces.keys {
            surfaceLayout?(token, .zero, 1, false)
        }
        placedSurfaces.removeAll()
        surfaceContainer?.removeFromSuperlayer()
        surfaceContainer = nil
        clock?.stop()
        clock = nil
        trimWork?.cancel()
        trimWork = nil
        documentView.owner = nil
        let detachedView = documentView
        if Thread.isMainThread {
            detachedView.removeFromSuperview()
        } else {
            DispatchQueue.main.async {
                detachedView.removeFromSuperview()
            }
        }
        var retired: [AnyObject] = documentView.retire()
        if let document = document {
            retired.append(document)
        }
        if let displayList = displayList {
            retired.append(displayList)
        }
        document = nil
        displayList = nil
        WidgetTeardown.release(retired)
    }

    static let messageLog = ProcessInfo.processInfo.environment["TW_MSG_LOG"] != nil

    private enum Held {
        case json(String)
        case ops([WidgetOp])
    }

    private var held: [Held] = []
    private var prepareScheduled = false

    private var preparesOffscreenValue = false

    public var preparesOffscreen: Bool {
        get {
            return preparesOffscreenValue
        }
        set {
            guard isOnExecutor() else {
                return scheduleUrgent { [weak self] in self?.preparesOffscreen = newValue }
            }
            if preparesOffscreenValue != newValue {
                preparesOffscreenValue = newValue
                updateTileTrim()
                schedulePrepare()
            }
        }
    }

    public static var preparedReport: String {
        return WidgetDocumentView.preparedReport
    }

    public var preparedTileCount: Int {
        return documentView.preparedTiles
    }

    public var preparedMilliseconds: Double {
        return documentView.preparedMilliseconds
    }

    public static func quiesceRaster() {
        WidgetDocumentView.prepareQueue.sync(flags: .barrier) {}
    }

    public var tileDebug: String {
        return "detached=\(documentView.detached) preparing=\(documentView.preparing) tiles=\(documentView.tileCount) trimmed=\(documentView.tilesTrimmed) \(documentView.flushDebug) doc=\(document != nil) h=\(document.map { Int($0.size.height) } ?? -1) inWindow=\(documentView.windowState.inWindow) onScreen=\(isOnScreen) hostVisible=\(hostVisible) frozen=\(frozen) destroyed=\(destroyed)"
    }

    public var isPrepared: Bool {
        return document != nil && !documentView.preparing && !documentView.hasUnpreparedTiles
    }

    private func schedulePrepare() {
        guard preparesOffscreen, !destroyed, !prepareScheduled, document != nil, !documentView.windowState.inWindow else {
            return
        }
        prepareScheduled = true
        schedule { [weak self] in
            guard let self = self else {
                return
            }
            self.prepareScheduled = false
            guard self.preparesOffscreen, !self.destroyed, !self.documentView.windowState.inWindow else {
                return
            }
            _ = self.documentView.prepareTiles { [weak self] in
                self?.rasterFinished()
            }
        }
    }

    func rasterFinished() {
        if documentView.detached {
            reportSize()
        }
        prepared()
        if renderDeferred {
            renderDeferred = false
            render()
        }
    }

    private var renderDeferred = false

    public var asyncTileReport: (jobs: Int, installed: Int) {
        return (documentView.asyncTileJobs, documentView.asyncTilesInstalled)
    }

    private var flushingHeld = false

    public static var slowApply: ((_ ops: Int, _ milliseconds: Double, _ held: Bool, _ stages: [String: Double]) -> Void)?

    private func applyMeasured(_ ops: [WidgetOp], held: Bool) {
        guard let report = LiteHTMLWidgetRenderer.slowApply else {
            return apply(ops)
        }
        let before = stageMilliseconds
        let start = CACurrentMediaTime()
        apply(ops)
        let spent = (CACurrentMediaTime() - start) * 1000
        if spent > 8 {
            var stages: [String: Double] = [:]
            for (key, value) in stageMilliseconds where value - (before[key] ?? 0) > 0.5 {
                stages[key] = value - (before[key] ?? 0)
            }
            report(ops.count, spent, held, stages)
        }
    }

    private func prepared() {
        guard !documentView.preparing, !destroyed else {
            return
        }
        let start = CACurrentMediaTime()
        flushingHeld = true
        while !held.isEmpty && !documentView.preparing {
            var ops: [WidgetOp] = []
            while case .ops(let batch)? = held.first {
                ops += batch
                held.removeFirst()
            }
            if !ops.isEmpty {
                applyMeasured(ops, held: true)
            } else if case .json(let json)? = held.first {
                held.removeFirst()
                send(json)
            }
            if CACurrentMediaTime() - start > 0.004 {
                break
            }
        }
        flushingHeld = false
        if !held.isEmpty && !documentView.preparing {
            schedule { [weak self] in
                self?.prepared()
            }
        }
        schedulePrepare()
    }

    public func send(_ json: String) {
        if (documentView.preparing || (!held.isEmpty && !flushingHeld)) && !destroyed {
            held.append(.json(json))
            return
        }
        if !destroyed, LiteHTMLWidgetRenderer.canvasFastPath, json.hasPrefix("{\"type\":\"ops\",\"ops\":[{\"op\":\"canvas\""), let list = CanvasStream.canvasOps(json: json) {
            sendCanvas(list)
            return
        }
        guard !destroyed, let data = json.data(using: .utf8), let message = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any], let type = message["type"] as? String else {
            return
        }
        if LiteHTMLWidgetRenderer.messageLog, type != "ops" {
            print(String(format: "MSG %.4f %@", CACurrentMediaTime(), type))
        }
        switch type {
        case "ops":
            apply(((message["ops"] as? [Any]) ?? []).compactMap { ($0 as? [String: Any]).map(WidgetOp.init) })
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
        case "anim":
            animator.web.apply(message)
            for item in (message["anims"] as? [[String: Any]]) ?? [] where item["css"] != nil {
                animator.applyCSSOverride(item, offset: hiddenTotal)
            }
            if !frozen {
                render()
            }
        default:
            break
        }
    }

    public var acceptsTypedOps: Bool {
        return true
    }

    public func send(ops: [WidgetOp]) {
        guard !destroyed else {
            return
        }
        if documentView.preparing || !held.isEmpty {
            held.append(.ops(ops))
            return
        }
        applyMeasured(ops, held: false)
    }

    private func apply(_ ops: [WidgetOp]) {
        defer {
            pacer.answered(CACurrentMediaTime())
        }
        if LiteHTMLWidgetRenderer.canvasFastPath, !ops.isEmpty, ops.allSatisfy({ $0.kind == .canvas && $0.canvas != nil }) {
            sendCanvas(ops.map { ($0.id, CanvasStream(batch: $0.canvas!)) })
            return
        }
        if LiteHTMLWidgetRenderer.messageLog {
            print(String(format: "MSG %.4f ops %@", CACurrentMediaTime(), ops.map { "\($0.op ?? "?"):\($0.kind == .other ? ($0["name"] as? String) ?? "" : $0.name)" }.joined(separator: ",")))
        }
        if !frozen, let touched = scriptedOnly(ops) {
            for op in ops {
                dom.apply(op)
            }
            stageMilliseconds["scripted-fast", default: 0] += 1
            if compositor.applyScripted(touched, dom: dom, document: document) {
                if !animator.isActive {
                    compositeCheck([:])
                }
                return
            }
            render()
            return
        }
        noteScriptedTransforms(ops)
        var capture: WidgetMagicMove.Capture?
        if dom.usesTransitions, let document = document, !frozen, isOnScreen, WidgetMagicMove.isStructural(ops, dom: dom) {
            capture = magic.capture(dom: dom, document: document)
        }
        for op in ops {
            dom.apply(op)
        }
        if var pending = capture, let document = document {
            magic.collectLeaving(&pending, dom: dom, document: document, scale: documentView.scale)
            capture = pending
        }
        sizeTransition = capture != nil
        render()
        sizeTransition = false
        if let capture = capture, let document = document, magic.run(capture, dom: dom, document: document, skip: animator.animatedIDs().union(compositor.ids), scale: documentView.scale) {
            applyExclusions()
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
        schedule { [weak self] in
            guard let self = self, self.needsRender else {
                return
            }
            self.render()
        }
    }

    private var width: CGFloat {
        return max(1, documentView.size.width)
    }

    private static let resolvedSheetsLock = NSLock()
    private static var resolvedSheets: [String: String] = [:]

    private static func resolvedSheet(_ css: String, theme: String) -> String {
        let key = theme + "\u{1}" + css
        resolvedSheetsLock.lock()
        if let resolved = resolvedSheets[key] {
            resolvedSheetsLock.unlock()
            return resolved
        }
        resolvedSheetsLock.unlock()
        let resolved = CSSTheme.resolve(css, theme: theme)
        resolvedSheetsLock.lock()
        if resolvedSheets.count >= 256 {
            resolvedSheets.removeAll()
        }
        resolvedSheets[key] = resolved
        resolvedSheetsLock.unlock()
        return resolved
    }

    private func render() {
        needsRender = false
        if documentView.preparing && documentView.windowState.inWindow {
            renderDeferred = true
            return
        }
        updateAnimationClock()
        let now = animationNow
        let sheetsStart = CACurrentMediaTime()
        let sheets = dom.styleSheets()
        let key = sheets.joined(separator: "\u{1}")
        if key != styleSheetKey {
            styleSheetKey = key
            animator.setStyleSheets(sheets.map { LiteHTMLWidgetRenderer.resolvedSheet($0, theme: theme) })
        }
        stageMilliseconds["sheets", default: 0] += (CACurrentMediaTime() - sheetsStart) * 1000
        compositor.styleKey = (key + "\u{2}" + theme + "\u{2}\(hoverId)\u{2}\(pressId)").hashValue
        layout(overrides)
        let scanStart = CACurrentMediaTime()
        var declarations: [Int: [String: String]] = [:]
        for (key, values) in document?.declarations(WidgetAnimator.watched, byAttribute: WidgetDOM.idAttribute, requiringAny: WidgetAnimator.entryProperties + ["backdrop-filter", "filter", "box-shadow"]) ?? [:] {
            if let id = Int(key) {
                declarations[id] = values
            }
        }
        if !animator.web.isEmpty, let document = document {
            webUnderlying.removeAll()
            let targets = animator.web.targets
            var names = Set(WidgetAnimator.watched)
            var required = Set<String>()
            for id in targets {
                for animation in animator.web.animations(id) {
                    required.formUnion(animation.properties)
                }
            }
            required.insert("transform-origin")
            names.formUnion(required)
            for (key, values) in document.declarations(Array(names), byAttribute: WidgetDOM.idAttribute, requiringAny: Array(required)) {
                guard let id = Int(key), targets.contains(id) else {
                    continue
                }
                let plain = values.filter { !$0.key.hasPrefix("animation") && !$0.key.hasPrefix("transition") }
                declarations[id] = (declarations[id] ?? [:]).merging(plain) { current, _ in current }
                webUnderlying[id] = values
            }
            for id in targets where declarations[id] == nil && dom.node(id) != nil {
                declarations[id] = [:]
            }
        }
        stageMilliseconds["declarations", default: 0] += (CACurrentMediaTime() - scanStart) * 1000
        animator.update(declarations: declarations, now: now)
        let sampled = animator.sample(now: now)
        let before = compositor.ids
        var scrollersChanged = false
        if let document = document {
            compositor.attach(documentView.contentLayer)
            compositor.update(document: document, animator: animator, dom: dom, declarations: declarations, scale: documentView.scale, space: documentView.windowState.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB)!, extraExclusions: magic.excluded)
            scrolling.attach(documentView.contentLayer)
            scrollersChanged = scrolling.update(document: document, dom: dom, animated: animator.animatedIDs(), promoted: compositor.ids.union(magic.excluded), styleKey: compositor.styleKey, scale: documentView.scale)
            documentView.publishScrollRegions(scrolling.geometries.values.map { $0.viewport })
            statsLock.lock()
            publishedComposited = compositor.ids.count
            statsLock.unlock()
        }
        compositor.drive(animator: animator, now: now, mediaOffset: hiddenTotal, enabled: canDrive)
        let rest = compositor.apply(sampled, bases: { [animator] in animator.base($0) })
        _ = compositor.applyScripted(compositor.scriptedIDs, dom: dom, document: document)
        if !LiteHTMLWidgetRenderer.same(rest, overrides) || before != compositor.ids || scrollersChanged {
            overrides = rest
            layout(overrides)
        }
        scheduleDrivenEnds(now: now)
        updateCanvasPresentation()
        if needsFrames && !frozen {
            startClock()
        }
    }

    static let canvasFastPath = ProcessInfo.processInfo.environment["TW_CANVAS_FAST"] != "0"
    static let canvasGPUAfter = ProcessInfo.processInfo.environment["TW_CANVAS_GPU_AFTER"].flatMap { Int($0) }.map { max(1, $0) } ?? 3
    static let maxPresentedPixels = 2048 * 2048 * 2
    private var presenters: [Int: WidgetCanvasPresenter] = [:]
    private var canvasDraws: [Int: Int] = [:]
    private var presentationEpoch = 0
    private var presentationBackoff = 0
    private var presentationChecked: LHDocument?
    public private(set) var canvasFastMessages = 0

    public var canvasReport: String {
        var accelerated = 0, fallback = 0, gpuFrames = 0, gpuMilliseconds = 0.0
        #if canImport(Metal)
        for canvas in canvases.values {
            if let gpu = canvas.backend as? WidgetCanvasMetal {
                accelerated += gpu.acceleratedBatches
                fallback += gpu.fallbackBatches
                let stats = gpu.gpuStats
                gpuFrames += stats.frames
                gpuMilliseconds += stats.milliseconds
            }
        }
        #endif
        let shown = presenters.values.reduce(0) { $0 + $1.presented }
        let skipped = presenters.values.reduce(0) { $0 + $1.skipped }
        return String(format: "canvas: fast=%d gpu-batches=%d cg-batches=%d gpu-buffers=%d gpu-ms=%.3f/buffer presented=%d layers=%d skipped=%d", canvasFastMessages, accelerated, fallback, gpuFrames, gpuMilliseconds / Double(max(1, gpuFrames)), shown, presenters.count, skipped)
    }

    private func sendCanvas(_ list: [(id: Int, stream: CanvasStream)]) {
        canvasFastMessages += 1
        opsBatch += 1
        var direct = true
        for (id, stream) in list {
            guard let node = dom.canvasNode(id) else {
                continue
            }
            let target = canvas(for: node)
            let size = (target.width, target.height)
            target.execute(stream)
            if presenters[id] == nil || size != (target.width, target.height) {
                direct = false
            }
        }
        if direct {
            stageMilliseconds["canvas-direct", default: 0] += 1
            return
        }
        render()
    }

    private func canvasDrew(_ id: Int) {
        let count = (canvasDraws[id] ?? 0) + 1
        canvasDraws[id] = count
        if let presenter = presenters[id] {
            presenter.present()
            return
        }
        #if canImport(Metal)
        if count + 1 >= LiteHTMLWidgetRenderer.canvasGPUAfter, let canvas = canvases[id], canvas.backend == nil, canvas.storage != nil, WidgetCanvasMetal.enabled {
            canvas.backend = WidgetCanvasMetal()
        }
        #endif
    }

    private func canvasPresentable(_ id: Int, _ canvas: WidgetCanvas, document: LHDocument, boxes: [String: NSValue]) -> (frame: CGRect, clip: (rect: CGRect, radius: CGFloat, corners: CACornerMask)?)? {
        guard WidgetCanvasPresenter.enabled, canvas.storage != nil, canvas.width > 0, canvas.height > 0, !magic.isActive, let node = dom.node(id), node.tag == "canvas", !node.svg else {
            return nil
        }
        let key = String(id)
        guard let box = boxes[key]?.rectValue, box.width >= 1, box.height >= 1, box.width * box.height < 4_000_000 else {
            return nil
        }
        for other in compositor.ids where other == id || isAncestor(other, of: id) {
            return nil
        }
        let style = document.computedStyle(attribute: WidgetDOM.idAttribute, value: key)
        for name in ["padding-top", "padding-right", "padding-bottom", "padding-left", "border-top-width", "border-right-width", "border-bottom-width", "border-left-width"] {
            guard let value = style[name], value == "0px" else {
                return nil
            }
        }
        if let opacity = style["opacity"], let number = Double(opacity), number < 0.999 {
            return nil
        }
        if let visibility = style["visibility"], visibility != "visible" {
            return nil
        }
        let declared = document.declarations(["transform", "filter", "border-radius", "clip-path", "mix-blend-mode", "object-fit", "object-position", "image-rendering", "mask", "-webkit-mask", "backdrop-filter", "outline-style"], byAttribute: WidgetDOM.idAttribute)[key] ?? [:]
        for (name, value) in declared {
            let trimmed = value.trimmingCharacters(in: .whitespaces).lowercased()
            if trimmed.isEmpty || trimmed == "none" || trimmed == "normal" || trimmed == "initial" || trimmed == "fill" || (name == "border-radius" && (trimmed == "0" || trimmed == "0px")) || (name == "image-rendering" && trimmed == "auto") {
                continue
            }
            return nil
        }
        var clipRect = CGRect.zero
        var radii = [CGFloat](repeating: 0, count: 8)
        var clip: (rect: CGRect, radius: CGFloat, corners: CACornerMask)?
        if document.clipOfElement(attribute: WidgetDOM.idAttribute, value: key, rect: &clipRect, radii: &radii) {
            let rounded = radii.filter { $0 > 0.01 }
            guard clipRect.intersects(box), rounded.allSatisfy({ abs($0 - rounded[0]) < 0.01 }) else {
                return nil
            }
            var corners: CACornerMask = []
            let masks: [CACornerMask] = [.layerMinXMinYCorner, .layerMaxXMinYCorner, .layerMaxXMaxYCorner, .layerMinXMaxYCorner]
            for (index, mask) in masks.enumerated() {
                let x = radii[index * 2], y = radii[index * 2 + 1]
                guard (x > 0.01) == (y > 0.01) else {
                    return nil
                }
                if x > 0.01 {
                    corners.insert(mask)
                }
            }
            if !rounded.isEmpty || !clipRect.contains(box) {
                clip = (clipRect, rounded.first ?? 0, corners)
            }
        }
        let visible = clip.map { box.intersection($0.rect) } ?? box
        guard !visible.isNull, !visible.isEmpty, document.isElement(attribute: WidgetDOM.idAttribute, value: key, unobscuredIn: visible, ignoring: [], clipped: clip != nil || !clipRect.isEmpty) else {
            return nil
        }
        return (box, clip)
    }

    private func updateCanvasPresentation() {
        guard let document = document, !presenters.isEmpty || canvasDraws.values.contains(where: { $0 >= LiteHTMLWidgetRenderer.canvasGPUAfter }) else {
            return
        }
        if presentationChecked === document {
            return
        }
        if presenters.isEmpty && presentationBackoff > 0 {
            presentationBackoff -= 1
            return
        }
        presentationChecked = document
        let boxes = document.bounds(byAttribute: WidgetDOM.idAttribute)
        var changed = false
        var added: [WidgetCanvasPresenter] = []
        var budget = LiteHTMLWidgetRenderer.maxPresentedPixels
        for (id, canvas) in canvases.sorted(by: { $0.key < $1.key }) {
            let wanted = presenters[id] != nil || (canvasDraws[id] ?? 0) >= LiteHTMLWidgetRenderer.canvasGPUAfter
            let affordable = canvas.pixelCount <= budget
            let placement = wanted && affordable ? canvasPresentable(id, canvas, document: document, boxes: boxes) : nil
            if placement != nil {
                budget -= canvas.pixelCount
            }
            if let placement = placement {
                let presenter: WidgetCanvasPresenter
                if let existing = presenters[id] {
                    presenter = existing
                } else {
                    presenter = WidgetCanvasPresenter(canvas: canvas)
                    presenter.schedule = { [weak self] block in self?.schedule(block) }
                    presenter.scheduleAfter = { [weak self] seconds, block in self?.scheduleAfter(seconds, block) }
                    presenters[id] = presenter
                    added.append(presenter)
                    changed = true
                }
                if presenter.frame != placement.frame || presenter.clip?.rect != placement.clip?.rect || presenter.clip?.radius != placement.clip?.radius || presenter.clip?.corners != placement.clip?.corners {
                    presenter.place(placement.frame, clip: placement.clip)
                }
            } else if let removed = presenters.removeValue(forKey: id) {
                removed.remove()
                changed = true
            }
        }
        if presenters.isEmpty && !changed {
            presentationBackoff = 30
        }
        guard changed else {
            return
        }
        presentationEpoch += 1
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for presenter in added {
            presenter.presentNow()
            documentView.insertOverlay(presenter.container)
        }
        layout(overrides)
        presentationChecked = document
        CATransaction.commit()
    }

    public private(set) var handoffWorst: (geometry: CGFloat, alpha: Float) = (0, 0)
    public private(set) var handoffOffsets: [Double] = []

    private func renderAnimationFrame() {
        let sampled = animator.sample(now: animationNow)
        if WidgetDocumentView.verify && !compositor.drivenIDs.isEmpty {
            let error = compositor.handoffError(animator: animator, now: animationNow)
            handoffWorst = (max(handoffWorst.geometry, error.geometry), max(handoffWorst.alpha, error.alpha))
            handoffOffsets.append(error.offset)
        }
        let rest = compositor.apply(sampled, bases: { [animator] in animator.base($0) })
        defer {
            compositeCheck(sampled)
        }
        if !LiteHTMLWidgetRenderer.same(rest, overrides) {
            if documentView.preparing {
                stageMilliseconds["raster-busy", default: 0] += 1
            } else {
                overrides = rest
                layout(overrides)
            }
        } else {
            stageMilliseconds["composited-only", default: 0] += 1
        }
        if !compositor.animatedPromotions.isEmpty && !animator.animatedIDs().isSuperset(of: compositor.animatedPromotions) {
            setNeedsRender()
        }
    }

    private func canvas(for node: WidgetNode) -> WidgetCanvas {
        let id = dom.publicId(node)
        if let canvas = canvases[id] {
            return canvas
        }
        let size = canvasAttributeSize(node)
        if canvases.count >= LiteHTMLWidgetRenderer.maxCanvases {
            return WidgetCanvas(width: 0, height: 0, allowance: { 0 })
        }
        let canvas = WidgetCanvas(width: size.0, height: size.1, allowance: { [weak self] in
            guard let self = self else {
                return 0
            }
            let used = self.canvases.filter { $0.key != id }.reduce(0) { $0 + $1.value.pixelCount }
            return LiteHTMLWidgetRenderer.maxCanvasPixels - used
        })
        canvasAttributeSizes[id] = size
        canvas.resolveImage = { [weak self] other in
            self?.image(forNode: other)
        }
        canvas.onDraw = { [weak self] in
            self?.canvasDrew(id)
        }
        #if canImport(Metal)
        if LiteHTMLWidgetRenderer.canvasGPUAfter <= 1, canvas.storage != nil, WidgetCanvasMetal.enabled {
            WidgetCanvasMetal.prepare(wait: true)
            canvas.backend = WidgetCanvasMetal()
        } else if canvas.storage != nil, WidgetCanvasMetal.enabled {
            WidgetCanvasMetal.prepare(wait: false)
        }
        #endif
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
        guard let node = dom.node(id), node.tag == "img", let src = node.attribute("src"), src.hasPrefix("data:") else {
            return nil
        }
        return LHImages.copyImage(forURL: src)
    }

    private func syncCanvases() {
        for (id, canvas) in canvases {
            guard let node = dom.node(id), node.tag == "canvas" else {
                canvases[id] = nil
                canvasAttributeSizes[id] = nil
                canvasDraws[id] = nil
                presenters.removeValue(forKey: id)?.remove()
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
        let url = "twcanvas:" + uid + "-" + String(id) + "-" + (presenters[id] != nil ? "p" + String(presentationEpoch) + "_" + String(canvas.width) + "_" + String(canvas.height) : String(canvas.version))
        if let previous = canvasURLs[id], previous != url {
            retiredURLs.append(previous)
        }
        canvasURLs[id] = url
        return url
    }

    private var retiredURLs: [String] = []
    private var listDebt: [CGRect] = []
    static let slowLayoutMilliseconds = ProcessInfo.processInfo.environment["TW_SLOW_MS"].flatMap { Double($0) }
    static let patchRegions = ProcessInfo.processInfo.environment["TW_NO_PATCH_REGIONS"] == nil
    private var lastLayoutTheme = ""
    private var lastLayoutOverrides: [Int: [(String, String)]]?
    private var lastExclusions: Set<Int> = []
    public private(set) var styleFastMismatches = 0
    static let styleFastPath = ProcessInfo.processInfo.environment["TW_NO_STYLE_FAST"] == nil
    static let holePatches = ProcessInfo.processInfo.environment["TW_NO_HOLE_PATCH"] == nil

    private func patchedDocument(_ overrides: [Int: [(String, String)]]) -> LHDocument? {
        guard LiteHTMLWidgetRenderer.styleFastPath, let document = document, onSerialized == nil, canvases.isEmpty, lastLayoutTheme == theme, let last = lastLayoutOverrides else {
            return nil
        }
        if !LiteHTMLWidgetRenderer.holePatches && (!dom.patchLog.attributes.isEmpty || !dom.patchLog.texts.isEmpty) {
            return nil
        }
        guard let changes = dom.patchChanges(theme: theme, overrides: overrides, previous: last), !changes.isEmpty else {
            return nil
        }
        return LHDocument(previous: document, changes: Data(changes), attribute: WidgetDOM.idAttribute, css: nil)
    }

    private func layout(_ overrides: [Int: [(String, String)]]) {
        let start = CACurrentMediaTime()
        var stages: [(String, Double)] = []
        defer {
            layoutCount += 1
            let spent = (CACurrentMediaTime() - start) * 1000
            layoutMilliseconds += spent
            layoutMaxMilliseconds = max(layoutMaxMilliseconds, spent)
            if let slow = LiteHTMLWidgetRenderer.slowLayoutMilliseconds, spent >= slow {
                print(String(format: "SLOW layout %.2f ms t=%.3f ", spent, CACurrentMediaTime() - timeOrigin) + stages.map { String(format: "%@ %.2f", $0.0, $0.1) }.joined(separator: "  "))
            }
        }
        syncCanvases()
        var mark = CACurrentMediaTime()
        func stage(_ name: String) {
            let now = CACurrentMediaTime()
            stageMilliseconds[name, default: 0] += (now - mark) * 1000
            stages.append((name, (now - mark) * 1000))
            mark = now
        }
        let fast = patchedDocument(overrides)
        stage("style-patch")
        let html = fast == nil || LiteHTMLWidgetRenderer.patchCheck ? dom.serialize(theme: theme, overrides: overrides, canvasSource: { [unowned self] node in self.canvasSource(node) }) : ""
        stage("serialize")
        let next = fast ?? LHDocument(html: html, css: nil, previous: document)
        next.controlsActive = controlsActive
        dom.resetPatchLog()
        if fast != nil {
            stageMilliseconds["style-fast", default: 0] += 1
            if LiteHTMLWidgetRenderer.patchCheck && next.html != html {
                styleFastMismatches += 1
                print("STYLEFAST source differs from serialized HTML")
            }
        }
        lastLayoutTheme = theme
        lastLayoutOverrides = overrides
        let excluded = compositor.ids.union(magic.excluded)
        let scrollExcluded = scrolling.promotedIDs
        let exclusionsChanged = excluded != lastExclusions || scrollExcluded != lastScrollExclusions
        lastExclusions = excluded
        lastScrollExclusions = scrollExcluded
        if !scrollExcluded.isEmpty {
            next.setScrollContentExclusions(Set(scrollExcluded.map { String($0) }), attribute: WidgetDOM.idAttribute)
        }
        if !excluded.isEmpty {
            next.setPaintExclusions(Set(excluded.map { String($0) }), attribute: WidgetDOM.idAttribute)
        }
        stage("document")
        if next.reusedPrevious {
            stageMilliseconds["reused", default: 0] += 1
        }
        onSerialized?(html, next.reusedPrevious)
        defer {
            if LiteHTMLWidgetRenderer.patchCheck {
                lastSerialized = html
            }
        }
        _ = next.layout(maxWidth: width, preferredWidth: width)
        for (id, offset) in scrolling.offsets {
            _ = next.setScrollOffset(offset, attribute: WidgetDOM.idAttribute, value: String(id))
        }
        stage("layout")
        if LiteHTMLWidgetRenderer.patchCheck && next.reusedPrevious {
            patchChecks += 1
            let fresh = LHDocument(html: html, css: nil)
            fresh.controlsActive = controlsActive
            _ = fresh.layout(maxWidth: width, preferredWidth: width)
            let size = CGSize(width: width, height: max(1, ceil(max(next.size.height, fresh.size.height))))
            if !excluded.isEmpty {
                fresh.setPaintExclusions(Set(excluded.map { String($0) }), attribute: WidgetDOM.idAttribute)
            }
            if !scrollExcluded.isEmpty {
                fresh.setScrollContentExclusions(Set(scrollExcluded.map { String($0) }), attribute: WidgetDOM.idAttribute)
            }
            for (id, offset) in scrolling.offsets {
                _ = fresh.setScrollOffset(offset, attribute: WidgetDOM.idAttribute, value: String(id))
            }
            let a = next.copyImage(maxWidth: width, rect: CGRect(origin: .zero, size: size), scale: 2)
            let b = fresh.copyImage(maxWidth: width, rect: CGRect(origin: .zero, size: size), scale: 2)
            let boundsDiffer = next.layoutSignature != fresh.layoutSignature
            if boundsDiffer {
                patchBoundsMismatches += 1
                print("PATCHBOUNDS reusedLayout=\(next.reusedLayout) element bounds differ from a fresh layout")
                if let dir = ProcessInfo.processInfo.environment["TW_PATCHCHECK_DUMP"], patchBoundsMismatches <= 3 {
                    try? next.layoutSignature.write(toFile: dir + "/bounds-patched\(patchBoundsMismatches).txt", atomically: true, encoding: .utf8)
                    try? fresh.layoutSignature.write(toFile: dir + "/bounds-fresh\(patchBoundsMismatches).txt", atomically: true, encoding: .utf8)
                }
            }
            if abs(next.size.height - fresh.size.height) > 0.01 || !LiteHTMLWidgetRenderer.sameImage(a, b) || boundsDiffer {
                patchMismatches += 1
                if let dir = ProcessInfo.processInfo.environment["TW_PATCHCHECK_DUMP"], patchMismatches <= 3 {
                    print("PATCHDBG reusedLayout=\(next.reusedLayout) sizes=\(next.size) \(fresh.size)")
                    for (name, image) in [("patched", a), ("fresh", b)] {
                        if let image = image {
                            try? NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: dir + "/\(name)\(patchMismatches).png"))
                        }
                    }
                    try? html.write(toFile: dir + "/mismatch\(patchMismatches).html", atomically: true, encoding: .utf8)
                    if let previousHTML = lastSerialized {
                        try? previousHTML.write(toFile: dir + "/before\(patchMismatches).html", atomically: true, encoding: .utf8)
                    }
                }
            }
        }
        defer { stage("report") }
        if LiteHTMLWidgetRenderer.patchRegions, next.reusedPrevious, !exclusionsChanged, displayList != nil, documentView.document != nil, let regions = (next.reusedLayout ? next.patchDirtyRects : next.layoutDirtyRects)?.map({ $0.rectValue }) {
            stage("diff-list")
            stageMilliseconds["regions", default: 0] += 1
            if listDebt.count + regions.count > 32 {
                listDebt = [(listDebt + regions).reduce(CGRect.null) { $0.union($1) }]
            } else {
                listDebt += regions
            }
            if LiteHTMLWidgetRenderer.dirtyLog, !regions.isEmpty {
                print("DIRTY t=\(String(format: "%.3f", CACurrentMediaTime() - timeOrigin)) regions \(regions.map { "\(Int($0.minX)),\(Int($0.minY)) \(Int($0.width))x\(Int($0.height))" }.joined(separator: " | "))")
            }
            documentView.asyncAllowed = !exclusionsChanged && !magic.isActive
            documentView.invalidate(regions.map { $0.insetBy(dx: -1, dy: -1) })
            documentView.asyncAllowed = false
        } else {
            let list = next.displayList(maxWidth: width, scale: documentView.scale)
            let previousList = displayList
            displayList = list
            let debt = listDebt
            listDebt.removeAll()
            stage("diff-list")
            if let previous = previousList, documentView.document != nil {
                let rects = list.changedRects(since: previous).map { $0.rectValue } + debt
                if !rects.isEmpty {
                    if LiteHTMLWidgetRenderer.dirtyLog {
                        print("DIRTY t=\(String(format: "%.3f", CACurrentMediaTime() - timeOrigin)) \(rects.map { "\(Int($0.minX)),\(Int($0.minY)) \(Int($0.width))x\(Int($0.height))" }.joined(separator: " | "))")
                    }
                    documentView.asyncAllowed = !exclusionsChanged && !magic.isActive
                    documentView.invalidate(rects.map { $0.insetBy(dx: -1, dy: -1) })
                    documentView.asyncAllowed = false
                }
            } else {
                documentView.invalidate(nil)
            }
            stage("diff")
        }
        document = next
        for url in retiredURLs {
            LHImages.forgetImage(withURL: url)
        }
        retiredURLs.removeAll()
        boundsValid = false
        documentView.document = next
        reportSize()
        reportResize()
        placeSurfaces()
        schedulePrepare()
        documentView.flushIfDetached()
    }

    private static let scriptedProperties: Set<String> = ["transform", "opacity"]
    static let scriptedPromotion = ProcessInfo.processInfo.environment["TW_NO_SCRIPTED"] == nil
    private var scriptedHits: [Int: (count: Int, batch: Int)] = [:]
    private var opsBatch = 0

    private func scriptedOnly(_ ops: [WidgetOp]) -> Set<Int>? {
        let promoted = compositor.scriptedIDs
        if promoted.isEmpty || ops.isEmpty {
            return nil
        }
        var touched: Set<Int> = []
        for op in ops {
            guard op.kind == .style, LiteHTMLWidgetRenderer.scriptedProperties.contains(op.name.lowercased()), promoted.contains(op.id) else {
                return nil
            }
            touched.insert(op.id)
        }
        return touched
    }

    private func noteScriptedTransforms(_ ops: [WidgetOp]) {
        opsBatch += 1
        var promote = false
        for op in ops {
            guard op.kind == .style, LiteHTMLWidgetRenderer.scriptedProperties.contains(op.name.lowercased()) else {
                continue
            }
            let id = op.id
            let hit = scriptedHits[id] ?? (0, 0)
            if hit.batch == opsBatch {
                continue
            }
            let count = hit.batch == opsBatch - 1 ? hit.count + 1 : 1
            scriptedHits[id] = (count, opsBatch)
            if count >= 3 && LiteHTMLWidgetRenderer.scriptedPromotion && !compositor.scriptedCandidates.contains(id) && compositor.scriptedCandidates.count < WidgetCompositor.maxLayers {
                compositor.scriptedCandidates.insert(id)
                promote = true
            }
        }
        if scriptedHits.count > 4096 {
            scriptedHits = scriptedHits.filter { opsBatch - $0.value.batch < 8 }
        }
        _ = promote
    }

    private var needsFrames: Bool {
        return WidgetDocumentView.verify ? animator.isActive : animator.isActive(excluding: compositor.drivenIDs)
    }

    public var drivenCount: Int {
        return compositor.drivenIDs.count
    }

    private var canDrive: Bool {
        return !frozen && isOnScreen && WidgetCompositor.handoffEnabled
    }

    private var drivenEndToken = 0

    private func scheduleDrivenEnds(now: CFTimeInterval) {
        let real = CACurrentMediaTime()
        var delays: [Double] = []
        for id in compositor.drivenIDs {
            let clock = animator.web.animations(id).isEmpty ? now : real
            if let end = animator.endTime(id), end > clock {
                delays.append(end - clock)
            }
        }
        if listened.contains("animationiteration") {
            delays += compositor.drivenIDs.compactMap { animator.nextIteration($0, after: now) }.map { $0 - now }
        }
        guard let next = delays.min() else {
            return
        }
        drivenEndToken += 1
        let token = drivenEndToken
        scheduleAfter(next + 0.002) { [weak self] in
            guard let self = self, self.drivenEndToken == token, !self.destroyed else {
                return
            }
            self.render()
        }
    }

    private func applyExclusions() {
        guard let document = document else {
            return
        }
        document.setPaintExclusions(Set(compositor.ids.union(magic.excluded).map { String($0) }), attribute: WidgetDOM.idAttribute)
        documentView.invalidate(nil)
    }

    private func finishMagicMove() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        magic.removeLayers()
        applyExclusions()
        documentView.displayIfNeeded()
        CATransaction.commit()
    }

    static let compositeChecking = ProcessInfo.processInfo.environment["TW_COMPOSITECHECK"] != nil
    public private(set) var compositeChecks = 0
    public private(set) var compositeWorst = 0.0

    private func compositeCheck(_ sampled: [Int: [(String, String)]]) {
        guard LiteHTMLWidgetRenderer.compositeChecking, !compositor.ids.isEmpty, let document = document else {
            return
        }
        let html = dom.serialize(theme: theme, overrides: sampled, canvasSource: { [unowned self] node in self.canvasSource(node) })
        let reference = LHDocument(html: html, css: nil)
        reference.controlsActive = controlsActive
        _ = reference.layout(maxWidth: width, preferredWidth: width)
        let size = CGSize(width: width, height: max(1, ceil(document.size.height)))
        let scale = documentView.scale
        let pw = Int(ceil(size.width * scale)), ph = Int(ceil(size.height * scale))
        func render(_ body: (CGContext) -> Void) -> CGImage? {
            guard let ctx = CGContext(data: nil, width: pw, height: ph, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
                return nil
            }
            ctx.translateBy(x: 0, y: CGFloat(ph))
            ctx.scaleBy(x: scale, y: -scale)
            ctx.setAllowsFontSmoothing(false)
            body(ctx)
            return ctx.makeImage()
        }
        guard let expected = render({ reference.draw(in: $0, origin: .zero, clip: CGRect(origin: .zero, size: size)) }), let root = documentView.contentLayer, let actual = render({ root.render(in: $0) }) else {
            return
        }
        let w = expected.width, h = expected.height
        guard actual.width == w, actual.height == h else {
            return
        }
        func pixels(_ image: CGImage) -> [UInt8] {
            var data = [UInt8](repeating: 0, count: w * h * 4)
            let ctx = CGContext(data: &data, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
            return data
        }
        let a = pixels(expected), b = pixels(actual)
        func distance(_ x: [UInt8], _ i: Int, _ y: [UInt8], _ j: Int) -> Int {
            return max(abs(Int(x[i]) - Int(y[j])), abs(Int(x[i + 1]) - Int(y[j + 1])), abs(Int(x[i + 2]) - Int(y[j + 2])), abs(Int(x[i + 3]) - Int(y[j + 3])))
        }
        func unexplained(_ x: [UInt8], _ y: [UInt8], _ row: Int, _ col: Int) -> Bool {
            let i = (row * w + col) * 4
            for dy in -1...1 {
                for dx in -1...1 {
                    let r = row + dy, c = col + dx
                    if r >= 0 && r < h && c >= 0 && c < w && distance(x, i, y, (r * w + c) * 4) <= 48 {
                        return false
                    }
                }
            }
            return true
        }
        var bad = 0
        for row in 0..<h {
            for col in 0..<w {
                let i = (row * w + col) * 4
                if distance(a, i, b, i) > 48 && (unexplained(a, b, row, col) || unexplained(b, a, row, col)) {
                    bad += 1
                }
            }
        }
        compositeChecks += 1
        let share = Double(bad) / Double(w * h) * 100
        if share > compositeWorst, let dir = ProcessInfo.processInfo.environment["TW_COMPOSITECHECK_DUMP"] {
            try? NSBitmapImageRep(cgImage: expected).representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: dir + "/expected.png"))
            try? NSBitmapImageRep(cgImage: actual).representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: dir + "/actual.png"))
        }
        compositeWorst = max(compositeWorst, share)
    }

    static let patchCheck = ProcessInfo.processInfo.environment["TW_PATCHCHECK"] != nil
    static let dirtyLog = ProcessInfo.processInfo.environment["TW_DIRTY_LOG"] != nil
    public private(set) var patchChecks = 0
    public private(set) var patchMismatches = 0
    public private(set) var patchBoundsMismatches = 0
    private var lastSerialized: String?

    private static func sameImage(_ a: CGImage?, _ b: CGImage?) -> Bool {
        guard let a = a, let b = b, a.width == b.width, a.height == b.height, let da = a.dataProvider?.data, let db = b.dataProvider?.data, CFDataGetLength(da) == CFDataGetLength(db) else {
            return a == nil && b == nil
        }
        let pa = CFDataGetBytePtr(da)!, pb = CFDataGetBytePtr(db)!
        for i in 0..<CFDataGetLength(da) where abs(Int(pa[i]) - Int(pb[i])) > 1 {
            return false
        }
        return true
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
        schedule { [weak self] in
            self?.post(["type": "event", "ev": ["type": type, "targetId": id]])
        }
    }

    private var reportedHeight = 0.0

    private func reportSize() {
        guard let document = document else {
            return
        }
        var message: [String: Any] = [
            "type": "size",
            "width": Double(width),
            "height": Double(ceil(document.size.height)),
            "innerWidth": Double(width),
            "innerHeight": Double(documentView.size.height),
            "dpr": Double(documentView.scale),
            "painted": documentView.paintedForReport,
        ]
        let key = "\(message["height"]!)|\(message["width"]!)|\(message["innerHeight"]!)|\(message["dpr"]!)|\(message["painted"]!)"
        let height = Double(ceil(document.size.height))
        if documentView.detached, height > reportedHeight, !documentView.tilesDrawn {
            return
        }
        if key != lastSize {
            if sizeTransition {
                message["transition"] = MagicMoveStage.timing
            }
            lastSize = key
            reportedHeight = height
            post(message)
        }
    }

    func documentViewScaleChanged() {
        documentView.scaleChanged()
    }

    func viewResized() {
        guard document != nil else {
            return
        }
        if magic.isActive {
            magic.cancel()
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

    func placeSurfaces() {
        guard !destroyed, surfaceLayer != nil || !placedSurfaces.isEmpty else {
            return
        }
        if surfaceRevision != WidgetDOM.revision {
            surfaceRevision = WidgetDOM.revision
            var tokens: [String] = []
            for node in dom.nodes.values where !node.isText {
                if let token = node.attribute(LiteHTMLWidgetRenderer.surfaceAttribute), !token.isEmpty, token.count <= 12, token.allSatisfy({ $0.isASCII && $0.isNumber }), !tokens.contains(token) {
                    tokens.append(token)
                    if tokens.count == LiteHTMLWidgetRenderer.maxSurfaces {
                        break
                    }
                }
            }
            surfaceTokens = tokens.sorted()
        }
        let shown = isOnScreen
        let active = shown && !frozen
        let scale = documentView.scale
        var rects: [String: CGRect] = [:]
        if let document = document, surfaceLayer != nil {
            for token in surfaceTokens {
                let rect = document.bounds(matching: "[\(LiteHTMLWidgetRenderer.surfaceAttribute)=\"\(token)\"]")
                if !rect.isNull, rect.width >= 1, rect.height >= 1 {
                    rects[token] = rect
                }
            }
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for (token, layer) in placedSurfaces where rects[token] == nil {
            layer.removeFromSuperlayer()
            placedSurfaces[token] = nil
            surfaceLayout?(token, .zero, scale, false)
        }
        if !rects.isEmpty {
            let container: CALayer
            if let existing = surfaceContainer {
                container = existing
            } else {
                container = CALayer()
                container.masksToBounds = true
                container.zPosition = 0.5
                container.actions = ["bounds": NSNull(), "position": NSNull(), "frame": NSNull(), "hidden": NSNull(), "sublayers": NSNull()]
                surfaceContainer = container
            }
            if container.superlayer !== documentView.contentLayer {
                documentView.contentLayer?.addSublayer(container)
            }
            container.frame = CGRect(origin: .zero, size: documentView.size)
            container.isHidden = !shown
            for (token, rect) in rects {
                var layer = placedSurfaces[token]
                if layer == nil, let made = surfaceLayer?(token) {
                    made.zPosition = 0
                    container.addSublayer(made)
                    placedSurfaces[token] = made
                    layer = made
                }
                guard let placed = layer else {
                    continue
                }
                if placed.frame != rect {
                    placed.frame = rect
                }
                placed.contentsScale = scale
                surfaceLayout?(token, rect.size, scale, active)
            }
        } else {
            surfaceContainer?.removeFromSuperlayer()
        }
        CATransaction.commit()
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

    private func call(_ node: WidgetNode, _ method: String, _ args: [Any] = []) {
        let id = dom.publicId(node)
        switch method {
        case "scrollTo", "scrollBy":
            guard scrolling.geometries[id] != nil, args.count >= 2, let left = (args[0] as? NSNumber)?.doubleValue, let top = (args[1] as? NSNumber)?.doubleValue, left.isFinite, top.isFinite else {
                return
            }
            let base = method == "scrollBy" ? scrolling.offset(id) : .zero
            scrollProgrammatically(id, CGPoint(x: base.x + CGFloat(left), y: base.y + CGFloat(top)))
        case "scrollIntoView":
            guard let box = elementBounds()[id] else {
                return
            }
            var walk = node.parent
            while let parent = walk {
                let scroller = dom.publicId(parent)
                if let geometry = scrolling.geometries[scroller] {
                    let offset = scrolling.offset(scroller)
                    scrollProgrammatically(scroller, CGPoint(x: offset.x, y: offset.y + box.minY - geometry.viewport.minY))
                    return
                }
                walk = parent.parent
            }
        case "observeResize":
            observedResize.insert(id)
            lastResize[id] = nil
            schedule { [weak self] in
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
        return ["innerWidth": Double(width), "innerHeight": Double(documentView.size.height), "dpr": Double(documentView.scale)]
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
            if let id = (q["id"] as? NSNumber)?.intValue, let geometry = scrolling.geometries[id] {
                let offset = scrolling.offset(id)
                var box: [String: Double] = ["offsetWidth": Double(r.width), "offsetHeight": Double(r.height), "offsetTop": Double(r.minY), "offsetLeft": Double(r.minX), "offsetParent": 0]
                box["clientWidth"] = Double(geometry.viewport.width)
                box["clientHeight"] = Double(geometry.viewport.height)
                box["clientTop"] = 0
                box["clientLeft"] = 0
                box["scrollWidth"] = Double(max(geometry.viewport.width, geometry.content.width))
                box["scrollHeight"] = Double(max(geometry.viewport.height, geometry.content.height))
                box["scrollTop"] = Double(offset.y)
                box["scrollLeft"] = Double(offset.x)
                return box
            }
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
        case "anims":
            guard let id = (q["id"] as? NSNumber)?.intValue, let root = dom.node(id) else {
                return ["list": []]
            }
            var ids = [id]
            if q["subtree"] as? Bool == true {
                var stack = Array(root.children.reversed())
                while let node = stack.popLast(), ids.count < 100_000 {
                    ids.append(dom.publicId(node))
                    stack.append(contentsOf: node.children.reversed())
                }
            }
            updateAnimationClock()
            return ["list": animator.cssAnimations(ids, now: animationNow)]
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

    private func measureText(_ full: String, font: String) -> [String: Any] {
        let text = String(full.prefix(4096))
        let ctFont = CSSFont.make(String(font.prefix(512)))
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
        let point = compositor.documentPoint(for: point) ?? point
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
        if magic.isActive {
            magic.cancel()
            applyExclusions()
        }
        clock?.stop()
        frameRequested = false
        pendingMoves.removeAll()
        if Thread.isMainThread {
            documentView.showBadge(reason, fatal: fatal)
        } else {
            DispatchQueue.main.async { [weak documentView] in
                documentView?.showBadge(reason, fatal: fatal)
            }
        }
        placeSurfaces()
    }

    func keyChanged() {
        let active = documentView.windowState.key
        guard active != controlsActive else {
            return
        }
        controlsActive = active
        document?.controlsActive = active
        documentView.invalidate(nil)
        setNeedsRender()
    }

    func screenChanged() {
        guard let number = documentView.windowState.display else {
            return
        }
        display = number
        if let display = display {
            clock?.setDisplay(display)
        }
        reportSize()
        placeSurfaces()
    }

    private var display: CGDirectDisplayID?

    private var hostVisible = true

    var isHostVisible: Bool {
        return hostVisible
    }

    public static var hiddenTrimDelay: TimeInterval = 2

    public static var scriptFrameBudget: TimeInterval = ScriptFramePacer.defaultBudget
    public var scriptFrameBudget: TimeInterval? {
        didSet {
            pacer.budget = scriptFrameBudget ?? LiteHTMLWidgetRenderer.scriptFrameBudget
        }
    }
    public var scriptFrameCost: TimeInterval {
        return pacer.cost
    }
    public private(set) var scriptFramesPaced = 0
    private lazy var pacer = ScriptFramePacer(budget: LiteHTMLWidgetRenderer.scriptFrameBudget, fallbackInterval: LiteHTMLWidgetRenderer.frameInterval)

    private var trimWork: DispatchWorkItem?

    private var keepsTilesValue = false

    public var keepsTilesWhileHidden: Bool {
        get {
            return keepsTilesValue
        }
        set {
            guard isOnExecutor() else {
                return scheduleUrgent { [weak self] in self?.keepsTilesWhileHidden = newValue }
            }
            if keepsTilesValue != newValue {
                keepsTilesValue = newValue
                updateTileTrim()
            }
        }
    }

    private func updateTileTrim() {
        if isOnScreen || destroyed || keepsTilesWhileHidden || preparesOffscreen {
            trimWork?.cancel()
            trimWork = nil
            if !destroyed {
                documentView.restoreTiles()
            }
            return
        }
        guard trimWork == nil else {
            return
        }
        let work = DispatchWorkItem { [weak self] in
            guard let self = self else {
                return
            }
            self.trimWork = nil
            if !self.destroyed && !self.isOnScreen && !self.keepsTilesWhileHidden && !self.preparesOffscreen {
                self.documentView.trimTiles()
            }
        }
        trimWork = work
        scheduleAfter(LiteHTMLWidgetRenderer.hiddenTrimDelay) {
            if !work.isCancelled {
                work.perform()
            }
        }
    }

    public func setVisible(_ visible: Bool) {
        guard isOnExecutor() else {
            return scheduleUrgent { [weak self] in self?.setVisible(visible) }
        }
        hostVisible = visible
        updateAnimationClock()
        updateTileTrim()
        schedulePrepare()
        placeSurfaces()
        if !isOnScreen {
            compositor.undrive()
        } else if !compositor.ids.isEmpty && animator.isActive && !frozen {
            setNeedsRender()
        }
        if isOnScreen && (needsFrames || frameRequested || !pendingMoves.isEmpty) && !frozen {
            if needsFrames {
                renderAnimationFrame()
            }
            startClock()
        }
        if isOnScreen && documentView.detached {
            documentView.scheduleFlush()
        }
    }

    private var isOnScreen: Bool {
        guard hostVisible else {
            return false
        }
        let state = documentView.windowState
        guard state.inWindow, !state.hidden else {
            return false
        }
        return state.occlusionTrusted ? state.occlusionVisible : state.windowVisible
    }

    private func startClock() {
        if clock == nil {
            clock = WidgetFrameClock(post: { [weak self] block in self?.schedule(block) }) { [weak self] in
                self?.tick()
            }
            if let display = display {
                clock?.setDisplay(display)
            }
        }
        clock?.start()
    }

    private func tick() {
        updateAnimationClock()
        if !isOnScreen {
            clock?.stop()
            return
        }
        if !pendingMoves.isEmpty {
            let moves = pendingMoves.values
            pendingMoves.removeAll()
            for event in moves {
                post(["type": "event", "ev": event])
            }
        }
        if needsFrames && !frozen && isOnScreen {
            renderAnimationFrame()
        }
        var worked = needsFrames
        if frameRequested {
            worked = true
            let now = CACurrentMediaTime()
            if pacer.due(now) {
                frameRequested = false
                lastFramePost = now
                pacer.posted(now)
                post(["type": "frame", "time": (now - timeOrigin) * 1000])
            } else {
                scriptFramesPaced += 1
            }
        }
        idleTicks = worked ? 0 : idleTicks + 1
        if !frameRequested && pendingMoves.isEmpty && !needsFrames && idleTicks > 8 {
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

    func handleScroll(_ point: CGPoint, event: NSEvent) -> Bool {
        if frozen {
            return false
        }
        let outcome = scrolling.scroll(event: event, at: point, document: document, scale: documentView.scale, attribute: WidgetDOM.idAttribute)
        switch outcome {
        case .ignored:
            return false
        case .unchanged:
            return true
        case let .moved(id):
            scrolled(id)
            return true
        }
    }

    private func scrolled(_ id: Int) {
        boundsValid = false
        if scrolling.layers[id] == nil, let geometry = scrolling.geometries[id] {
            documentView.invalidate(geometry.viewport.insetBy(dx: -1, dy: -1))
        }
        if listened.contains("scroll") && !frozen {
            pendingMoves["scroll:\(id)"] = ["type": "scroll", "targetId": id, "bubbles": false]
            startClock()
        }
    }

    private func setProperty(_ node: WidgetNode, _ name: String, _ value: Any) {
        guard name == "scrollTop" || name == "scrollLeft", let number = (value as? NSNumber)?.doubleValue, number.isFinite else {
            return
        }
        let id = dom.publicId(node)
        guard scrolling.geometries[id] != nil else {
            return
        }
        var offset = scrolling.offset(id)
        if name == "scrollTop" {
            offset.y = CGFloat(number)
        } else {
            offset.x = CGFloat(number)
        }
        scrollProgrammatically(id, offset)
    }

    private func scrollProgrammatically(_ id: Int, _ offset: CGPoint) {
        if scrolling.setOffset(id, offset, document: document) {
            if let layer = scrolling.layers[id], let document = document {
                layer.ensureTiles(document: document, scale: documentView.scale, attribute: WidgetDOM.idAttribute)
            }
            scrolled(id)
        }
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
        layerUsesCoreImageFilters = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        lockedWindowState.size = frame.size
        drawer.view = self
        content.anchorPoint = .zero
        content.frame = CGRect(origin: .zero, size: frame.size)
        content.autoresizingMask = [.layerWidthSizable, .layerHeightSizable]
        content.actions = ["bounds": NSNull(), "position": NSNull(), "contents": NSNull(), "sublayers": NSNull()]
        layer?.addSublayer(content)
    }

    private let content = CALayer()

    var contentLayer: CALayer? {
        return content
    }

    required init?(coder: NSCoder) {
        fatalError()
    }

    override func setFrameSize(_ newSize: NSSize) {
        let widthChanged = newSize.width != frame.width
        let heightChanged = newSize.height != frame.height
        super.setFrameSize(newSize)
        captureSize(newSize)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        content.frame = CGRect(origin: .zero, size: newSize)
        CATransaction.commit()
        if widthChanged {
            toOwner { $0.viewResized() }
        } else if heightChanged {
            toOwner { $0.placeSurfaces() }
        }
    }

    private(set) var drawMilliseconds = 0.0
    private(set) var drawCount = 0
    private(set) var drawMaxMilliseconds = 0.0
    private final class Surface {
        let surface: IOSurfaceRef
        let context: CGContext
        let width: Int
        let height: Int
        var stale = CGRect.null

        init?(width: Int, height: Int, space: CGColorSpace) {
            guard let surface = WidgetSurfacePool.shared.take(width: width, height: height) ?? WidgetSurfacePool.make(width: width, height: height) else {
                return nil
            }
            if let plist = space.copyPropertyList() {
                IOSurfaceSetValue(surface, kIOSurfaceColorSpace, plist)
            }
            IOSurfaceLock(surface, [], nil)
            let base = IOSurfaceGetBaseAddress(surface)
            let context = CGContext(data: base, width: width, height: height, bitsPerComponent: 8, bytesPerRow: IOSurfaceGetBytesPerRow(surface), space: space, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
            IOSurfaceUnlock(surface, [], nil)
            guard let made = context else {
                return nil
            }
            self.surface = surface
            self.context = made
            self.width = width
            self.height = height
        }
    }

    static let cullMargin: CGFloat = 32
    static let recycleSurfaces = ProcessInfo.processInfo.environment["TW_NO_SURFACE_RECYCLE"] == nil
    static let surfaceSpace = CGColorSpace(name: CGColorSpace.sRGB)!
    static let screenTagged = ProcessInfo.processInfo.environment["TW_SURFACE_SCREEN"] != nil
    static var verify = ProcessInfo.processInfo.environment["TW_VERIFY"] != nil
    private(set) var verifiedFrames = 0
    private(set) var mismatchedFrames = 0

    private func verifyFrame(_ surface: Surface, document: LHDocument, rect: CGRect, scale: CGFloat, space: CGColorSpace) {
        guard let fresh = document.copyImage(maxWidth: bounds.width, rect: rect, scale: scale), fresh.width == surface.width, fresh.height == surface.height,
              let context = CGContext(data: nil, width: surface.width, height: surface.height, bitsPerComponent: 8, bytesPerRow: surface.width * 4, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue) else {
            return
        }
        context.setBlendMode(.copy)
        context.draw(fresh, in: CGRect(x: 0, y: 0, width: surface.width, height: surface.height))
        IOSurfaceLock(surface.surface, .readOnly, nil)
        defer { IOSurfaceUnlock(surface.surface, .readOnly, nil) }
        guard let a = context.data, let b = IOSurfaceGetBaseAddress(surface.surface) as UnsafeMutableRawPointer? else {
            return
        }
        let rowA = context.bytesPerRow, rowB = IOSurfaceGetBytesPerRow(surface.surface)
        var worst = 0
        var worstAt = (0, 0)
        for y in 0..<surface.height {
            let pa = a.advanced(by: y * rowA).assumingMemoryBound(to: UInt8.self)
            let pb = b.advanced(by: y * rowB).assumingMemoryBound(to: UInt8.self)
            for x in 0..<(surface.width * 4) {
                let d = abs(Int(pa[x]) - Int(pb[x]))
                if d > worst {
                    worst = d
                    worstAt = (x / 4, y)
                }
            }
        }
        if worst > 1, ProcessInfo.processInfo.environment["TW_VERIFY_LOG"] != nil {
            print("VERIFYDBG rect=\(rect) worst=\(worst) at=\(worstAt)")
        }
        verifiedFrames += 1
        let tiled = rect.width < bounds.width - 0.5 || rect.height < max(1, ceil(document.size.height)) - 0.5
        if worst > (tiled ? 2 : 1) {
            mismatchedFrames += 1
        }
    }

    private final class TileDrawer: NSObject, CALayerDelegate {
        weak var view: WidgetDocumentView?

        func draw(_ layer: CALayer, in ctx: CGContext) {
            guard let view = view, let document = view.document else {
                return
            }
            let start = CACurrentMediaTime()
            let clip = ctx.boundingBoxOfClipPath
            let origin = layer.frame.origin
            ctx.saveGState()
            ctx.translateBy(x: -origin.x, y: -origin.y)
            ctx.setAllowsFontSmoothing(false)
            ctx.setShouldAntialias(true)
            let dirty = clip.offsetBy(dx: origin.x, dy: origin.y)
            document.draw(in: ctx, origin: .zero, clip: dirty.insetBy(dx: -WidgetDocumentView.cullMargin, dy: -WidgetDocumentView.cullMargin))
            ctx.restoreGState()
            let spent = (CACurrentMediaTime() - start) * 1000
            view.drawMilliseconds += spent
            view.drawCount += 1
            view.drawMaxMilliseconds = max(view.drawMaxMilliseconds, spent)
        }

        func action(for layer: CALayer, forKey event: String) -> CAAction? {
            return NSNull()
        }
    }

    private let drawer = TileDrawer()

    private final class PreparedDisplay: NSObject, CALayerDelegate {
        weak var drawer: TileDrawer?

        func display(_ layer: CALayer) {
            if drawer?.view?.detached ?? false {
                return
            }
            layer.delegate = drawer
        }

        func action(for layer: CALayer, forKey event: String) -> CAAction? {
            return NSNull()
        }
    }

    private lazy var preparedDisplay: PreparedDisplay = {
        let display = PreparedDisplay()
        display.drawer = drawer
        return display
    }()

    static let prepareQueue = DispatchQueue(label: "twidget.tiles.prepare", qos: .userInitiated, attributes: .concurrent)
    private(set) var preparing = false
    private(set) var preparedTiles = 0
    private(set) var preparedMilliseconds = 0.0
    private var invalidations = 0
    private var syncInvalidations = 0
    private var tileScale: CGFloat = 0

    private static func surface(of layer: CALayer) -> IOSurfaceRef? {
        guard let contents = layer.contents else {
            return nil
        }
        let object = contents as CFTypeRef
        guard CFGetTypeID(object) == IOSurfaceGetTypeID() else {
            return nil
        }
        return (object as! IOSurfaceRef)
    }

    var hasUnpreparedTiles: Bool {
        return tilesTrimmed || tiles.isEmpty || tiles.contains { !$0.prepared && ($0.layer.contents == nil || $0.layer.needsDisplay()) }
    }

    func prepareTiles(completion: @escaping () -> Void) -> Bool {
        guard WidgetDocumentView.layerTiles, !WidgetDocumentView.backingStore, !preparing, let layer = rootLayer, let document = document else {
            return false
        }
        tilesTrimmed = false
        let scale = self.scale
        let detached = self.detached
        guard syncTiles(layer: layer, document: document, scale: scale) else {
            return false
        }
        var jobs: [(index: Int, rect: CGRect)] = []
        for (index, tile) in tiles.enumerated() where !tile.prepared && (detached || tile.layer.contents == nil || tile.layer.needsDisplay()) {
            jobs.append((index, tile.rect))
        }
        if jobs.isEmpty {
            return false
        }
        if detached {
            for job in jobs {
                asyncDirty[job.index] = nil
            }
        }
        preparing = true
        let epoch = invalidations
        let syncEpoch = syncInvalidations
        let space = WidgetDocumentView.surfaceSpace
        let margin = WidgetDocumentView.cullMargin
        let schedule = owner?.scheduleUrgent
        WidgetDocumentView.prepareQueue.async { [weak self] in
            let start = CACurrentMediaTime()
            var made: [(index: Int, rect: CGRect, surface: IOSurfaceRef)] = []
            for job in jobs {
                let rect = job.rect
                let width = max(1, Int(ceil(rect.maxX * scale)) - Int(floor(rect.minX * scale)))
                let height = max(1, Int(ceil(rect.height * scale)))
                guard let surface = Surface(width: width, height: height, space: space) else {
                    made.removeAll()
                    break
                }
                let ctx = surface.context
                IOSurfaceLock(surface.surface, [], nil)
                ctx.saveGState()
                ctx.clear(CGRect(x: 0, y: 0, width: width, height: height))
                ctx.translateBy(x: 0, y: CGFloat(height))
                ctx.scaleBy(x: scale, y: -scale)
                ctx.translateBy(x: -rect.minX, y: -rect.minY)
                ctx.setAllowsFontSmoothing(false)
                ctx.setShouldAntialias(true)
                let drawn = document.drawIfCurrent(in: ctx, origin: .zero, clip: rect.insetBy(dx: -margin, dy: -margin))
                ctx.restoreGState()
                ctx.flush()
                IOSurfaceUnlock(surface.surface, [], nil)
                guard drawn else {
                    made.removeAll()
                    break
                }
                made.append((job.index, rect, surface.surface))
            }
            let spent = (CACurrentMediaTime() - start) * 1000
            let finish = {
                guard let self = self else {
                    return
                }
                self.preparing = false
                self.preparedMilliseconds += spent
                if self.document === document && ((detached && self.detached) || (self.invalidations == epoch && self.syncInvalidations == syncEpoch)) && !self.tilesTrimmed && self.tileScale == scale {
                    CATransaction.begin()
                    CATransaction.setDisableActions(true)
                    for item in made where item.index < self.tiles.count {
                        let tile = self.tiles[item.index]
                        guard tile.rect == item.rect, !tile.prepared else {
                            continue
                        }
                        if WidgetDocumentView.verifyPrepared {
                            WidgetDocumentView.checkPrepared(item.surface, document: document, rect: item.rect, width: self.size.width, scale: scale)
                        }
                        let previous = WidgetDocumentView.surface(of: tile.layer)
                        tile.layer.contents = item.surface
                        tile.layer.delegate = self.preparedDisplay
                        if let previous = previous, WidgetDocumentView.recycleSurfaces {
                            WidgetSurfacePool.shared.recycle(previous)
                        }
                        tile.prepared = true
                        self.preparedTiles += 1
                    }
                    CATransaction.commit()
                }
                completion()
                if detached && self.detached {
                    self.scheduleFlush()
                }
            }
            if let schedule = schedule {
                schedule(finish)
            } else {
                DispatchQueue.main.async(execute: finish)
            }
        }
        return true
    }

    static let verifyPrepared = ProcessInfo.processInfo.environment["TW_VERIFY_PREPARE"] != nil
    static var preparedChecks = 0
    static var preparedWorstDiff = 0

    private static func checkPrepared(_ surface: IOSurfaceRef, document: LHDocument, rect: CGRect, width: CGFloat, scale: CGFloat, label: String = "prepare") {
        let w = IOSurfaceGetWidth(surface), h = IOSurfaceGetHeight(surface)
        guard let context = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: surfaceSpace, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue) else {
            return
        }
        context.saveGState()
        context.translateBy(x: 0, y: CGFloat(h))
        context.scaleBy(x: scale, y: -scale)
        context.translateBy(x: -rect.minX, y: -rect.minY)
        context.setAllowsFontSmoothing(false)
        context.setShouldAntialias(true)
        document.draw(in: context, origin: .zero, clip: rect.insetBy(dx: -cullMargin, dy: -cullMargin))
        context.restoreGState()
        guard let fresh = context.makeImage() else {
            return
        }
        IOSurfaceLock(surface, .readOnly, nil)
        defer { IOSurfaceUnlock(surface, .readOnly, nil) }
        guard let a = context.data, let b = IOSurfaceGetBaseAddress(surface) as UnsafeMutableRawPointer? else {
            return
        }
        let rowA = context.bytesPerRow, rowB = IOSurfaceGetBytesPerRow(surface)
        var worst = 0
        var box = (x0: Int.max, y0: Int.max, x1: -1, y1: -1)
        for y in 0..<h {
            let pa = a.advanced(by: y * rowA).assumingMemoryBound(to: UInt8.self)
            let pb = b.advanced(by: y * rowB).assumingMemoryBound(to: UInt8.self)
            for x in 0..<(w * 4) {
                let d = abs(Int(pa[x]) - Int(pb[x]))
                worst = max(worst, d)
                if d > 2 {
                    box = (min(box.x0, x / 4), min(box.y0, y), max(box.x1, x / 4), max(box.y1, y))
                }
            }
        }
        if worst > 40, ProcessInfo.processInfo.environment["TW_VERIFY_PREPARE_DUMP"] != nil {
            print("PREPARE DIFF box=\(box) of \(w)x\(h)")
        }
        preparedChecks += 1
        preparedWorstDiff = max(preparedWorstDiff, worst)
        if worst > 2 {
            preparedMismatches += 1
            mismatchByLabel[label + (worst > 16 ? ">16" : "<=16"), default: 0] += 1
            if worst > 40 { bigMismatches += 1 }
            if worst > 40, bigMismatches <= 4, let dir = ProcessInfo.processInfo.environment["TW_VERIFY_PREPARE_DUMP"] {
                let rep = NSBitmapImageRep(cgImage: fresh)
                try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: dir + "/fresh\(bigMismatches).png"))
                if let mine = context.makeImage() {
                    _ = mine
                }
                let surfaceImage = CIImage(ioSurface: surface)
                let ci = CIContext()
                if let cg = ci.createCGImage(surfaceImage, from: surfaceImage.extent) {
                    try? NSBitmapImageRep(cgImage: cg).representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: dir + "/surface\(bigMismatches).png"))
                }
                print("PREPARE MISMATCH \(bigMismatches) rect=\(rect) worst=\(worst) label=\(label)")
            }
        }
    }
    static var preparedMismatches = 0
    static var bigMismatches = 0
    static var mismatchByLabel: [String: Int] = [:]

    public static var preparedReport: String {
        return "prepared checks=\(preparedChecks) worst diff=\(preparedWorstDiff) mismatches=\(preparedMismatches) big=\(bigMismatches) \(mismatchByLabel)"
    }

    private func unprepare(_ tile: Tile) {
        syncInvalidations += 1
        tile.prepared = false
        if detached {
            return
        }
        tile.layer.delegate = drawer
        tile.layer.setNeedsDisplay()
    }

    private var rootLayer: CALayer? {
        return contentLayer
    }
    private var flushScheduled = false

    func scheduleFlush() {
        guard !flushScheduled, let owner = owner else {
            return
        }
        flushScheduled = true
        owner.scheduleUrgent { [weak self] in
            guard let self = self else {
                return
            }
            self.flushScheduled = false
            self.flushDetached()
        }
    }

    var flushDebug: String {
        return "flushScheduled=\(flushScheduled) gridFails=\(gridFails) size=\(size) scale=\(scale)"
    }
    private var gridFails = 0

    func flushIfDetached() {
        if detached {
            flushDetached()
        }
    }

    private(set) var everPainted = false

    var tilesDrawn: Bool {
        guard let document = document, !tiles.isEmpty, !tilesTrimmed else {
            return false
        }
        let covered = tiles.reduce(CGFloat(0)) { max($0, $1.rect.maxY) }
        return covered >= ceil(document.size.height) - 0.5 && !tiles.contains(where: { $0.layer.contents == nil })
    }

    var paintedForReport: Bool {
        if !detached || everPainted {
            return true
        }
        if document != nil, !tiles.isEmpty, !tiles.contains(where: { $0.layer.contents == nil }) {
            everPainted = true
        }
        return everPainted
    }

    private func flushDetached() {
        guard detached, document != nil, !tilesTrimmed, windowState.inWindow || owner?.preparesOffscreen == true || !everPainted else {
            return
        }
        if preparing {
            return
        }
        let started = prepareTiles { [weak self] in
            self?.owner?.rasterFinished()
        }
        if !started && !asyncDirty.isEmpty {
            rasterDirtyTiles()
        }
    }

    private func invalidateDetached(_ rects: [CGRect]?) {
        invalidations += 1
        for (index, tile) in tiles.enumerated() where tile.prepared {
            if let rects = rects {
                for rect in rects where rect.intersects(tile.rect) {
                    asyncDirty[index] = (asyncDirty[index] ?? .null).union(rect.intersection(tile.rect))
                }
            } else {
                asyncDirty[index] = tile.rect
            }
        }
        scheduleFlush()
    }

    func insertOverlay(_ overlay: CALayer) {
        rootLayer?.insertSublayer(overlay, at: UInt32(tiles.count))
    }
    static var layerTiles = ProcessInfo.processInfo.environment["TW_SURFACE_TILES"] == nil && ProcessInfo.processInfo.environment["TW_VERIFY"] == nil

    private final class Tile {
        let layer = CALayer()
        var surfaces: [Surface] = []
        var shown: Surface?
        var rect = CGRect.null
        var pending = true
        var prepared = false
    }

    private var tiles: [Tile] = []
    private var surfaceSpace: CGColorSpace?
    private var surfaceWidth = 0
    private var badgeLayer: CATextLayer?

    static let backingStore = ProcessInfo.processInfo.environment["TW_BACKING"] != nil
    static let eightBitTiles = ProcessInfo.processInfo.environment["TW_DEEP_TILES"] == nil
    static let tileHeight: CGFloat = ProcessInfo.processInfo.environment["TW_TILE"].flatMap { Double($0) }.map { CGFloat($0) } ?? 128

    override var wantsUpdateLayer: Bool {
        return !WidgetDocumentView.backingStore
    }

    override func draw(_ dirtyRect: NSRect) {
        guard WidgetDocumentView.backingStore, let document = document, let ctx = NSGraphicsContext.current?.cgContext else {
            return
        }
        let start = CACurrentMediaTime()
        defer {
            drawCount += 1
            let spent = (CACurrentMediaTime() - start) * 1000
            drawMilliseconds += spent
            drawMaxMilliseconds = max(drawMaxMilliseconds, spent)
        }
        ctx.saveGState()
        ctx.setAllowsFontSmoothing(false)
        ctx.clear(dirtyRect)
        document.draw(in: ctx, origin: .zero, clip: dirtyRect.insetBy(dx: -WidgetDocumentView.cullMargin, dy: -WidgetDocumentView.cullMargin))
        ctx.restoreGState()
    }

    static let asyncFullTiles = ProcessInfo.processInfo.environment["TW_ASYNC_FULL"] != nil
    static var asyncTiles = ProcessInfo.processInfo.environment["TW_SYNC_TILES"] == nil
    var asyncAllowed = false
    private var asyncDirty: [Int: CGRect] = [:]
    private(set) var asyncTileJobs = 0
    private(set) var asyncTilesInstalled = 0

    private var canInvalidateAsync: Bool {
        return asyncAllowed && WidgetDocumentView.asyncTiles && WidgetDocumentView.layerTiles && !WidgetDocumentView.backingStore && windowState.inWindow && !tilesTrimmed && !tiles.isEmpty && tileScale == scale
    }

    private func invalidateAsync(_ rects: [CGRect]) {
        invalidations += 1
        markNeedsDisplay()
        for rect in rects {
            for (index, tile) in tiles.enumerated() where rect.intersects(tile.rect) {
                asyncDirty[index] = (asyncDirty[index] ?? .null).union(rect.intersection(tile.rect))
            }
        }
        if !asyncDirty.isEmpty {
            owner?.schedule { [weak self] in
                self?.rasterDirtyTiles()
            }
        }
    }

    private func rasterDirtyTiles() {
        guard !preparing, !asyncDirty.isEmpty, let document = document, windowState.inWindow, !tilesTrimmed else {
            return
        }
        let scale = self.scale
        var jobs: [(index: Int, rect: CGRect, dirty: CGRect, base: IOSurfaceRef?)] = []
        for (index, dirty) in asyncDirty where index < tiles.count {
            let tile = tiles[index]
            if detached ? !tile.prepared : tile.layer.needsDisplay() {
                continue
            }
            let base = tile.prepared && !WidgetDocumentView.asyncFullTiles ? WidgetDocumentView.surface(of: tile.layer) : nil
            jobs.append((index, tile.rect, base == nil ? tile.rect : dirty, base))
        }
        let pending = asyncDirty
        asyncDirty.removeAll()
        if jobs.isEmpty {
            return
        }
        preparing = true
        asyncTileJobs += 1
        let syncEpoch = syncInvalidations
        let space = WidgetDocumentView.surfaceSpace
        let margin = WidgetDocumentView.cullMargin
        let schedule = owner?.scheduleUrgent
        WidgetDocumentView.prepareQueue.async { [weak self] in
            var made: [(index: Int, rect: CGRect, surface: IOSurfaceRef)] = []
            var aborted = false
            var stats = WidgetTileStats.Snapshot()
            stats.batches = 1
            for job in jobs {
                var mark = WidgetTileStats.threadMilliseconds()
                let rect = job.rect
                let width = max(1, Int(ceil(rect.maxX * scale)) - Int(floor(rect.minX * scale)))
                let height = max(1, Int(ceil(rect.height * scale)))
                guard let surface = Surface(width: width, height: height, space: space) else {
                    aborted = true
                    break
                }
                let ctx = surface.context
                IOSurfaceLock(surface.surface, [], nil)
                var dirty = job.dirty.intersection(rect)
                var now = WidgetTileStats.threadMilliseconds()
                stats.setup += now - mark
                mark = now
                if let base = job.base, IOSurfaceGetWidth(base) == width, IOSurfaceGetHeight(base) == height {
                    IOSurfaceLock(base, .readOnly, nil)
                    let from = IOSurfaceGetBaseAddress(base), to = IOSurfaceGetBaseAddress(surface.surface)
                    let fromRow = IOSurfaceGetBytesPerRow(base), toRow = IOSurfaceGetBytesPerRow(surface.surface)
                    for y in 0..<height {
                        memcpy(to.advanced(by: y * toRow), from.advanced(by: y * fromRow), width * 4)
                    }
                    IOSurfaceUnlock(base, .readOnly, nil)
                } else {
                    dirty = rect
                    stats.full += 1
                }
                now = WidgetTileStats.threadMilliseconds()
                stats.copy += now - mark
                mark = now
                stats.jobs += 1
                stats.area += Double(dirty.width * dirty.height)
                let x0 = floor(dirty.minX * scale) / scale, y0 = floor(dirty.minY * scale) / scale
                let x1 = ceil(dirty.maxX * scale) / scale, y1 = ceil(dirty.maxY * scale) / scale
                let aligned = CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
                ctx.saveGState()
                ctx.translateBy(x: 0, y: CGFloat(height))
                ctx.scaleBy(x: scale, y: -scale)
                ctx.translateBy(x: -rect.minX, y: -rect.minY)
                ctx.clip(to: aligned)
                ctx.clear(aligned)
                ctx.setAllowsFontSmoothing(false)
                ctx.setShouldAntialias(true)
                let drawn = document.drawIfCurrent(in: ctx, origin: .zero, clip: aligned.insetBy(dx: -margin, dy: -margin))
                ctx.restoreGState()
                ctx.flush()
                IOSurfaceUnlock(surface.surface, [], nil)
                stats.draw += WidgetTileStats.threadMilliseconds() - mark
                guard drawn else {
                    aborted = true
                    break
                }
                made.append((job.index, rect, surface.surface))
            }
            WidgetTileStats.add(stats)
            let finish = {
                guard let self = self else {
                    return
                }
                self.preparing = false
                if aborted || self.syncInvalidations != syncEpoch {
                    for (index, dirty) in pending {
                        self.asyncDirty[index] = (self.asyncDirty[index] ?? .null).union(dirty)
                    }
                } else if self.windowState.inWindow && !self.tilesTrimmed && self.tileScale == scale {
                    CATransaction.begin()
                    CATransaction.setDisableActions(true)
                    for item in made where item.index < self.tiles.count {
                        let tile = self.tiles[item.index]
                        guard tile.rect == item.rect, self.detached ? tile.prepared : !tile.layer.needsDisplay() else {
                            continue
                        }
                        if WidgetDocumentView.verifyPrepared, self.document === document {
                            WidgetDocumentView.checkPrepared(item.surface, document: document, rect: item.rect, width: self.size.width, scale: scale, label: "async")
                        }
                        let previous = WidgetDocumentView.surface(of: tile.layer)
                        tile.layer.contents = item.surface
                        tile.layer.delegate = self.preparedDisplay
                        if let previous = previous, WidgetDocumentView.recycleSurfaces {
                            WidgetSurfacePool.shared.recycle(previous)
                        }
                        tile.prepared = true
                        self.asyncTilesInstalled += 1
                    }
                    CATransaction.commit()
                }
                self.owner?.rasterFinished()
                if self.detached {
                    self.scheduleFlush()
                } else if !self.asyncDirty.isEmpty {
                    self.rasterDirtyTiles()
                }
            }
            if let schedule = schedule {
                schedule(finish)
            } else {
                DispatchQueue.main.async(execute: finish)
            }
        }
    }

    func invalidate(_ rects: [CGRect]) {
        if detached {
            invalidateDetached(rects)
            return
        }
        if canInvalidateAsync {
            invalidateAsync(rects)
            return
        }
        syncInvalidations += 1
        if rects.count <= 32 || WidgetDocumentView.backingStore || tiles.isEmpty {
            for rect in rects {
                invalidate(rect)
            }
            return
        }
        var pieces = [CGRect](repeating: .null, count: tiles.count)
        var all = CGRect.null
        for rect in rects {
            all = all.union(rect)
            for (index, tile) in tiles.enumerated() where rect.intersects(tile.rect) {
                pieces[index] = pieces[index].union(rect.intersection(tile.rect))
            }
        }
        invalidations += 1
        for (index, tile) in tiles.enumerated() where !pieces[index].isNull {
            if WidgetDocumentView.layerTiles {
                if tile.prepared {
                    unprepare(tile)
                    continue
                }
                tile.layer.setNeedsDisplay(pieces[index].offsetBy(dx: -tile.rect.minX, dy: -tile.rect.minY))
            } else {
                tile.pending = true
            }
        }
        if !WidgetDocumentView.layerTiles {
            for tile in tiles {
                for surface in tile.surfaces {
                    surface.stale = surface.stale.union(all)
                }
            }
        }
        markNeedsDisplay()
    }

    func invalidate(_ rect: CGRect?) {
        if detached {
            invalidateDetached(rect.map { [$0] })
            return
        }
        if canInvalidateAsync {
            invalidateAsync([rect ?? CGRect(x: 0, y: 0, width: size.width, height: CGFloat.greatestFiniteMagnitude / 4)])
            return
        }
        syncInvalidations += 1
        if WidgetDocumentView.backingStore {
            if let rect = rect {
                setNeedsDisplay(rect)
            } else {
                markNeedsDisplay()
            }
            return
        }
        let everything = CGRect(x: 0, y: 0, width: CGFloat.greatestFiniteMagnitude / 4, height: CGFloat.greatestFiniteMagnitude / 4)
        invalidations += 1
        if WidgetDocumentView.layerTiles {
            for tile in tiles where rect.map({ $0.intersects(tile.rect) }) ?? true {
                if tile.prepared {
                    unprepare(tile)
                } else if let rect = rect {
                    tile.layer.setNeedsDisplay(rect.intersection(tile.rect).offsetBy(dx: -tile.rect.minX, dy: -tile.rect.minY))
                } else {
                    tile.layer.setNeedsDisplay()
                }
            }
            markNeedsDisplay()
            return
        }
        for tile in tiles {
            if rect.map({ $0.intersects(tile.rect) }) ?? true {
                tile.pending = true
            }
            for surface in tile.surfaces {
                surface.stale = rect.map { surface.stale.union($0) } ?? everything
            }
        }
        markNeedsDisplay()
    }

    private static let screenScaleLock = NSLock()
    private static var screenScaleValue: CGFloat = 2
    private let scaleLock = NSLock()
    private var windowScale: CGFloat?

    private static var screenScale: CGFloat {
        get {
            screenScaleLock.lock()
            defer { screenScaleLock.unlock() }
            return screenScaleValue
        }
        set {
            screenScaleLock.lock()
            screenScaleValue = newValue
            screenScaleLock.unlock()
        }
    }

    private func refreshWindowScale() {
        let value = window?.backingScaleFactor
        scaleLock.lock()
        windowScale = value
        scaleLock.unlock()
        if let main = NSScreen.main {
            WidgetDocumentView.screenScale = main.backingScaleFactor
        }
    }

    var scale: CGFloat {
        if Thread.isMainThread {
            if let window = window {
                return window.backingScaleFactor
            }
            if let main = NSScreen.main {
                WidgetDocumentView.screenScale = main.backingScaleFactor
            }
            return WidgetDocumentView.screenScale
        }
        scaleLock.lock()
        let cached = windowScale
        scaleLock.unlock()
        return cached ?? WidgetDocumentView.screenScale
    }

    private var displayPending = false

    private func markNeedsDisplay() {
        if detached {
            scheduleFlush()
            return
        }
        if Thread.isMainThread {
            needsDisplay = true
        } else {
            displayPending = true
        }
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        refreshWindowScale()
        captureWindowState()
        if detached {
            toOwner { owner in
                owner.documentViewScaleChanged()
            }
            return
        }
        if tileScale != scale {
            removeTiles()
        }
        markNeedsDisplay()
    }

    func retire() -> [AnyObject] {
        var retired: [AnyObject] = []
        if let document = document {
            retired.append(document)
        }
        document = nil
        for tile in tiles {
            retired.append(contentsOf: tile.surfaces as [AnyObject])
            tile.surfaces.removeAll()
            tile.shown = nil
        }
        return retired
    }

    private func removeTiles() {
        syncInvalidations += 1
        for tile in tiles {
            tile.layer.removeFromSuperlayer()
        }
        tiles.removeAll()
    }

    private(set) var tilesTrimmed = false

    static let trimsDetached = ProcessInfo.processInfo.environment["TW_TRIM_DETACHED"] != nil

    func trimTiles() {
        guard !tilesTrimmed, !detached || WidgetDocumentView.trimsDetached else {
            return
        }
        tilesTrimmed = true
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        removeTiles()
        CATransaction.commit()
    }

    func restoreTiles() {
        guard tilesTrimmed else {
            return
        }
        tilesTrimmed = false
        markNeedsDisplay()
    }

    var tileCount: Int {
        return tiles.count
    }

    private func syncGrid(layer: CALayer, document: LHDocument, scale: CGFloat) -> (columns: Int, tileSize: CGFloat, height: CGFloat, space: CGColorSpace)? {
        let space = WidgetDocumentView.screenTagged ? (window?.colorSpace?.cgColorSpace ?? WidgetDocumentView.surfaceSpace) : WidgetDocumentView.surfaceSpace
        let height = max(1, ceil(document.size.height))
        let pixelWidth = Int(ceil(size.width * scale))
        if pixelWidth <= 0 || Int(ceil(height * scale)) <= 0 || pixelWidth > 16384 || height * scale > 16384 {
            return nil
        }
        if surfaceSpace.map({ $0 != space }) ?? true || surfaceWidth != pixelWidth {
            removeTiles()
            surfaceSpace = space
            surfaceWidth = pixelWidth
        }
        let tileSize = WidgetDocumentView.tileHeight
        let columns = max(1, Int(ceil(size.width / tileSize)))
        let count = columns * Int(ceil(height / tileSize))
        while tiles.count > count {
            tiles.removeLast().layer.removeFromSuperlayer()
        }
        while tiles.count < count {
            let tile = Tile()
            tile.layer.contentsGravity = .topLeft
            if WidgetDocumentView.eightBitTiles {
                tile.layer.contentsFormat = .RGBA8Uint
            }
            tile.layer.anchorPoint = .zero
            tile.layer.actions = ["contents": NSNull(), "position": NSNull(), "bounds": NSNull()]
            layer.insertSublayer(tile.layer, at: UInt32(tiles.count))
            tiles.append(tile)
        }
        document.colorSpace = space
        tileScale = scale
        return (columns, tileSize, height, space)
    }

    private func syncTiles(layer: CALayer, document: LHDocument, scale: CGFloat) -> Bool {
        guard let grid = syncGrid(layer: layer, document: document, scale: scale) else {
            gridFails += 1
            return false
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer.contents = nil
        for (index, tile) in tiles.enumerated() {
            let top = CGFloat(index / grid.columns) * grid.tileSize
            let left = CGFloat(index % grid.columns) * grid.tileSize
            let rect = CGRect(x: left, y: top, width: min(grid.tileSize, size.width - left), height: min(grid.tileSize, grid.height - top))
            if rect != tile.rect {
                tile.rect = rect
                tile.surfaces.removeAll()
                tile.shown = nil
                tile.pending = true
            }
            if tile.layer.delegate == nil {
                drawer.view = self
                tile.layer.delegate = detached ? preparedDisplay : drawer
                tile.layer.isGeometryFlipped = false
                tile.layer.needsDisplayOnBoundsChange = !detached
            }
            if tile.layer.frame != rect || tile.layer.contentsScale != scale {
                syncInvalidations += 1
                tile.layer.frame = rect
                tile.layer.contentsScale = scale
                if tile.prepared {
                    unprepare(tile)
                } else if !detached {
                    tile.layer.setNeedsDisplay()
                }
            }
            tile.pending = false
        }
        CATransaction.commit()
        return true
    }

    func scaleChanged() {
        if tileScale != scale {
            removeTiles()
        }
        markNeedsDisplay()
    }

    override func updateLayer() {
        if detached {
            return
        }
        guard let layer = contentLayer, let document = document, !tilesTrimmed else {
            return
        }
        let scale = self.scale
        if WidgetDocumentView.layerTiles {
            _ = syncTiles(layer: layer, document: document, scale: scale)
            return
        }
        let start = CACurrentMediaTime()
        defer {
            drawCount += 1
            let spent = (CACurrentMediaTime() - start) * 1000
            drawMilliseconds += spent
            drawMaxMilliseconds = max(drawMaxMilliseconds, spent)
        }
        guard let (columns, tileSize, height, space) = syncGrid(layer: layer, document: document, scale: scale) else {
            return
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer.contents = nil
        for (index, tile) in tiles.enumerated() {
            let top = CGFloat(index / columns) * tileSize
            let left = CGFloat(index % columns) * tileSize
            let rect = CGRect(x: left, y: top, width: min(tileSize, bounds.width - left), height: min(tileSize, height - top))
            if rect != tile.rect {
                tile.rect = rect
                tile.surfaces.removeAll()
                tile.shown = nil
                tile.pending = true
            }
            if !tile.pending {
                continue
            }
            let pixelHeight = Int(ceil(rect.height * scale))
            let tileWidth = Int(ceil(rect.maxX * scale)) - Int(floor(rect.minX * scale))
            var target = tile.surfaces.first { $0 !== tile.shown && !IOSurfaceIsInUse($0.surface) }
            if target == nil && tile.surfaces.count < 3, let made = Surface(width: max(1, tileWidth), height: pixelHeight, space: space) {
                made.stale = rect
                tile.surfaces.append(made)
                target = made
            }
            guard let surface = target ?? tile.surfaces.first(where: { $0 !== tile.shown }) ?? tile.shown else {
                continue
            }
            let dirty = surface.stale.intersection(rect)
            if !dirty.isNull && !dirty.isEmpty {
                let x0 = floor(dirty.minX * scale) / scale, y0 = floor(dirty.minY * scale) / scale
                let x1 = ceil(dirty.maxX * scale) / scale, y1 = ceil(dirty.maxY * scale) / scale
                let aligned = CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
                let ctx = surface.context
                IOSurfaceLock(surface.surface, [], nil)
                ctx.saveGState()
                ctx.translateBy(x: 0, y: CGFloat(surface.height))
                ctx.scaleBy(x: scale, y: -scale)
                ctx.translateBy(x: -left, y: -top)
                ctx.setAllowsFontSmoothing(false)
                ctx.setShouldAntialias(true)
                ctx.clip(to: aligned)
                ctx.clear(aligned)
                document.draw(in: ctx, origin: .zero, clip: aligned.insetBy(dx: -WidgetDocumentView.cullMargin, dy: -WidgetDocumentView.cullMargin))
                ctx.restoreGState()
                ctx.flush()
                IOSurfaceUnlock(surface.surface, [], nil)
            }
            if WidgetDocumentView.verify {
                verifyFrame(surface, document: document, rect: rect, scale: scale, space: space)
            }
            surface.stale = .null
            tile.layer.frame = rect
            tile.layer.contentsScale = scale
            if tile.layer.contents as AnyObject? === surface.surface as AnyObject {
                tile.layer.contents = nil
            }
            tile.layer.contents = surface.surface
            tile.shown = surface
            tile.pending = false
        }
        CATransaction.commit()
    }

    func showBadge(_ text: String, fatal: Bool) {
        guard let layer = contentLayer else {
            return
        }
        if fatal {
            layer.backgroundColor = NSColor(white: 0.5, alpha: 0.25).cgColor
        }
        let badge = badgeLayer ?? CATextLayer()
        badgeLayer = badge
        badge.string = NSAttributedString(string: text, attributes: [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.white])
        badge.backgroundColor = NSColor(white: 0.24, alpha: 0.75).cgColor
        badge.cornerRadius = 9
        badge.alignmentMode = .center
        badge.contentsScale = scale
        badge.zPosition = 1000
        let width = ceil((text as NSString).size(withAttributes: [.font: NSFont.systemFont(ofSize: 11)]).width) + 14
        badge.frame = CGRect(x: size.width - width - 6, y: 6, width: width, height: 17)
        if badge.superlayer == nil {
            layer.addSublayer(badge)
        }
    }

    private var screenObserver: NSObjectProtocol?
    private var occlusionObserver: NSObjectProtocol?
    private var keyObservers: [NSObjectProtocol] = []
    struct WindowState {
        var inWindow = false
        var hidden = false
        var occlusionVisible = false
        var windowVisible = false
        var occlusionTrusted = false
        var key = false
        var display: CGDirectDisplayID?
        var colorSpace: CGColorSpace?
        var size = CGSize.zero
        var scrollRegions: [CGRect] = []
    }

    private let windowStateLock = NSLock()
    private var lockedWindowState = WindowState()

    var windowState: WindowState {
        windowStateLock.lock()
        defer { windowStateLock.unlock() }
        return lockedWindowState
    }

    private var occlusionTrusted = false

    func captureWindowState() {
        var state = WindowState()
        if let window = window {
            state.inWindow = true
            state.hidden = isHiddenOrHasHiddenAncestor
            state.occlusionVisible = window.occlusionState.contains(.visible)
            state.windowVisible = window.isVisible
            state.key = window.isKeyWindow
            state.display = (window.screen?.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber).map { CGDirectDisplayID($0.uint32Value) }
            state.colorSpace = window.colorSpace?.cgColorSpace
            if state.occlusionVisible {
                occlusionTrusted = true
            }
        }
        state.occlusionTrusted = occlusionTrusted
        windowStateLock.lock()
        state.size = lockedWindowState.size
        state.scrollRegions = lockedWindowState.scrollRegions
        lockedWindowState = state
        windowStateLock.unlock()
    }

    var size: CGSize {
        windowStateLock.lock()
        defer { windowStateLock.unlock() }
        return lockedWindowState.size
    }

    private func captureSize(_ size: CGSize) {
        windowStateLock.lock()
        lockedWindowState.size = size
        windowStateLock.unlock()
    }

    func publishScrollRegions(_ regions: [CGRect]) {
        windowStateLock.lock()
        lockedWindowState.scrollRegions = regions
        windowStateLock.unlock()
    }

    var detached: Bool {
        return !(owner?.executorIsMain() ?? true)
    }

    private func toOwner(_ block: @escaping (LiteHTMLWidgetRenderer) -> Void) {
        guard let owner = owner else {
            return
        }
        if owner.isOnExecutor() {
            block(owner)
            return
        }
        owner.scheduleUrgent { [weak owner] in
            if let owner = owner {
                block(owner)
            }
        }
    }

    override func viewDidHide() {
        super.viewDidHide()
        captureWindowState()
    }

    override func viewDidUnhide() {
        super.viewDidUnhide()
        captureWindowState()
        if detached {
            toOwner { $0.setVisible($0.isHostVisible) }
        }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        refreshWindowScale()
        if displayPending {
            displayPending = false
            needsDisplay = true
        }
        if let screenObserver = screenObserver {
            NotificationCenter.default.removeObserver(screenObserver)
        }
        if let occlusionObserver = occlusionObserver {
            NotificationCenter.default.removeObserver(occlusionObserver)
        }
        screenObserver = nil
        occlusionObserver = nil
        keyObservers.forEach { NotificationCenter.default.removeObserver($0) }
        keyObservers = []
        occlusionTrusted = false
        captureWindowState()
        if let window = window {
            for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification] {
                keyObservers.append(NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                    self?.captureWindowState()
                    self?.toOwner { $0.keyChanged() }
                })
            }
            screenObserver = NotificationCenter.default.addObserver(forName: NSWindow.didChangeScreenNotification, object: window, queue: .main) { [weak self] _ in
                self?.captureWindowState()
                self?.toOwner { $0.screenChanged() }
            }
            occlusionObserver = NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
                self?.occlusionTrusted = true
                self?.captureWindowState()
                self?.toOwner { $0.setVisible($0.isHostVisible) }
            }
        }
        toOwner { owner in
            owner.screenChanged()
            owner.keyChanged()
            owner.setVisible(owner.isHostVisible)
        }
    }

    deinit {
        keyObservers.forEach { NotificationCenter.default.removeObserver($0) }
        if let screenObserver = screenObserver {
            NotificationCenter.default.removeObserver(screenObserver)
        }
        if let occlusionObserver = occlusionObserver {
            NotificationCenter.default.removeObserver(occlusionObserver)
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
        let at = point(event)
        toOwner { $0.handleMouseDown(at, event: event) }
    }

    override func mouseDragged(with event: NSEvent) {
        let at = point(event)
        toOwner { $0.handleMouseDragged(at, event: event) }
    }

    override func mouseUp(with event: NSEvent) {
        let at = point(event)
        toOwner { $0.handleMouseUp(at, event: event) }
    }

    override func mouseMoved(with event: NSEvent) {
        let at = point(event)
        toOwner { $0.handleMouseMoved(at, event: event) }
    }

    override func mouseExited(with event: NSEvent) {
        toOwner { $0.handleMouseExited(event) }
    }

    override static var isCompatibleWithResponsiveScrolling: Bool {
        return true
    }

    override func scrollWheel(with event: NSEvent) {
        let at = point(event)
        if detached {
            windowStateLock.lock()
            let hit = lockedWindowState.scrollRegions.contains { $0.contains(at) }
            windowStateLock.unlock()
            if hit {
                toOwner { _ = $0.handleScroll(at, event: event) }
            } else {
                super.scrollWheel(with: event)
            }
            return
        }
        if owner?.handleScroll(at, event: event) != true {
            super.scrollWheel(with: event)
        }
    }

    override func keyDown(with event: NSEvent) {
        toOwner { $0.handleKey("keydown", event: event) }
    }

    override func keyUp(with event: NSEvent) {
        toOwner { $0.handleKey("keyup", event: event) }
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
    private static let lock = NSLock()
    private static var cache: [String: CTFont] = [:]
    static let cacheLimit = 64

    static func make(_ css: String) -> CTFont {
        lock.lock()
        if let font = cache[css] {
            lock.unlock()
            return font
        }
        lock.unlock()
        let font = resolve(css)
        lock.lock()
        if cache.count >= cacheLimit {
            cache.removeAll(keepingCapacity: true)
        }
        cache[css] = font
        lock.unlock()
        return font
    }

    static var cachedCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return cache.count
    }

    private static func resolve(_ css: String) -> CTFont {
        var size: CGFloat = 10
        var weight: CGFloat = 400
        var italic = false
        var families: [String] = []
        let tokens = WidgetNode.splitTopLevel(css, separator: " ").filter { !$0.isEmpty }
        var familyStart = tokens.count
        for (index, token) in tokens.enumerated() {
            let t = token.lowercased()
            if t == "bold" {
                weight = 700
            } else if t == "bolder" {
                weight = 900
            } else if t == "lighter" {
                weight = 300
            } else if let numeric = Int(t), numeric >= 1, numeric <= 1000, index < tokens.count - 1 {
                weight = CGFloat(numeric)
            } else if t == "italic" || t == "oblique" {
                italic = true
            } else if let px = t.split(separator: "/").first, px.hasSuffix("px"), let value = Double(px.dropLast(2)), value.isFinite {
                size = CGFloat(min(max(value, 1), 1000))
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
        let axis = 0x77676874
        if let axes = CTFontCopyVariationAxes(font) as? [[String: Any]], axes.contains(where: { ($0[kCTFontVariationAxisIdentifierKey as String] as? Int) == axis }) {
            let varied = CTFontDescriptorCreateCopyWithVariation(CTFontCopyFontDescriptor(font), axis as CFNumber, weight)
            font = CTFontCreateWithFontDescriptor(varied, size, nil)
        } else if weight >= 600, let bolded = CTFontCreateCopyWithSymbolicTraits(font, size, nil, .traitBold, .traitBold) {
            font = bolded
        }
        var traits: CTFontSymbolicTraits = []
        if italic {
            traits.insert(.traitItalic)
        }
        if !traits.isEmpty, let styled = CTFontCreateCopyWithSymbolicTraits(font, size, nil, traits, traits) {
            font = styled
        }
        return font
    }
}

public struct ScriptFramePacer {
    public static let defaultBudget: TimeInterval = ProcessInfo.processInfo.environment["TW_RAF_BUDGET_MS"].flatMap { Double($0) }.map { $0 / 1000 } ?? 0.001
    public var budget: TimeInterval
    public let fallbackInterval: CFTimeInterval
    public private(set) var cost: TimeInterval = 0
    public private(set) var fullRate = true
    private var postedAt: CFTimeInterval?
    private var lastPost: CFTimeInterval = -.infinity

    public init(budget: TimeInterval, fallbackInterval: CFTimeInterval) {
        self.budget = budget
        self.fallbackInterval = fallbackInterval
    }

    public func due(_ now: CFTimeInterval) -> Bool {
        return fullRate || now - lastPost >= fallbackInterval
    }

    public mutating func posted(_ now: CFTimeInterval) {
        lastPost = now
        if postedAt.map({ now - $0 > 0.25 }) ?? true {
            postedAt = now
        }
    }

    public mutating func answered(_ now: CFTimeInterval) {
        guard let posted = postedAt else {
            return
        }
        postedAt = nil
        let spent = now - posted
        cost = cost == 0 ? spent : cost * 0.8 + spent * 0.2
        if fullRate && cost > budget {
            fullRate = false
        } else if !fullRate && cost < budget * 0.7 {
            fullRate = true
        }
    }
}

final class WidgetFrameClock {
    private var link: CVDisplayLink?
    private let tick: () -> Void
    private let post: (@escaping () -> Void) -> Void
    private var running = false
    private var scheduled = false
    private var lastFire: CFTimeInterval = 0
    private let lock = NSLock()
    private static let minimumInterval = ProcessInfo.processInfo.environment["TW_FPS"].flatMap { Double($0) }.map { 1 / $0 - 0.002 } ?? 0

    init(post: @escaping (@escaping () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) }, tick: @escaping () -> Void) {
        self.tick = tick
        self.post = post
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
        let now = CACurrentMediaTime()
        if scheduled || now - lastFire < WidgetFrameClock.minimumInterval {
            lock.unlock()
            return
        }
        lastFire = now
        scheduled = true
        lock.unlock()
        post { [weak self] in
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

    func setDisplay(_ display: CGDirectDisplayID) {
        if let link = link {
            CVDisplayLinkSetCurrentCGDisplay(link, display)
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
