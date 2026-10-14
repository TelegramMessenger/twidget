import XCTest
import AppKit
import LiteHTML
@testable import TwidgetRenderer

final class ScrollLayerTests: XCTestCase {
    func testRevealAlignmentAndNearestEdges() {
        let geometry = WidgetScrollGeometry(viewport: CGRect(x: 10, y: 20, width: 100, height: 100), content: CGSize(width: 500, height: 600), radii: [])
        let current = CGPoint(x: 150, y: 150)
        let target = CGRect(x: 130, y: 150, width: 20, height: 30)
        XCTAssertEqual(geometry.offsetToReveal(target, from: current, block: 0, inline: 0), CGPoint(x: 270, y: 280))
        XCTAssertEqual(geometry.offsetToReveal(target, from: current, block: 1, inline: 1), CGPoint(x: 230, y: 245))
        XCTAssertEqual(geometry.offsetToReveal(target, from: current, block: 2, inline: 2), CGPoint(x: 190, y: 210))
        XCTAssertEqual(geometry.offsetToReveal(target, from: current, block: 3, inline: 3), CGPoint(x: 190, y: 210))
        for target in [CGRect(x: 30, y: 40, width: 20, height: 20), CGRect(x: 0, y: 0, width: 150, height: 150)] {
            XCTAssertEqual(geometry.offsetToReveal(target, from: current, block: 3, inline: 3), current)
        }
        XCTAssertEqual(geometry.offsetToReveal(CGRect(x: 0, y: 0, width: 20, height: 30), from: current, block: 3, inline: 3), CGPoint(x: 140, y: 130))
        XCTAssertEqual(geometry.offsetToReveal(CGRect(x: 0, y: 0, width: 100, height: 110), from: current, block: 3, inline: 3), CGPoint(x: 140, y: 140))
        XCTAssertEqual(geometry.offsetToReveal(CGRect(x: 30, y: 40, width: 140, height: 150), from: current, block: 3, inline: 3), CGPoint(x: 170, y: 170))
    }

