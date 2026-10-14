import XCTest
import AppKit
@testable import TwidgetRenderer

final class FrameCostBenchTests: XCTestCase {
    private func pump(_ seconds: TimeInterval, until done: () -> Bool) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done() && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.001))
        }
    }

    private func json(_ ops: [[String: Any]]) -> String {
        let data = try! JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops])
        return String(data: data, encoding: .utf8)!
    }

    func testBusyWidgetFrameCost() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["TWIDGET_BENCH"] != nil)
        _ = NSApplication.shared
        let count = Int(ProcessInfo.processInfo.environment["TWIDGET_BENCH_COUNT"] ?? "") ?? 1000
        let frames = Int(ProcessInfo.processInfo.environment["TWIDGET_BENCH_FRAMES"] ?? "") ?? 300
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 600), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.orderFrontRegardless()
        defer { window.orderOut(nil) }
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
        defer { renderer.destroy() }
        var queue: [() -> Void] = []
        renderer.schedule = { queue.append($0) }
        var answers = 0
        renderer.onMessage = { _ in answers += 1 }
        renderer.view.frame = NSRect(x: 0, y: 0, width: 400, height: 600)
        window.contentView?.addSubview(renderer.view)
        renderer.load()
        func drain() {
            while !queue.isEmpty {
                let items = queue
                queue = []
                items.forEach { $0() }
            }
        }
        var setup: [[String: Any]] = [
            ["op": "el", "id": 1, "tag": "body", "attrs": [], "adopt": "body"],
            ["op": "el", "id": 2, "tag": "style", "attrs": []],
            ["op": "css", "id": 2, "text": "body { margin: 0; font: 12px sans-serif; } .box { position: absolute; width: 18px; height: 18px; border-radius: 4px; background: #3a7; } .label { position: absolute; left: 0; color: #222; } canvas { position: absolute; left: 200px; top: 0; width: 200px; height: 200px; }"],
            ["op": "ins", "id": 2, "parent": 1],
        ]
        var next = 100
        var boxes: [Int] = []
        var texts: [Int] = []
        for i in 0..<count {
            let id = next
            next += 1
            setup.append(["op": "el", "id": id, "tag": "div", "attrs": [["class", "box"], ["style", "left:\((i % 20) * 20)px;top:\((i / 20) * 20)px"]]])
            setup.append(["op": "ins", "id": id, "parent": 1])
            boxes.append(id)
        }
        for i in 0..<30 {
            let id = next
            let text = next + 1
            next += 2
            setup.append(["op": "el", "id": id, "tag": "div", "attrs": [["class", "label"], ["style", "top:\(i * 16)px"]]])
            setup.append(["op": "ins", "id": id, "parent": 1])
            setup.append(["op": "text", "id": text, "data": "label \(i)"])
            setup.append(["op": "ins", "id": text, "parent": id])
            texts.append(text)
        }
        let canvas = next
        next += 1
        setup.append(["op": "el", "id": canvas, "tag": "canvas", "attrs": [["width", "200"], ["height", "200"]]])
        setup.append(["op": "ins", "id": canvas, "parent": 1])
        renderer.send(json(setup))
        drain()
        pump(10, until: { drain(); return !renderer.isPreparingTiles && renderer.heldOpBatches == 0 })
        let mode = ProcessInfo.processInfo.environment["TWIDGET_BENCH_MODE"] ?? "mixed"
        var payloads: [String] = []
        for f in 0..<frames {
            var ops: [[String: Any]] = []
            let moving = min(boxes.count, 300)
            for k in 0..<moving {
                let id = boxes[(k * 7 + f) % boxes.count]
                ops.append(["op": "style", "id": id, "name": "transform", "value": "translate(\(f % 13)px, \((f + k) % 9)px) rotate(\((f * 3 + k) % 360)deg)"])
            }
            if mode != "transform" {
                for k in 0..<20 {
                    ops.append(["op": "style", "id": boxes[(k * 31 + f) % boxes.count], "name": "background-color", "value": "rgb(\(f % 255), \(k * 10), 120)"])
                }
                for (k, text) in texts.enumerated() {
                    ops.append(["op": "data", "id": text, "data": "label \(k) frame \(f)"])
                }
                ops.append(["op": "canvas", "id": canvas, "cmds": [["fillStyle", "#f80"], ["fillRect", Double(f % 100), 10.0, 40.0, 40.0]]])
            }
            payloads.append(json(ops))
        }
        let queries = (0..<3).map { "{\"type\":\"query\",\"qid\":\($0),\"query\":{\"q\":\"rect\",\"id\":\(boxes[$0 * 50])}}" }
        var wall = 0.0
        var cpu = 0.0
        var waitCPU = 0.0
        let stagesBefore = renderer.stageMilliseconds
        for f in 0..<frames {
            let t0 = CACurrentMediaTime()
            let c0 = LiteHTMLWidgetRenderer.threadCPUSeconds()
            renderer.send(payloads[f])
            if mode != "transform" {
                for q in queries { renderer.send(q) }
            }
            if let point = Optional(NSPoint(x: Double(f % 400), y: 300)), mode == "mixed" {
                let event = NSEvent.mouseEvent(with: .mouseMoved, location: renderer.view.convert(point, to: nil), modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 0, pressure: 0)
                if let event = event { renderer.view.mouseMoved(with: event) }
            }
            drain()
            wall += CACurrentMediaTime() - t0
            cpu += LiteHTMLWidgetRenderer.threadCPUSeconds() - c0
            let c1 = LiteHTMLWidgetRenderer.threadCPUSeconds()
            pump(2, until: { drain(); return !renderer.isPreparingTiles && renderer.heldOpBatches == 0 })
            waitCPU += LiteHTMLWidgetRenderer.threadCPUSeconds() - c1
        }
        var stages: [String] = []
        for (key, value) in renderer.stageMilliseconds.sorted(by: { $0.key < $1.key }) {
            let delta = value - (stagesBefore[key] ?? 0)
            if delta > 0 { stages.append(String(format: "%@=%.1f", key, delta * 1000 / Double(frames))) }
        }
        print(String(format: "BENCH mode=%@ n=%d frames=%d wall=%.1fus cpu=%.1fus tailcpu=%.1fus answers=%d layouts=%d", mode, count, frames, wall * 1e6 / Double(frames), cpu * 1e6 / Double(frames), waitCPU * 1e6 / Double(frames), answers, renderer.layoutCount))
        print("BENCH stages(us/frame or count*1000/frame) " + stages.joined(separator: " "))
    }
}
