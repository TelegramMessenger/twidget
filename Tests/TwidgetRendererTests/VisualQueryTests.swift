import XCTest
@testable import TwidgetRenderer

final class VisualQueryTests: XCTestCase {
    private func send(_ renderer: LiteHTMLWidgetRenderer, _ value: [String: Any]) {
        renderer.send(String(data: try! JSONSerialization.data(withJSONObject: value), encoding: .utf8)!)
    }

    private func make() -> LiteHTMLWidgetRenderer {
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
        renderer.schedule = { _ in }
        renderer.load()
        send(renderer, ["type": "ops", "ops": [
            ["op": "el", "id": 7, "tag": "body", "adopt": "body", "attrs": []],
            ["op": "el", "id": 8, "tag": "div", "attrs": [["style", "width:100px;height:60px;background:red"]]],
            ["op": "ins", "parent": 7, "id": 8]
        ]])
        return renderer
    }

    private func query(_ renderer: LiteHTMLWidgetRenderer, _ kind: String) -> [String: Any] {
        var result: [String: Any] = [:]
        renderer.onMessage = { json in
            let message = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any]
            if message?["type"] as? String == "result" { result = message?["result"] as? [String: Any] ?? [:] }
        }
        send(renderer, ["type": "query", "qid": 1, "query": ["q": kind, "id": 8]])
        return result
    }

    func testVisualWritesAndSizeReadsDoNotRenderUntilPaintIsNeeded() {
        let renderer = make()
        defer { renderer.destroy() }
        let count = renderer.layoutCount
        for (name, value) in [("color", "blue"), ("background-color", "green"), ("box-shadow", "0 2px 4px black"), ("opacity", "0.7"), ("transform", "translateY(20px)")] {
            send(renderer, ["type": "ops", "ops": [["op": "style", "id": 8, "name": name, "value": value]]])
            XCTAssertEqual(query(renderer, "box")["offsetHeight"] as? Double, 60)
            XCTAssertEqual(renderer.layoutCount, count, name)
        }
        _ = query(renderer, "rect")
        XCTAssertGreaterThan(renderer.layoutCount, count)
    }

    func testTextareaSizeQueriesFlushValuesAndRows() {
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
        renderer.schedule = { _ in }
        renderer.load()
        defer { renderer.destroy() }
        send(renderer, ["type": "ops", "ops": [
            ["op": "el", "id": 7, "tag": "body", "adopt": "body", "attrs": []],
            ["op": "el", "id": 8, "tag": "textarea", "attrs": [["style", "display:block;width:300px;height:auto;font:16px/20px monospace;padding:6px 10px;border:2px solid black"]]],
            ["op": "ins", "parent": 7, "id": 8],
            ["op": "prop", "id": 8, "name": "value", "value": "a"]
        ]])
        XCTAssertEqual(query(renderer, "box")["offsetHeight"] as? Double, 56)
        XCTAssertEqual(query(renderer, "box")["clientHeight"] as? Double, 52)
        send(renderer, ["type": "ops", "ops": [["op": "prop", "id": 8, "name": "value", "value": "one\ntwo\nthree\nfour\nfive"]]])
        XCTAssertEqual(query(renderer, "box")["offsetHeight"] as? Double, 56)
        XCTAssertEqual(query(renderer, "box")["scrollHeight"] as? Double, 112)
        send(renderer, ["type": "ops", "ops": [["op": "attr", "id": 8, "name": "rows", "value": "4"]]])
        XCTAssertEqual(query(renderer, "box")["offsetHeight"] as? Double, 96)
        XCTAssertEqual(query(renderer, "box")["clientHeight"] as? Double, 92)
        send(renderer, ["type": "ops", "ops": [
            ["op": "style", "id": 8, "name": "box-sizing", "value": "border-box"],
            ["op": "style", "id": 8, "name": "height", "value": "40px"]
        ]])
        XCTAssertEqual(query(renderer, "box")["clientHeight"] as? Double, 36)
        XCTAssertEqual(query(renderer, "box")["scrollHeight"] as? Double, 112)
        send(renderer, ["type": "ops", "ops": [["op": "prop", "id": 8, "name": "value", "value": ""]]])
        XCTAssertEqual(query(renderer, "box")["scrollHeight"] as? Double, 36)
    }

    func testLayoutMutationAfterVisualWritesStillFlushes() {
        let renderer = make()
        defer { renderer.destroy() }
        send(renderer, ["type": "ops", "ops": [
            ["op": "style", "id": 8, "name": "color", "value": "green"],
            ["op": "style", "id": 8, "name": "height", "value": "90px"]
        ]])
        XCTAssertEqual(query(renderer, "box")["offsetHeight"] as? Double, 90)
    }

    func testVisualWritesCoalesceAndEventuallyPaint() {
        let renderer = make()
        defer { renderer.destroy() }
        var scheduled: [() -> Void] = []
        renderer.schedule = { scheduled.append($0) }
        let count = renderer.layoutCount
        for color in ["blue", "green", "yellow"] {
            send(renderer, ["type": "ops", "ops": [["op": "style", "id": 8, "name": "color", "value": color]]])
        }
        XCTAssertEqual(renderer.layoutCount, count)
        XCTAssertEqual(scheduled.count, 1)
        scheduled.forEach { $0() }
        XCTAssertEqual(renderer.layoutCount, count + 1)
        XCTAssertEqual(query(renderer, "cs")["color"] as? String, "rgb(255, 255, 0)")
    }

    func testComputedStylesFlushPendingVisualWrites() {
        let renderer = make()
        defer { renderer.destroy() }
        let count = renderer.layoutCount
        send(renderer, ["type": "ops", "ops": [["op": "style", "id": 8, "name": "color", "value": "blue"]]])
        XCTAssertEqual(query(renderer, "cs")["color"] as? String, "rgb(0, 0, 255)")
        XCTAssertGreaterThan(renderer.layoutCount, count)
    }

    func testComputedLineHeightPreservesNormalAndResolvesLengths() {
        let renderer = make()
        defer { renderer.destroy() }
        XCTAssertEqual(query(renderer, "cs")["line-height"] as? String, "normal")
        send(renderer, ["type": "ops", "ops": [
            ["op": "style", "id": 8, "name": "font-size", "value": "20px"],
            ["op": "style", "id": 8, "name": "line-height", "value": "1.5"]
        ]])
        XCTAssertEqual(query(renderer, "cs")["line-height"] as? String, "30px")
    }

    func testVisualFeedbackAfterUserInputRendersImmediately() {
        let renderer = make()
        defer { renderer.destroy() }
        let count = renderer.layoutCount
        renderer.noteUserInput()
        send(renderer, ["type": "ops", "ops": [["op": "style", "id": 8, "name": "color", "value": "blue"]]])
        XCTAssertGreaterThan(renderer.layoutCount, count)
        XCTAssertEqual(query(renderer, "cs")["color"] as? String, "rgb(0, 0, 255)")
    }

    func testStyleAttributeSelectorsDisableTheVisualShortcut() {
        let renderer = make()
        defer { renderer.destroy() }
        send(renderer, ["type": "ops", "ops": [
            ["op": "el", "id": 9, "tag": "style", "attrs": []],
            ["op": "text", "id": 10, "data": "div[style*=blue]{height:120px!important}"],
            ["op": "ins", "parent": 9, "id": 10],
            ["op": "ins", "parent": 7, "id": 9]
        ]])
        let count = renderer.layoutCount
        send(renderer, ["type": "ops", "ops": [["op": "style", "id": 8, "name": "color", "value": "blue"]]])
        XCTAssertGreaterThan(renderer.layoutCount, count)
    }
}