    func testRevealNewTargetInNestedScrollers() throws {
        _ = NSApplication.shared
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 200)
        renderer.view.frame = NSRect(x: 0, y: 0, width: 200, height: 180)
        var scheduled: [() -> Void] = []
        renderer.schedule = { scheduled.append($0) }
        defer { renderer.destroy() }
        func send(_ ops: [[String: Any]]) throws {
            renderer.send(String(data: try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops]), encoding: .utf8)!)
            while !scheduled.isEmpty { scheduled.removeFirst()() }
        }
        renderer.load()
        try send([
            ["op": "el", "id": 1, "tag": "body", "attrs": [], "adopt": "body"],
            ["op": "el", "id": 2, "tag": "style", "attrs": []],
            ["op": "css", "id": 2, "text": "body{margin:0}#outer{width:120px;height:100px;overflow:auto}#space{width:500px;height:500px;padding-top:200px;box-sizing:border-box}#inner{margin-left:200px;width:180px;height:140px;overflow:auto}#content{position:relative;width:600px;height:500px}#target{position:absolute;left:260px;top:220px;width:40px;height:30px}"],
            ["op": "ins", "id": 2, "parent": 1],
            ["op": "el", "id": 3, "tag": "div", "attrs": [["id", "outer"]]], ["op": "ins", "id": 3, "parent": 1],
            ["op": "el", "id": 4, "tag": "div", "attrs": [["id", "space"]]], ["op": "ins", "id": 4, "parent": 3],
            ["op": "el", "id": 5, "tag": "div", "attrs": [["id", "inner"]]], ["op": "ins", "id": 5, "parent": 4],
            ["op": "el", "id": 6, "tag": "div", "attrs": [["id", "content"]]], ["op": "ins", "id": 6, "parent": 5],
        ])
        try send([
            ["op": "el", "id": 7, "tag": "div", "attrs": [["id", "target"]]], ["op": "ins", "id": 7, "parent": 6],
            ["op": "call", "id": 7, "method": "scrollIntoView", "args": [1, 1, 1, 1]],
        ])
        XCTAssertEqual(renderer.scrolling.offset(5), CGPoint(x: 190, y: 165))
        XCTAssertEqual(renderer.scrolling.offset(3), .zero)
        try send([["op": "call", "id": 7, "method": "scrollIntoView", "args": [1, 1, 1, 0]]])
        XCTAssertEqual(renderer.scrolling.offset(5), CGPoint(x: 190, y: 165))
        XCTAssertEqual(renderer.scrolling.offset(3), CGPoint(x: 230, y: 220))
        try send([["op": "call", "id": 7, "method": "scrollIntoView", "args": [3, 3, 1, 0]]])
        XCTAssertEqual(renderer.scrolling.offset(5), CGPoint(x: 190, y: 165))
        XCTAssertEqual(renderer.scrolling.offset(3), CGPoint(x: 230, y: 220))
    }

    func testSmoothRevealMovesDocumentAndCanBeInterrupted() throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 180), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.orderFrontRegardless()
        defer { window.orderOut(nil) }
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 200)
        renderer.view.frame = NSRect(x: 0, y: 0, width: 200, height: 180)
        window.contentView?.addSubview(renderer.view)
        defer { renderer.destroy() }
        func send(_ ops: [[String: Any]]) throws {
            renderer.send(String(data: try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops]), encoding: .utf8)!)
        }
        renderer.load()
        var ops: [[String: Any]] = [
            ["op": "el", "id": 1, "tag": "body", "attrs": [], "adopt": "body"],
            ["op": "el", "id": 2, "tag": "style", "attrs": []],
            ["op": "css", "id": 2, "text": "body{margin:0}#list{width:120px;height:100px;overflow:auto}.row{height:20px}"],
            ["op": "ins", "id": 2, "parent": 1],
            ["op": "el", "id": 3, "tag": "div", "attrs": [["id", "list"]]], ["op": "ins", "id": 3, "parent": 1],
        ]
        for row in 0..<30 {
            ops.append(["op": "el", "id": 10 + row, "tag": "div", "attrs": [["class", "row"]]])
            ops.append(["op": "ins", "id": 10 + row, "parent": 3])
        }
        try send(ops)
        pump(1, until: { renderer.scrolling.geometries[3] != nil })
        func reveal(_ behavior: Int) throws {
            try send([["op": "call", "id": 39, "method": "scrollIntoView", "args": [3, 3, behavior, 0]]])
        }
        try reveal(2)
        pump(1, until: { renderer.scrolling.offset(3).y > 0 })
        XCTAssertGreaterThan(renderer.scrolling.offset(3).y, 0)
        XCTAssertLessThan(renderer.scrolling.offset(3).y, 500)
        XCTAssertEqual(renderer.elementBounds()[39]?.minY ?? -1, 580 - renderer.scrolling.offset(3).y, accuracy: 1)
        pump(1, until: { renderer.scrolling.offset(3).y == 500 })
        XCTAssertEqual(renderer.scrolling.offset(3).y, 500)
        try send([["op": "prop", "id": 3, "name": "scrollTop", "value": 0]])
        pump(1, until: { renderer.scrolling.offset(3).y == 0 })
        try reveal(2)
        pump(1, until: { renderer.scrolling.offset(3).y > 20 })
        XCTAssertTrue(renderer.handleScroll(CGPoint(x: 50, y: 50), event: wheel(10)))
        let interrupted = renderer.scrolling.offset(3)
        pump(0.4)
        XCTAssertEqual(renderer.scrolling.offset(3), interrupted)
        try reveal(2)
        pump(1, until: { renderer.scrolling.offset(3).y > interrupted.y + 5 })
        try send([["op": "prop", "id": 3, "name": "scrollTop", "value": 75]])
        pump(0.4)
        XCTAssertEqual(renderer.scrolling.offset(3).y, 75)
        try reveal(1)
        pump(1, until: { renderer.scrolling.offset(3).y == 500 })
        XCTAssertEqual(renderer.scrolling.offset(3).y, 500)
    }

    func testNestedScrollRefreshesCachedAncestorAndHitRegion() throws {
        _ = NSApplication.shared
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 200)
        renderer.view.frame = NSRect(x: 0, y: 0, width: 200, height: 180)
        defer { renderer.destroy() }
        func send(_ ops: [[String: Any]]) throws {
            renderer.send(String(data: try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops]), encoding: .utf8)!)
        }
        renderer.load()
        var ops: [[String: Any]] = [
            ["op": "el", "id": 1, "tag": "body", "attrs": [], "adopt": "body"],
            ["op": "el", "id": 2, "tag": "style", "attrs": []],
            ["op": "css", "id": 2, "text": "body{margin:0}#outer{width:120px;height:100px;overflow:auto}#inner{height:100px;overflow:auto}.space{height:200px}.row{height:20px}"],
            ["op": "ins", "id": 2, "parent": 1],
            ["op": "el", "id": 3, "tag": "div", "attrs": [["id", "outer"]]], ["op": "ins", "id": 3, "parent": 1],
            ["op": "el", "id": 4, "tag": "div", "attrs": [["class", "space"]]], ["op": "ins", "id": 4, "parent": 3],
            ["op": "el", "id": 5, "tag": "div", "attrs": [["id", "inner"]]], ["op": "ins", "id": 5, "parent": 3],
            ["op": "el", "id": 6, "tag": "div", "attrs": [["class", "space"]]], ["op": "ins", "id": 6, "parent": 3],
        ]
        for row in 0..<30 {
            ops.append(["op": "el", "id": 10 + row, "tag": "div", "attrs": [["class", "row"], ["style", "background:\(row % 2 == 0 ? "#f00" : "#00f")"]]])
            ops.append(["op": "ins", "id": 10 + row, "parent": 5])
        }
        try send(ops)
        pump(1, until: { renderer.scrolling.layers[3]?.rasterCount ?? 0 > 0 })
        let layer = try XCTUnwrap(renderer.scrolling.layers[3])
        XCTAssertNil(renderer.scrolling.layers[5])
        try send([["op": "prop", "id": 3, "name": "scrollTop", "value": 200]])
        pump(1, until: { renderer.scrolling.offset(3).y == 200 })
        renderer.viewResized()
        XCTAssertEqual(renderer.scrolling.geometries[5]?.viewport.minY, 0)
        func tileRed() -> Int? {
            guard let tile = layer.content.sublayers?.first(where: { $0.frame.contains(CGPoint(x: 100, y: 250)) }), let image = tile.contents else { return nil }
            return red(image as! CGImage, 250 - Int(tile.frame.minY), scale: Int(tile.contentsScale))
        }
        pump(1, until: { tileRed() == 255 })
        XCTAssertEqual(tileRed(), 255)
        XCTAssertTrue(renderer.handleScroll(CGPoint(x: 50, y: 50), event: wheel(-20)))
        XCTAssertEqual(renderer.scrolling.offset(5).y, 20)
        XCTAssertEqual(renderer.scrolling.offset(3).y, 200)
        pump(1, until: { tileRed() == 0 })
        XCTAssertEqual(tileRed(), 0)
    }

    func testInvalidatedPendingScrollTileRetriesWithLatestDocument() throws {
        let before = LHDocument(html: "<body style='margin:0'><div id='3' style='width:120px;height:100px;overflow:auto'><div style='height:500px;background:red'></div></div></body>", css: nil)
        let after = LHDocument(html: "<body style='margin:0'><div id='3' style='width:120px;height:100px;overflow:auto'><div style='height:500px;background:blue'></div></div></body>", css: nil)
        for document in [before, after] { _ = document.layout(maxWidth: 120, preferredWidth: 120) }
        let geometry = WidgetScrollGeometry(viewport: CGRect(x: 0, y: 0, width: 120, height: 100), content: CGSize(width: 120, height: 500), radii: [])
        let layer = WidgetScrollLayer(id: 3, geometry: geometry)
        let gate = DispatchSemaphore(value: 0)
        WidgetScrollLayer.queue.async { gate.wait() }
        layer.update(signature: 1)
        layer.ensureTiles(document: before, scale: 1, attribute: "id")
        layer.update(signature: 2)
        layer.ensureTiles(document: after, scale: 1, attribute: "id")
        gate.signal()
        pump(2, until: { layer.rasterCount >= layer.tileCount })
        XCTAssertGreaterThan(layer.tileCount, 0)
        XCTAssertEqual(layer.rasterCount, layer.tileCount)
        for tile in layer.content.sublayers ?? [] {
            let image = try XCTUnwrap(tile.contents) as! CGImage
            XCTAssertEqual(red(image, 10, scale: 1), 0)
        }
    }

    func testScrollHitViewportRespectsEveryClippingAncestor() throws {
        for clipping in ["overflow:hidden", "overflow:clip", "contain:paint"] {
            let document = LHDocument(html: "<body style='margin:0'><div style='width:120px;height:100px;border:4px solid red;\(clipping)'><div style='width:200px;height:160px;overflow:hidden;margin:10px'><div id='list' style='width:240px;height:200px;overflow:auto'><div style='height:500px'></div></div></div></div></body>", css: nil)
            _ = document.layout(maxWidth: 400, preferredWidth: 400)
            let info = try XCTUnwrap(document.scrollers(byAttribute: "id")["list"])
            XCTAssertEqual((info["viewport"] as? NSValue)?.rectValue, CGRect(x: 14, y: 14, width: 240, height: 200))
            XCTAssertEqual((info["hitViewport"] as? NSValue)?.rectValue, CGRect(x: 14, y: 14, width: 110, height: 90), clipping)
        }
    }

    func testFixedScrollerEscapesOrdinaryAncestorOverflowClip() throws {
        let document = LHDocument(html: "<body style='margin:0'><div style='width:50px;height:50px;overflow:hidden'><div id='list' style='position:fixed;left:150px;top:150px;width:100px;height:100px;overflow:auto'><div style='height:500px'></div></div></div></body>", css: nil)
        _ = document.layout(maxWidth: 400, preferredWidth: 400)
        let info = try XCTUnwrap(document.scrollers(byAttribute: "id")["list"])
        XCTAssertEqual((info["hitViewport"] as? NSValue)?.rectValue, (info["viewport"] as? NSValue)?.rectValue)
    }

    func testWheelOutsideClippedScrollerLeavesItsOffsetUnchanged() throws {
        _ = NSApplication.shared
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
        renderer.view.frame = NSRect(x: 0, y: 0, width: 400, height: 300)
        defer { renderer.destroy() }
        renderer.load()
        let ops: [[String: Any]] = [
            ["op": "el", "id": 1, "tag": "body", "attrs": [], "adopt": "body"],
            ["op": "el", "id": 2, "tag": "style", "attrs": []],
            ["op": "css", "id": 2, "text": "body{margin:0}#clip{width:120px;height:100px;overflow:hidden;border:4px solid red}#list{width:240px;height:200px;overflow:auto}#rows{height:800px}"],
            ["op": "ins", "id": 2, "parent": 1],
            ["op": "el", "id": 3, "tag": "div", "attrs": [["id", "clip"]]], ["op": "ins", "id": 3, "parent": 1],
            ["op": "el", "id": 4, "tag": "div", "attrs": [["id", "list"]]], ["op": "ins", "id": 4, "parent": 3],
            ["op": "el", "id": 5, "tag": "div", "attrs": [["id", "rows"]]], ["op": "ins", "id": 5, "parent": 4],
        ]
        renderer.send(String(data: try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops]), encoding: .utf8)!)
        pump(1, until: { renderer.scrolling.geometries[4] != nil })
        XCTAssertEqual(renderer.scrolling.hitViewports[4], CGRect(x: 4, y: 4, width: 120, height: 100))
        XCTAssertFalse(renderer.handleScroll(CGPoint(x: 200, y: 50), event: wheel(-20)))
        XCTAssertFalse(renderer.handleScroll(CGPoint(x: 50, y: 150), event: wheel(-20)))
        XCTAssertEqual(renderer.scrolling.offset(4), .zero)
        XCTAssertTrue(renderer.handleScroll(CGPoint(x: 50, y: 50), event: wheel(-20)))
        XCTAssertEqual(renderer.scrolling.offset(4).y, 20)
    }

    func testClipOverflowDoesNotBecomeAScrollContainerForStickyContent() throws {
        let document = LHDocument(html: "<body style='margin:0'><div id='outer' style='width:120px;height:100px;overflow:auto'><div id='clip' style='overflow:clip;height:300px'><div id='sticky' style='position:sticky;top:0;height:20px'>sticky</div><div style='height:500px'></div></div></div></body>", css: nil)
        _ = document.layout(maxWidth: 200, preferredWidth: 200)
        XCTAssertNil(document.scrollers(byAttribute: "id")["clip"])
        XCTAssertEqual(document.setScrollOffset(CGPoint(x: 0, y: 50), attribute: "id", value: "clip"), .zero)
        XCTAssertEqual(document.setScrollOffset(CGPoint(x: 0, y: 50), attribute: "id", value: "outer").y, 50)
        XCTAssertEqual(document.bounds(byAttribute: "id")["sticky"]?.rectValue.minY, 0)
    }

    func testFixedScrollerInsideTransformStaysWithinOuterClip() throws {
        let document = LHDocument(html: "<body style='margin:0'><div style='width:50px;height:50px;overflow:hidden'><div style='transform:translateX(0)'><div id='list' style='position:fixed;left:150px;top:150px;width:100px;height:100px;overflow:auto'><div style='height:500px'></div></div></div></div></body>", css: nil)
        _ = document.layout(maxWidth: 400, preferredWidth: 400)
        let info = try XCTUnwrap(document.scrollers(byAttribute: "id")["list"])
        XCTAssertEqual((info["hitViewport"] as? NSValue)?.rectValue, .zero)
    }

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
