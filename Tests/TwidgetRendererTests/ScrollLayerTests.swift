import XCTest
import AppKit
import LiteHTML
@testable import TwidgetRenderer

final class ScrollLayerTests: XCTestCase {
    private func list(rows: Int = 100) -> LHDocument {
        let items = (0..<rows).map { "<div style='height:20px;background:\($0 % 2 == 0 ? "#f00" : "#00f")'>\($0)</div>" }.joined()
        let document = LHDocument(html: "<body style='margin:0;background:#fff'><div style='height:10px'></div><div id='list' style='height:100px;overflow:auto'>\(items)</div></body>", css: nil)
        _ = document.layout(maxWidth: 120, preferredWidth: 120)
        return document
    }

    func testScrollersReportViewportAndContent() throws {
        let document = list()
        let info = try XCTUnwrap(document.scrollers(byAttribute: "id")["list"])
        XCTAssertEqual((info["viewport"] as? NSValue)?.rectValue, CGRect(x: 0, y: 10, width: 120, height: 100))
        XCTAssertEqual((info["content"] as? NSValue)?.sizeValue.height, 2000)
    }

    func testOffsetsClampAndDriveHitTesting() {
        let document = list()
        XCTAssertEqual(document.setScrollOffset(CGPoint(x: 0, y: 5000), attribute: "id", value: "list"), CGPoint(x: 0, y: 1900))
        XCTAssertEqual(document.setScrollOffset(CGPoint(x: 0, y: 410), attribute: "id", value: "list"), CGPoint(x: 0, y: 410))
        document.setPaintExclusions([], attribute: "id")
        XCTAssertEqual(document.setScrollOffset(CGPoint(x: 0, y: -3), attribute: "id", value: "list"), .zero)
    }

    private func red(_ image: CGImage, _ y: Int, scale: Int) -> Int {
        var data = [UInt8](repeating: 0, count: 4)
        let ctx = CGContext(data: &data, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(image, in: CGRect(x: -100 * scale, y: (y * scale) - image.height + 1, width: image.width, height: image.height))
        return Int(data[0])
    }

    func testScrollContentRastersInContentCoordinates() throws {
        let document = list()
        _ = document.setScrollOffset(CGPoint(x: 0, y: 300), attribute: "id", value: "list")
        let tile = try XCTUnwrap(document.copyScrollContent(attribute: "id", value: "list", rect: CGRect(x: 0, y: 40, width: 120, height: 60), scale: 2))
        XCTAssertEqual(tile.height, 120)
        XCTAssertEqual(red(tile, 5, scale: 2), 255)
        XCTAssertEqual(red(tile, 25, scale: 2), 0)
        document.setScrollContentExclusions(["list"], attribute: "id")
        let page = try XCTUnwrap(document.copyImage(maxWidth: 120, rect: CGRect(x: 0, y: 0, width: 120, height: 120), scale: 1))
        XCTAssertEqual(red(page, 50, scale: 1), 255)
        var data = [UInt8](repeating: 0, count: 4)
        let ctx = CGContext(data: &data, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(page, in: CGRect(x: -100, y: 50 - page.height + 1, width: page.width, height: page.height))
        XCTAssertEqual(Array(data.prefix(3)), [255, 255, 255])
    }

    func testRubberBandingResistsAndSettles() {
        let geometry = WidgetScrollGeometry(viewport: CGRect(x: 0, y: 0, width: 100, height: 200), content: CGSize(width: 100, height: 1000), radii: [CGFloat](repeating: 0, count: 8))
        XCTAssertEqual(geometry.maxOffset, CGPoint(x: 0, y: 800))
        XCTAssertEqual(geometry.clamp(CGPoint(x: 5, y: -20)), .zero)
        let pulled = geometry.banded(CGPoint(x: 0, y: -100))
        XCTAssertLessThan(pulled.y, 0)
        XCTAssertGreaterThan(pulled.y, -100)
        XCTAssertLessThan(abs(geometry.banded(CGPoint(x: 0, y: -10_000)).y), 200)
        XCTAssertEqual(geometry.banded(CGPoint(x: 0, y: 400)).y, 400)
        XCTAssertGreaterThan(geometry.banded(CGPoint(x: 0, y: 900)).y, 800)
    }
    private func pump(_ seconds: TimeInterval, until done: () -> Bool = { false }) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done() && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.005))
        }
    }

    private func wheel(_ dy: Int32, phase: Int64 = 0, momentum: Int64 = 0) -> NSEvent {
        let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: dy, wheel2: 0, wheel3: 0)!
        cg.setIntegerValueField(CGEventField(rawValue: 88)!, value: 1)
        cg.setIntegerValueField(CGEventField(rawValue: 99)!, value: phase)
        cg.setIntegerValueField(CGEventField(rawValue: 123)!, value: momentum)
        return NSEvent(cgEvent: cg)!
    }

    func testTrackpadGesturesScrollRubberBandAndSettle() throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 300), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.orderFrontRegardless()
        defer { window.orderOut(nil) }
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 200)
        var messages: [String] = []
        renderer.onMessage = { messages.append($0) }
        renderer.view.frame = NSRect(x: 0, y: 0, width: 200, height: 300)
        window.contentView?.addSubview(renderer.view)
        renderer.load()
        var ops: [[String: Any]] = [
            ["op": "el", "id": 1, "tag": "body", "attrs": [], "adopt": "body"],
            ["op": "el", "id": 2, "tag": "style", "attrs": []],
            ["op": "css", "id": 2, "text": "body { margin: 0; } .list { height: 100px; overflow: auto; } .row { height: 20px; }"],
            ["op": "ins", "id": 2, "parent": 1],
            ["op": "el", "id": 3, "tag": "div", "attrs": [["class", "list"]]],
            ["op": "ins", "id": 3, "parent": 1],
        ]
        for row in 0..<50 {
            ops.append(["op": "el", "id": 10 + row, "tag": "div", "attrs": [["class", "row"]]])
            ops.append(["op": "ins", "id": 10 + row, "parent": 3])
        }
        let data = try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops])
        renderer.send(String(data: data, encoding: .utf8)!)
        renderer.send("{\"type\":\"listen\",\"types\":[\"scroll\"]}")
        pump(1.0, until: { renderer.scrolling.layers[3] != nil })
        let layer = try XCTUnwrap(renderer.scrolling.layers[3])
        let point = CGPoint(x: 50, y: 50)
        XCTAssertFalse(renderer.handleScroll(point, event: wheel(30, phase: 1)))
        for _ in 0..<5 {
            XCTAssertFalse(renderer.handleScroll(point, event: wheel(30, phase: 2)))
        }
        XCTAssertFalse(renderer.handleScroll(point, event: wheel(0, phase: 4)))
        XCTAssertEqual(renderer.scrolling.offset(3), .zero)
        XCTAssertTrue(renderer.handleScroll(point, event: wheel(-40, phase: 1)))
        for _ in 0..<10 {
            XCTAssertTrue(renderer.handleScroll(point, event: wheel(-40, phase: 2)))
        }
        XCTAssertEqual(renderer.scrolling.offset(3).y, 440)
        XCTAssertEqual(layer.offset.y, 440)
        for _ in 0..<20 {
            _ = renderer.handleScroll(point, event: wheel(-60, phase: 2))
        }
        XCTAssertEqual(renderer.scrolling.offset(3).y, 900)
        XCTAssertGreaterThan(layer.offset.y, 900)
        XCTAssertLessThan(layer.offset.y, 1000)
        XCTAssertTrue(renderer.handleScroll(point, event: wheel(0, phase: 4)))
        XCTAssertEqual(layer.offset.y, 900)
        XCTAssertNotNil(layer.content.animation(forKey: "tw.settle"))
        XCTAssertTrue(renderer.handleScroll(point, event: wheel(-20, momentum: 1)))
        XCTAssertTrue(renderer.handleScroll(point, event: wheel(-10, momentum: 2)))
        XCTAssertTrue(renderer.handleScroll(point, event: wheel(0, momentum: 3)))
        XCTAssertEqual(layer.offset.y, 900)
        pump(0.3, until: { messages.contains { $0.contains("\"scroll\"") } })
        XCTAssertTrue(messages.contains { $0.contains("\"scroll\"") && $0.contains("\"targetId\":3") })
        renderer.destroy()
    }

    func testScrollingInsidePromotedModalRefreshesItsPixels() throws {
        try checkScrolling(promotedID: 3)
    }

    func testScrollingMovesPromotedDescendantsWithTheirContents() throws {
        try checkScrolling(promotedID: 12)
    }

    private func checkScrolling(promotedID: Int) throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 180), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 200)
        var scheduled: [() -> Void] = []
        renderer.schedule = { scheduled.append($0) }
        renderer.view.frame = NSRect(x: 0, y: 0, width: 200, height: 180)
        window.contentView?.addSubview(renderer.view)
        defer { renderer.destroy() }
        func drain() {
            while !scheduled.isEmpty { scheduled.removeFirst()() }
        }
        func send(_ ops: [[String: Any]]) throws {
            let data = try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops])
            renderer.send(String(data: data, encoding: .utf8)!)
            drain()
        }
        func redAtMiddle() throws -> Int {
            let image = try XCTUnwrap(renderer.snapshot())
            let cgImage = try XCTUnwrap(image.cgImage(forProposedRect: nil, context: nil, hints: nil))
            return red(cgImage, 50, scale: cgImage.width / 200)
        }
        renderer.load()
        var ops: [[String: Any]] = [
            ["op": "el", "id": 1, "tag": "body", "attrs": [], "adopt": "body"],
            ["op": "el", "id": 2, "tag": "style", "attrs": []],
            ["op": "css", "id": 2, "text": "body{margin:0}.modal{width:120px;height:100px;overflow:hidden}.list{height:100px;overflow:auto}.row{height:20px}.moving{animation:slide 1000s linear infinite}@keyframes slide{from{transform:translateX(0px)}to{transform:translateX(1px)}}"],
            ["op": "ins", "id": 2, "parent": 1],
            ["op": "el", "id": 3, "tag": "div", "attrs": [["class", "modal"]]],
            ["op": "ins", "id": 3, "parent": 1],
            ["op": "el", "id": 4, "tag": "div", "attrs": [["class", "list"]]],
            ["op": "ins", "id": 4, "parent": 3],
        ]
        for row in 0..<20 {
            let spacing = row == 2 ? ";height:12px;width:100px;margin:4px 10px" : ""
            let classes = promotedID == 12 && row == 2 ? "row moving" : "row"
            ops.append(["op": "el", "id": 10 + row, "tag": "div", "attrs": [["class", classes], ["style", "background:\(row % 2 == 0 ? "#f00" : "#00f")\(spacing)"]]])
            ops.append(["op": "ins", "id": 10 + row, "parent": 4])
        }
        try send(ops)
        for x in promotedID == 3 ? [1, 2, 3, 4, 0] : [] {
            try send([["op": "style", "id": promotedID, "name": "transform", "value": "translate(\(x)px,0px)"]])
        }
        XCTAssertEqual(renderer.compositedCount, 1)
        XCTAssertNil(renderer.scrolling.layers[4])
        XCTAssertGreaterThan(try redAtMiddle(), 250)
        XCTAssertTrue(renderer.handleScroll(CGPoint(x: 50, y: 50), event: wheel(-20)))
        drain()
        XCTAssertEqual(renderer.scrolling.offset(4).y, 20)
        XCTAssertLessThan(try redAtMiddle(), 5)
        try send([["op": "prop", "id": 4, "name": "scrollTop", "value": 40]])
        XCTAssertGreaterThan(try redAtMiddle(), 250)
    }
}
