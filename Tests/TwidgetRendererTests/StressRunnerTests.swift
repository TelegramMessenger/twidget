import AppKit
import QuartzCore
import WidgetSandbox
import XCTest

@testable import TwidgetRenderer

@available(macOS 14.0, *)
private final class StressFrameClock: NSObject {
    var times: [Double] = []
    @objc func tick(_ link: CADisplayLink) {
        times.append(link.timestamp * 1000)
    }
    func summary() -> [String: Any] {
        let gaps = zip(times.dropFirst(), times).map { $0.0 - $0.1 }.sorted()
        func percentile(_ fraction: Double) -> Any {
            guard !gaps.isEmpty else { return NSNull() }
            return gaps[min(gaps.count - 1, Int(Double(gaps.count - 1) * fraction))]
        }
        return [
            "count": times.count, "p50": percentile(0.5), "p95": percentile(0.95),
            "max": gaps.last as Any? ?? NSNull(), "dropped": NSNull(), "longTaskMs": NSNull(),
            "mainThreadMsPerSec": NSNull(),
        ]
    }
}

final class StressRunnerTests: XCTestCase {
    private func pump(_ seconds: Double, until done: () -> Bool = { false }) {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline && !done() {
            RunLoop.main.run(until: min(deadline, Date().addingTimeInterval(0.01)))
        }
    }

