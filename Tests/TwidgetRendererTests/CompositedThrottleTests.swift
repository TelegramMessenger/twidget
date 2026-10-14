import XCTest
import AppKit
import WidgetSandboxProtocol
@testable import TwidgetRenderer

final class CompositedThrottleTests: XCTestCase {
    func testNativeMotionContinuesWithoutReorderingBudgetedChanges() throws {
        _ = NSApplication.shared
        for scenario in 0..<9 {
            let format = scenario % 3
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 240), styleMask: [.borderless], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
            renderer.view.frame = NSRect(x: 0, y: 0, width: 400, height: 240)
            window.contentView?.addSubview(renderer.view)
            var scheduled: [() -> Void] = []
            renderer.schedule = { scheduled.append($0) }
            defer { renderer.destroy() }
            func send(_ ops: [[String: Any]]) throws {
                if format == 0 {
                    renderer.send(ops: ops.map(WidgetOp.init))
                } else if format == 1 {
                    let data = try JSONSerialization.data(withJSONObject: ops)
                    renderer.send("{\"type\":\"ops\",\"ops\":" + String(data: data, encoding: .utf8)! + "}")
                } else {
                    renderer.send(String(data: try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops], options: .sortedKeys), encoding: .utf8)!)
                }
                while !scheduled.isEmpty { scheduled.removeFirst()() }
            }
            func move(_ x: Int) -> [[String: Any]] {
                [["op": "style", "id": 3, "name": "transform", "value": "translate(\(x)px,20px)"]]
            }
            renderer.load()
            try send([
                ["op": "el", "id": 1, "tag": "body", "attrs": [["style", "margin:0;height:240px"]], "adopt": "body"],
                ["op": "el", "id": 3, "tag": "div", "attrs": [["style", "width:20px;height:20px;background:red"]]],
                ["op": "ins", "id": 3, "parent": 1],
            ])
            for x in 100...103 { try send(move(x)) }
            XCTAssertEqual(renderer.compositedCount, 1)
            Thread.sleep(forTimeInterval: 1.05)
            renderer.charge(0.2)
            let layouts = renderer.layoutCount
            let updates = renderer.stageMilliseconds["scripted-fast", default: 0]
            try send(move(110))
            XCTAssertEqual(renderer.heldOpBatches, 0)
            XCTAssertEqual(renderer.layoutCount, layouts)
            XCTAssertEqual(renderer.stageMilliseconds["scripted-fast", default: 0], updates + 1)
            let image = try XCTUnwrap(renderer.snapshot())
            let pixels = NSBitmapImageRep(cgImage: try XCTUnwrap(image.cgImage(forProposedRect: nil, context: nil, hints: nil)))
            let scale = CGFloat(pixels.pixelsWide) / image.size.width
            let moved = try XCTUnwrap(pixels.colorAt(x: Int(115 * scale), y: Int(25 * scale))?.usingColorSpace(.sRGB))
            XCTAssertGreaterThan(moved.redComponent, 0.9)
            XCTAssertLessThan(moved.greenComponent, 0.3)
            let costly: [[String: Any]]
            switch scenario / 3 {
            case 0: costly = [["op": "style", "id": 3, "name": "width", "value": "30px"]]
            case 1: costly = move(390)
            default: costly = [["op": "style", "id": 3, "name": "transform", "value": "scale(8)"]]
            }
            renderer.charge(0.2)
            try send(costly)
            XCTAssertEqual(renderer.heldOpBatches, 1, "scenario \(scenario)")
            try send(move(111))
            XCTAssertEqual(renderer.heldOpBatches, 2, "scenario \(scenario)")
            XCTAssertEqual(renderer.layoutCount, layouts)
            XCTAssertEqual(renderer.stageMilliseconds["scripted-fast", default: 0], updates + 1)
        }
    }
}
