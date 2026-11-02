import XCTest
import AppKit
import LiteHTML
import WidgetSandbox
import WidgetSandboxProtocol
@testable import TwidgetRenderer

final class WidgetPaintPolicyTests: XCTestCase {
    private func pump(_ seconds: TimeInterval, until done: () -> Bool) {
        let end = Date().addingTimeInterval(seconds)
        while !done() && Date() < end { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    }

    func fixture() throws -> [String: Any] {
        let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let path = repository.deletingLastPathComponent().appendingPathComponent("widget-sandbox/packages/core/tests/fixtures/paint-policy.json")
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: path)) as? [String: Any])
    }

    func testSharedFixture() throws {
        let data = try fixture()
        let constants = try XCTUnwrap(data["constants"] as? [String: Int])
        XCTAssertEqual(constants["PAINT_HEAVY_BOXES"], WidgetPaintLimit.maxHeavyBoxes)
        XCTAssertEqual(constants["PAINT_BACKDROP_BOXES"], WidgetPaintLimit.maxBackdropBoxes)
        func paint(_ value: Any?) -> WidgetBoxPaint? {
            guard let value = value as? [String: Bool], let heavy = value["heavy"], let backdrop = value["backdrop"] else { return nil }
            return WidgetBoxPaint(heavy: heavy, backdrop: backdrop)
        }
        for item in data["shadows"] as? [[String: Any]] ?? [] {
            let value = try XCTUnwrap(item["value"] as? String)
            XCTAssertEqual(WidgetPaintPolicy.shadowBlurred(value), item["blurred"] as? Bool, value)
            XCTAssertEqual(WidgetPaintPolicy.withoutBlur(value), item["withoutBlur"] as? String, value)
        }
        for item in data["boxes"] as? [[String: Any]] ?? [] {
            XCTAssertEqual(WidgetPaintPolicy.box(item["style"] as? [String: Any] ?? [:], pseudo: item["pseudo"] as? Bool ?? false), paint(item["expect"]), item["name"] as? String ?? "")
        }
        for item in data["animations"] as? [[String: Any]] ?? [] {
            XCTAssertEqual(WidgetPaintPolicy.animation(item["keyframes"] as? [[String: Any]] ?? []), paint(item["expect"]), item["name"] as? String ?? "")
        }
        for item in data["cases"] as? [[String: Any]] ?? [] {
            let counter = WidgetPaintCounter()
            for step in item["steps"] as? [[String: Any]] ?? [] {
                func keys(_ name: String) -> [String] { (step[name] as? [String]) ?? (step[name] as? String).map { [$0] } ?? [] }
                for key in keys("set") { counter.set(key, WidgetPaintPolicy.box(step["style"] as? [String: Any] ?? [:], pseudo: step["pseudo"] as? Bool ?? false)) }
                for key in keys("remove") { counter.remove(key) }
                for key in keys("start") {
                    if let value = WidgetPaintPolicy.animation(step["keyframes"] as? [[String: Any]] ?? []) { counter.start(key, id: step["animation"] as? String ?? "", paint: value) }
                }
                for key in keys("end") { counter.end(key, id: step["animation"] as? String ?? "") }
                if let expected = step["check"] as? [String: Any] {
                    let name = item["name"] as? String ?? ""
                    XCTAssertEqual(counter.counts.heavy, expected["heavy"] as? Int, name)
                    XCTAssertEqual(counter.counts.backdrop, expected["backdrop"] as? Int, name)
                    let verdict = counter.check()
                    let stop = expected["stop"] as? [String: Any]
                    XCTAssertEqual(verdict?.code.rawValue, stop?["code"] as? String, name)
                    XCTAssertEqual(verdict?.count, stop?["count"] as? Int, name)
                }
            }
        }
    }

    func testComputedCascadeIncludesPseudoAndExcludesHiddenDescendantsAndDisabledButtons() {
        let html = """
        <html data-tw-id="1"><body data-tw-id="2">
        <div data-tw-id="3" class="heavy"></div>
        <div data-tw-id="4" style="display:none"><div data-tw-id="5" class="heavy"></div></div>
        <button data-tw-id="6" disabled style="opacity:.5"></button>
        <div data-tw-id="7" style="visibility:hidden;opacity:0"></div>
        </body></html>
        """
        let document = LHDocument(html: html, css: ".heavy{filter:brightness(1)}.heavy::before{content:'';backdrop-filter:blur(1px)}")
        _ = document.layout(maxWidth: 390, preferredWidth: 390)
        let boxes = document.paintBoxes(attribute: "data-tw-id", declarations: [])
        XCTAssertNil(boxes["4"])
        XCTAssertNil(boxes["5"])
        XCTAssertEqual(boxes["3"]?["heavy"] as? Bool, true)
        XCTAssertEqual(boxes["3::before"]?["backdrop"] as? Bool, true)
        XCTAssertEqual(boxes["6"]?["heavy"] as? Bool, false)
        XCTAssertEqual(boxes["7"]?["heavy"] as? Bool, true)
        document.suppressPaintEffects()
        XCTAssertTrue(document.paintBoxes(attribute: "data-tw-id", declarations: []).values.allSatisfy { ($0["heavy"] as? Bool) == false })
    }

    func testRendererStopsOnceAndCompatDoesNotCount() throws {
        for policy in ["strict", "compat"] {
            let html = "<style>.a{opacity:.99;width:10px;height:10px}</style>" + String(repeating: "<div class='a'></div>", count: 17)
            let session = try ConformanceRunnerTests.Session(html: html, theme: "light", features: ["webAnimations"], configure: { $0.policy = policy == "compat" ? .compat : .strict })
            defer { session.close() }
            pump(5, until: { session.host.diagnostics.dead != nil || session.host.diagnostics.loaded })
            if policy == "strict" {
                XCTAssertTrue(session.host.diagnostics.dead?.hasPrefix("Paint limit exceeded: 17") == true)
                XCTAssertTrue(session.probe.inner.frozen)
                XCTAssertNotNil(session.probe.inner.snapshot())
            } else {
                XCTAssertNil(session.host.diagnostics.dead)
                XCTAssertEqual(session.probe.inner.paintCounts.heavy, 0)
            }
        }
    }

    func testLargeTaskRendersAfterAllChunks() throws {
        let html = "<script>for(let i=0;i<600;i++){const d=document.createElement('div');d.textContent='row '+i;document.body.appendChild(d)}</script>"
        let session = try ConformanceRunnerTests.Session(html: html, theme: "light", features: [])
        defer { session.close() }
        pump(10, until: { session.host.diagnostics.loaded || session.host.diagnostics.dead != nil })
        XCTAssertNil(session.host.diagnostics.dead)
        XCTAssertGreaterThan(session.probe.opCount, 2000)
        XCTAssertLessThanOrEqual(session.probe.maxBatchOps, 1024)
        XCTAssertLessThan(session.probe.inner.layoutCount, 6)
        print("CHUNKED_TASK ops=\(session.probe.opCount) maxBatch=\(session.probe.maxBatchOps) layouts=\(session.probe.inner.layoutCount)")
    }

    func testPaintScanCostOnLargeDocument() {
        let html = "<html data-tw-id='1'><body data-tw-id='2'>" + (3...1002).map { "<div data-tw-id='\($0)' style='width:10px;height:10px'>x</div>" }.joined() + "</body></html>"
        let document = LHDocument(html: html, css: nil)
        _ = document.layout(maxWidth: 390, preferredWidth: 390)
        let start = ProcessInfo.processInfo.systemUptime
        for _ in 0..<30 { _ = document.paintBoxes(attribute: "data-tw-id", declarations: WidgetAnimator.watched) }
        let milliseconds = (ProcessInfo.processInfo.systemUptime - start) * 1000 / 30
        print("PAINT_SCAN 1002 elements: \(milliseconds) ms per scan")
        XCTAssertLessThan(milliseconds, 50)
    }

    func testStoppedFrameKeepsRingsAndHidesZeroOpacitySubtrees() {
        let html = "<html><body><div data-lh-id='1' style='opacity:0'><div data-lh-id='2' style='visibility:visible;width:40px;height:40px;background:red'></div></div><div data-lh-id='3' style='width:60px;height:30px;opacity:.4;filter:brightness(1);backdrop-filter:blur(1px);box-shadow:0 0 0 2px red,0 2px 4px blue'></div></body></html>"
        let document = LHDocument(html: html, css: nil)
        document.suppressPaintEffects()
        _ = document.layout(maxWidth: 390, preferredWidth: 390)
        for id in ["1", "2"] {
            XCTAssertEqual(document.computedStyle(attribute: WidgetDOM.idAttribute, value: id)["visibility"], "hidden")
        }
        let style = document.computedStyle(attribute: WidgetDOM.idAttribute, value: "3")
        XCTAssertEqual(style["opacity"], "1")
        XCTAssertEqual(style["filter"], "none")
        XCTAssertEqual(style["backdrop-filter"], "none")
        XCTAssertNotEqual(style["box-shadow"], "none")
        XCTAssertFalse(WidgetPaintPolicy.shadowBlurred(style["box-shadow"] ?? ""))
    }

    func testChunkedTaskChecksOnlyItsFinalCascade() throws {
        _ = NSApplication.shared
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 390)
        renderer.schedule = { $0() }
        defer { renderer.destroy() }
        var limits: [String] = []
        renderer.onMessage = { if $0.contains("\"type\":\"limit\"") { limits.append($0) } }
        func send(_ ops: [[String: Any]]) throws {
            renderer.send(String(decoding: try JSONSerialization.data(withJSONObject: ["type": "ops", "ops": ops]), as: UTF8.self))
        }
        renderer.load()
        let count = renderer.layoutCount
        renderer.beginTask()
        try send([["op": "el", "id": 1, "tag": "body", "adopt": "body"]])
        for id in 2...18 {
            try send([["op": "el", "id": id, "tag": "div", "attrs": [["style", "opacity:.8;height:5px"]]], ["op": "ins", "id": id, "parent": 1]])
        }
        try send([["op": "style", "id": 18, "name": "opacity", "value": "1"]])
        XCTAssertEqual(renderer.layoutCount, count)
        renderer.endTask()
        XCTAssertEqual(renderer.layoutCount, count + 1)
        XCTAssertEqual(renderer.paintCounts.heavy, 16)
        XCTAssertTrue(limits.isEmpty)
        try send([["op": "style", "id": 18, "name": "opacity", "value": ".5"]])
        XCTAssertEqual(limits.count, 1)
        renderer.setTheme("dark")
        renderer.setNeedsRender()
        XCTAssertEqual(limits.count, 1)
        XCTAssertTrue(renderer.document?.paintBoxes(attribute: WidgetDOM.idAttribute, declarations: []).values.allSatisfy { ($0["heavy"] as? Bool) == false } == true)
    }

    func testThemeAndWidthChangesRecheckPaint() throws {
        for trigger in ["theme", "width"] {
            let condition = trigger == "theme" ? "(prefers-color-scheme:dark)" : "(max-width:350px)"
            let html = "<style>@media \(condition){i{opacity:.8}}i{display:block;height:5px}</style>" + String(repeating: "<i></i>", count: 17)
            let session = try ConformanceRunnerTests.Session(html: html, theme: "light", features: [])
            defer { session.close() }
            pump(5, until: { session.host.diagnostics.loaded || session.host.diagnostics.dead != nil })
            XCTAssertNil(session.host.diagnostics.dead)
            if trigger == "theme" { session.setTheme("dark") }
            else { session.probe.inner.view.setFrameSize(CGSize(width: 320, height: 200)) }
            pump(5, until: { session.host.diagnostics.dead != nil })
            XCTAssertTrue(session.host.diagnostics.dead?.hasPrefix("Paint limit exceeded: 17") == true, "\(trigger) counts=\(session.probe.inner.paintCounts)")
        }
    }

    func testColorSchemeMediaRestylesNativeDocument() {
        let document = LHDocument(html: "<html><body><i data-lh-id='1'>x</i></body></html>", css: "@media(prefers-color-scheme:dark){i{opacity:.5}}")
        _ = document.layout(maxWidth: 390, preferredWidth: 390)
        XCTAssertEqual(document.computedStyle(attribute: WidgetDOM.idAttribute, value: "1")["opacity"], "1")
        document.setDarkColorScheme(true)
        _ = document.layout(maxWidth: 390, preferredWidth: 390)
        XCTAssertEqual(document.computedStyle(attribute: WidgetDOM.idAttribute, value: "1")["opacity"], "0.5")
        document.setDarkColorScheme(false)
        _ = document.layout(maxWidth: 390, preferredWidth: 390)
        XCTAssertEqual(document.computedStyle(attribute: WidgetDOM.idAttribute, value: "1")["opacity"], "1")
    }
}