    func testStressKit() throws {
        let env = ProcessInfo.processInfo.environment
        guard env["TW_STRESS"] == "1" else {
            throw XCTSkip("set TW_STRESS=1 for the native stress kit")
        }
        guard #available(macOS 14.0, *) else { throw XCTSkip("display link requires macOS 14") }
        let sandbox = URL(fileURLWithPath: try XCTUnwrap(env["TWIDGET_WIDGET_SANDBOX"]))
        let root = sandbox.appendingPathComponent("packages/stress")
        let manifest = try XCTUnwrap(
            JSONSerialization.jsonObject(
                with: Data(contentsOf: root.appendingPathComponent("manifest.json")))
                as? [String: Any])
        let widgets = try XCTUnwrap(manifest["widgets"] as? [[String: Any]])
        let defaults = try XCTUnwrap(manifest["run"] as? [String: Any])
        let warmup =
            Double(env["TW_STRESS_WARMUP_MS"] ?? "") ?? (defaults["warmupMs"] as? Double ?? 2000)
        let measure =
            Double(env["TW_STRESS_MEASURE_MS"] ?? "") ?? (defaults["measureMs"] as? Double ?? 10000)
        let smoke = warmup < 2000 || measure < 10000
        let cooldown = Double(env["TW_STRESS_COOLDOWN_MS"] ?? "") ?? 20000
        let out = URL(fileURLWithPath: try XCTUnwrap(env["TW_STRESS_OUT"]))
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        for pass in ["measure", "production"] {
            var options = WidgetHost.Options()
            options.features = ["binaryOps", "canvasWords", "webAnimations", "morphUpdates"]
            options.nativeScrolling = true
            options.asyncValidation = true
            options.sharedValidatorThread = true
            options.startBeforeRendererReady = true
            options.pools = true
            if pass == "measure" {
                options.policy = .compat
                options.cpuQuotaMs = 10000
                options.mainThreadQuotaMs = 10000
                options.maxTimers = 256
                options.memoryLimitBytes = 64 << 20
            }
            let config: [String: Any] = [
                "policy": pass == "measure" ? "compat" : "strict", "cpuQuotaMs": options.cpuQuotaMs,
                "cpuQuotaWindowMs": options.cpuQuotaWindowMs,
                "mainThreadQuotaMs": options.mainThreadQuotaMs, "maxTimers": options.maxTimers,
                "memoryLimitBytes": options.memoryLimitBytes,
                "features": ["binaryOps", "canvasWords", "webAnimations", "morphUpdates"],
                "nativeScrolling": true, "width": 390, "theme": "light", "warmupMs": warmup,
                "measureMs": measure,
            ]
            var runs: [[String: Any]] = []
            func save() throws {
                let result: [String: Any] = [
                    "kit": ["version": manifest["version"] ?? 1, "commit": "v0.3.0"],
                    "platform": "macos",
                    "client": "Telegram macOS native renderer harness", "clientBuild": "Debug",
                    "framework": "v0.3.0", "renderer": "twidget-litehtml", "rendererKind": "own",
                    "engine": "Apple litehtml",
                    "device": [
                        "model": ConformanceRunnerTests.machine,
                        "os": ProcessInfo.processInfo.operatingSystemVersionString,
                        "class": "desktop",
                        "refreshHz": NSNull(),
                        "dpr": NSScreen.main?.backingScaleFactor as Any? ?? NSNull(),
                    ], "pass": pass, "config": ["requested": config, "applied": config],
                    "date": ISO8601DateFormatter().string(from: Date()), "smoke": smoke,
                    "runs": runs,
                ]
                try JSONSerialization.data(
                    withJSONObject: result, options: [.prettyPrinted, .sortedKeys]
                )
                .write(to: out.appendingPathComponent("macos-\(pass).run.json"), options: .atomic)
            }
            for (index, widget) in widgets.enumerated() {
                try autoreleasepool {
                    let id = try XCTUnwrap(widget["id"] as? String)
                    let path = try XCTUnwrap(widget["file"] as? String)
                    let html = try String(
                        contentsOf: root.appendingPathComponent(path), encoding: .utf8)
                    let started = CACurrentMediaTime()
                    let session = try ConformanceRunnerTests.Session(
                        html: html, theme: "light",
                        features: ["binaryOps", "canvasWords", "webAnimations", "morphUpdates"],
                        configure: { $0 = options })
                    session.probe.publicationAttribute = "data-stress"
                    defer { session.close() }
                    var diedAt: Double?
                    func observeDeath() {
                        if diedAt == nil && session.host.diagnostics.dead != nil {
                            diedAt = (CACurrentMediaTime() - started) * 1000
                        }
                    }
                    pump(
                        60,
                        until: {
                            session.host.diagnostics.loaded || session.host.diagnostics.dead != nil
                        })
                    let boot = session.host.diagnostics.loadedMs
                    observeDeath()
                    pump(warmup / 1000)
                    let before = session.host.diagnostics.stats
                    let first = session.probe.published
                    let ops = session.probe.opCount
                    let clock = StressFrameClock()
                    let link = session.host.view.displayLink(
                        target: clock, selector: #selector(StressFrameClock.tick(_:)))
                    link.add(to: .main, forMode: .common)
                    let windowStart = CACurrentMediaTime()
                    var maxGap = 0.0
                    while (CACurrentMediaTime() - windowStart) * 1000 < measure {
                        pump(min(0.1, measure / 1000))
                        maxGap = max(maxGap, session.probe.published?["maxGap"] as? Double ?? 0)
                        observeDeath()
                    }
                    link.invalidate()
                    let seconds = CACurrentMediaTime() - windowStart
                    let state = session.host.diagnostics
                    let last = session.probe.published
                    let frameDelta =
                        (last?["frames"] as? Int ?? 0) - (first?["frames"] as? Int ?? 0)
                    let timeDelta = (last?["t"] as? Double ?? 0) - (first?["t"] as? Double ?? 0)
                    let errors = state.logs.filter { $0.level == "error" }.map { $0.text }
                    let row: [String: Any] = [
                        "id": id, "load": widget["load"] ?? "", "value": widget["value"] ?? 0,
                        "loaded": state.loaded, "bootMs": boot as Any? ?? NSNull(),
                        "dead": state.dead as Any? ?? NSNull(),
                        "diedAtMs": diedAt as Any? ?? NSNull(),
                        "frames": clock.summary(), "renderer": NSNull(),
                        "host": [
                            "jsMsPerSec": (state.stats.jsMs - before.jsMs) / seconds,
                            "opsPerSec": Double(session.probe.opCount - ops) / seconds,
                            "queriesPerSec": Double(state.stats.queries - before.queries) / seconds,
                            "queryMsPerSec": (state.stats.queryMs - before.queryMs) / seconds,
                            "nodes": state.nodes, "memoryBytes": state.stats.memoryBytes,
                            "throttledTasks": state.stats.throttledTasks - before.throttledTasks,
                            "maxBatchOps": session.probe.maxBatchOps,
                        ],
                        "widget": [
                            "fps": timeDelta > 0
                                ? Double(frameDelta) * 1000 / timeDelta as Any : NSNull(),
                            "frames": first != nil && last != nil ? frameDelta as Any : NSNull(),
                            "maxGap": last != nil ? maxGap as Any : NSNull(),
                            "jank": first != nil && last != nil
                                ? ((last?["jank"] as? Int ?? 0) - (first?["jank"] as? Int ?? 0))
                                    as Any : NSNull(),
                            "last": last as Any? ?? NSNull(),
                        ], "thermal": String(describing: ProcessInfo.processInfo.thermalState),
                        "errors": errors,
                        "diags": state.diags.mapValues {
                            ["count": $0.count, "details": $0.details] as [String: Any]
                        },
                    ]
                    runs.append(row)
                    try save()
                    XCTAssertTrue(
                        state.loaded || state.dead != nil, "\(pass) \(id) never loaded or stopped")
                    XCTAssertTrue(
                        last != nil || state.dead != nil, "\(pass) \(id) never published or stopped"
                    )
                    print(
                        "STRESS \(pass) \(index + 1)/\(widgets.count) \(id) dead=\(state.dead ?? "none") errors=\(errors.count)"
                    )
                }
                pump(smoke ? 0.05 : 1)
                let load = widget["load"] as? String
                if index + 1 == widgets.count || widgets[index + 1]["load"] as? String != load {
                    pump(cooldown / 1000)
                }
            }
            XCTAssertEqual(runs.count, widgets.count)
        }
    }
}
