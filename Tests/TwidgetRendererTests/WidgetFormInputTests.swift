import XCTest
import AppKit
@testable import TwidgetRenderer

final class WidgetFormInputTests: XCTestCase {
    private func pump(_ seconds: TimeInterval, until done: () -> Bool = { false }) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done() && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
    }

    private func make(_ ops: [[String: Any]]) -> (LiteHTMLWidgetRenderer, NSWindow, () -> [[String: Any]]) {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.orderFrontRegardless()
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
        var events: [[String: Any]] = []
        renderer.onMessage = { json in
            if let object = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any], object["type"] as? String == "event", let ev = object["ev"] as? [String: Any] {
                events.append(ev)
            }
        }
        renderer.view.frame = NSRect(x: 0, y: 0, width: 400, height: 300)
        window.contentView?.addSubview(renderer.view)
        renderer.load()
        let base: [[String: Any]] = [["op": "el", "id": 2, "tag": "html", "attrs": [], "adopt": "html"], ["op": "el", "id": 7, "tag": "body", "attrs": [], "adopt": "body"]]
        let data = try! JSONSerialization.data(withJSONObject: ["type": "ops", "ops": base + ops])
        renderer.send(String(data: data, encoding: .utf8)!)
        pump(2, until: { renderer.elementBounds().count > 2 })
        return (renderer, window, { events })
    }

    private func click(_ renderer: LiteHTMLWidgetRenderer, _ id: Int) {
        let box = renderer.elementBounds()[id] ?? .zero
        let point = CGPoint(x: box.midX, y: box.midY)
        let event = NSEvent.mouseEvent(with: .leftMouseDown, location: renderer.view.convert(point, to: nil), modifierFlags: [], timestamp: 0, windowNumber: renderer.view.window?.windowNumber ?? 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
        renderer.handleMouseDown(point, event: event)
    }

    func testChoosingFromTheSelectMenuSetsTheValueAndDispatchesInputAndChange() {
        let previous = WidgetRendererEnvironment.presentMenu
        defer { WidgetRendererEnvironment.presentMenu = previous }
        var shown: [WidgetMenuItem] = []
        WidgetRendererEnvironment.presentMenu = { items, _, _, _, completion in
            shown = items
            completion(items.firstIndex { $0.title == "Blue" })
        }
        let (renderer, window, events) = make([
            ["op": "el", "id": 10, "tag": "select", "attrs": []], ["op": "ins", "parent": 7, "id": 10, "before": NSNull()],
            ["op": "el", "id": 11, "tag": "option", "attrs": [["value", "r"]]], ["op": "ins", "parent": 10, "id": 11, "before": NSNull()],
            ["op": "text", "id": 12, "data": "Red"], ["op": "ins", "parent": 11, "id": 12, "before": NSNull()],
            ["op": "el", "id": 13, "tag": "optgroup", "attrs": [["label", "Cool"]]], ["op": "ins", "parent": 10, "id": 13, "before": NSNull()],
            ["op": "el", "id": 14, "tag": "option", "attrs": [["value", "g"], ["disabled", ""]]], ["op": "ins", "parent": 13, "id": 14, "before": NSNull()],
            ["op": "text", "id": 15, "data": "Green"], ["op": "ins", "parent": 14, "id": 15, "before": NSNull()],
            ["op": "el", "id": 16, "tag": "option", "attrs": [["value", "b"]]], ["op": "ins", "parent": 13, "id": 16, "before": NSNull()],
            ["op": "text", "id": 17, "data": "Blue"], ["op": "ins", "parent": 16, "id": 17, "before": NSNull()],
        ])
        defer { renderer.destroy(); window.orderOut(nil) }
        click(renderer, 10)
        pump(2, until: { events().contains { $0["type"] as? String == "change" } })
        XCTAssertEqual(shown.map { $0.kind }, [.option, .separator, .header, .option, .option])
        XCTAssertEqual(shown.filter { $0.kind == .option }.map { $0.enabled }, [true, false, true])
        XCTAssertEqual(shown.first?.selected, true)
        let form = events().filter { ["input", "change"].contains($0["type"] as? String) }
        XCTAssertEqual(form.map { $0["type"] as? String }, ["input", "change"])
        XCTAssertEqual(form.last?["value"] as? String, "b")
        XCTAssertEqual(form.last?["selectedIndex"] as? Int, 2)
        XCTAssertEqual(WidgetDOM.selectedIndex(of: renderer.dom.node(10)!), 2)
        XCTAssertTrue(renderer.dom.serialize(theme: "light").contains("value=\"b\" selected"))
    }

    func testTypingIntoAnInputUpdatesTheDOMAndDispatchesInputThenSubmitAndChangeOnBlur() {
        let (renderer, window, events) = make([
            ["op": "el", "id": 20, "tag": "form", "attrs": []], ["op": "ins", "parent": 7, "id": 20, "before": NSNull()],
            ["op": "el", "id": 21, "tag": "input", "attrs": [["placeholder", "Name"], ["maxlength", "8"]]], ["op": "ins", "parent": 20, "id": 21, "before": NSNull()],
            ["op": "el", "id": 22, "tag": "input", "attrs": [["type", "password"]]], ["op": "ins", "parent": 20, "id": 22, "before": NSNull()],
        ])
        defer { renderer.destroy(); window.orderOut(nil) }
        click(renderer, 22)
        pump(0.2)
        XCTAssertNil((renderer.view as? WidgetDocumentView)?.formEditor)
        click(renderer, 21)
        let view = renderer.view as! WidgetDocumentView
        pump(2, until: { view.formEditor != nil })
        guard let field = view.formEditor?.field else {
            return XCTFail("no editor")
        }
        XCTAssertTrue(field.superview === view)
        XCTAssertFalse(renderer.dom.serialize(theme: "light").contains("placeholder"))
        field.stringValue = "hello"
        view.formEditor?.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification))
        field.stringValue = "hello world!"
        view.formEditor?.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification))
        XCTAssertEqual(field.stringValue, "hello")
        pump(1, until: { events().contains { $0["type"] as? String == "input" } })
        XCTAssertEqual(renderer.dom.node(21)?.formValue, "hello")
        let input = events().first { $0["type"] as? String == "input" }
        XCTAssertEqual(input?["targetId"] as? Int, 21)
        XCTAssertEqual(input?["value"] as? String, "hello")
        renderer.editorCommand(generation: renderer.form.generation, command: "Enter")
        XCTAssertEqual(events().last?["type"] as? String, "submit")
        XCTAssertEqual(events().last?["targetId"] as? Int, 20)
        renderer.editorCommand(generation: renderer.form.generation, command: "Escape")
        pump(1, until: { view.formEditor == nil })
        XCTAssertNil(view.formEditor)
        XCTAssertNil(field.superview)
        let tail = events().map { $0["type"] as? String ?? "" }.filter { ["change", "blur"].contains($0) }
        XCTAssertEqual(tail, ["change", "blur"])
        let html = renderer.dom.serialize(theme: "light")
        XCTAssertTrue(html.contains("value=\"hello\""), html)
        XCTAssertTrue(html.contains("placeholder=\"Name\""), html)
    }
}
