import AppKit
import LiteHTML
import XCTest
import WidgetSandbox
@testable import TwidgetRenderer

final class WidgetGeometryRegressionTests: XCTestCase {
    private let width: CGFloat = 320

    private func prepare(_ html: String) throws -> WidgetPrepared {
        _ = NSApplication.shared
        let pool = WidgetShellPool.shared
        let previousFactory = pool.factory
        pool.factory = { width in
            let renderer = LiteHTMLWidgetRenderer(theme: "light", width: width)
            renderer.preparesOffscreen = true
            return renderer
        }
        pool.prewarm(width: width)
        defer { pool.factory = previousFactory }
        let deadline = Date().addingTimeInterval(5)
        while !pool.available(width: width) && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
        var options = WidgetHost.Options()
        options.nativeScrolling = true
        let prepared = try XCTUnwrap(WidgetPreparer.prepare(html: html, theme: WidgetTheme(colorScheme: .light), width: width, options: options, tracked: false))
        let readyDeadline = Date().addingTimeInterval(5)
        while !prepared.isReady && Date() < readyDeadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
        XCTAssertTrue(prepared.isReady)
        XCTAssertNil(prepared.host.diagnostics.dead)
        return prepared
    }

    func testOversizedWidgetPreparesWithoutTrapping() throws {
        for style in ["height:1e38px", "height:100000000px", "margin-top:1e38px",
                      "padding-bottom:1e30px", "font-size:1e38px", "line-height:1e308",
                      "height:9223372036854775808px"] {
            let prepared = try prepare(#"<div style="\#(style)">x</div>"#)
            defer { prepared.discard() }
            XCTAssertTrue(prepared.height.isFinite, style)
            XCTAssertGreaterThan(prepared.height, 0, style)
        }
    }

    func testRasterDimensionsAreCheckedBeforeIntegerConversion() {
        XCTAssertEqual(widgetRasterPixelSize(width: 320, height: 8192, scale: 2), CGSize(width: 640, height: 16384))
        XCTAssertNil(widgetRasterPixelSize(width: 320, height: 8192.1, scale: 2))
        XCTAssertNil(widgetRasterPixelSize(width: .infinity, height: 60, scale: 2))
        XCTAssertNil(widgetRasterPixelSize(width: 320, height: .nan, scale: 2))
        XCTAssertNil(widgetRasterPixelSize(width: 320, height: 60, scale: 0))
        XCTAssertNil(WidgetScrollLayer.tileGrid(for: CGSize(width: 100, height: 1e38)))
        XCTAssertNil(WidgetScrollLayer.tileGrid(for: CGSize(width: 1e15, height: 1e15)))
        XCTAssertEqual(WidgetScrollLayer.tileGrid(for: CGSize(width: 512, height: 100_000))?.rows, 391)
    }

    func testOversizedScrollContentPreparesWithoutTrapping() throws {
        let prepared = try prepare(#"<div style="overflow:auto;height:60px"><div style="height:1e38px">x</div></div>"#)
        defer { prepared.discard() }
        XCTAssertGreaterThanOrEqual(prepared.height, 60)
    }

    func testInvalidScrollerPreparesWithoutTrapping() throws {
        let prepared = try prepare(#"<div style="overflow:auto;height:60px;margin-top:1e308px"><p>a</p><p>b</p><p>c</p><p>d</p><p>e</p></div>"#)
        defer { prepared.discard() }
        XCTAssertTrue(prepared.height.isFinite)
        XCTAssertGreaterThan(prepared.height, 0)
    }

    func testSVGWithInvalidFrameFallsBackToDocumentPainting() throws {
        let attribute = WidgetDOM.idAttribute
        func layout(_ margin: String) -> LHDocument {
            let html = #"<div style="margin-top:\#(margin)"><svg \#(attribute)="7" width="20" height="20"><rect width="20" height="20" fill="red"/></svg></div>"#
            let document = LHDocument(html: html, css: "", previous: nil)
            _ = document.layout(maxWidth: width, preferredWidth: width)
            return document
        }
        let host = CALayer()
        let layers = WidgetSVGLayers()
        let dom = WidgetDOM()
        for _ in 0..<WidgetSVGLayers.streakNeeded {
            _ = layers.observe([7])
        }
        XCTAssertFalse(layers.render(document: layout("8px"), width: width, host: host, scale: 2, dom: dom, composited: []))
        XCTAssertEqual(host.sublayers?.count, 1)
        let invalid = layout("1e308px")
        let shapes = try XCTUnwrap(invalid.svgShapes(attribute: attribute, value: "7", maxWidth: width))
        XCTAssertFalse(shapes.frame.origin.y.isFinite)
        XCTAssertTrue(layers.render(document: invalid, width: width, host: host, scale: 2, dom: dom, composited: []))
        XCTAssertTrue(host.sublayers?.isEmpty ?? true)
        XCTAssertFalse(layers.ids.contains(7))
    }

    func testValidUpdateRecoversAfterOversizedLayout() throws {
        _ = NSApplication.shared
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: width)
        defer { renderer.destroy() }
        var scheduled: [() -> Void] = []
        renderer.schedule = { scheduled.append($0) }
        func send(_ ops: [[String: Any]]) throws {
            let json = try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops])
            renderer.send(String(decoding: json, as: UTF8.self))
            while !scheduled.isEmpty {
                scheduled.removeFirst()()
            }
        }

        renderer.load()
        try send([
            ["op": "el", "id": 1, "tag": "body", "attrs": [], "adopt": "body"],
            ["op": "el", "id": 2, "tag": "div", "attrs": [["style", "height:1e38px"]]],
            ["op": "ins", "id": 2, "parent": 1],
        ])
        XCTAssertEqual(renderer.document?.size.height, 1)
        try send([["op": "attr", "id": 2, "name": "style", "value": "height:100px"]])
        XCTAssertGreaterThanOrEqual(try XCTUnwrap(renderer.document?.size.height), 100)
    }

    func testControlsPrepareAndRender() throws {
        let block = try prepare(#"<div style="height:100px;background:#2a9d8f">x</div>"#)
        defer { block.discard() }
        XCTAssertGreaterThanOrEqual(block.height, 100)
        let scroll = try prepare(#"<div style="overflow:auto;height:60px;margin-top:8px"><p>a</p><p>b</p><p>c</p><p>d</p><p>e</p></div>"#)
        defer { scroll.discard() }
        XCTAssertGreaterThanOrEqual(scroll.height, 68)
    }
}
