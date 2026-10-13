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

    func testRangeDragClampsSnapsAndCommitsOnceOutsideTheControl() {
        let (renderer, window, events) = make([
            ["op": "el", "id": 10, "tag": "input", "attrs": [["type", "range"], ["min", "10"], ["max", "90"], ["step", "10"], ["value", "50"], ["style", "width:200px"]]],
            ["op": "ins", "parent": 7, "id": 10]
        ])
        defer { renderer.destroy(); window.orderOut(nil) }
        click(renderer, 10)
        let event = NSEvent.mouseEvent(with: .leftMouseDragged, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
        let box = renderer.elementBounds()[10]!
        renderer.handleMouseDragged(CGPoint(x: box.minX + 130, y: box.midY), event: event)
        XCTAssertEqual(renderer.dom.node(10)?.formValue, "60")
        renderer.handleMouseDragged(CGPoint(x: -100, y: 200), event: event)
        XCTAssertEqual(renderer.dom.node(10)?.formValue, "10")
        renderer.handleMouseUp(CGPoint(x: 600, y: 200), event: event)
        XCTAssertEqual(renderer.dom.node(10)?.formValue, "90")
        XCTAssertEqual(events().filter { $0["type"] as? String == "change" }.count, 1)
        XCTAssertEqual(events().filter { $0["type"] as? String == "input" }.last?["value"] as? String, "90")
        renderer.handleMouseDragged(CGPoint(x: -100, y: 200), event: event)
        XCTAssertEqual(renderer.dom.node(10)?.formValue, "90")
    }

    func testDisabledRangeDoesNotChange() {
        let (renderer, window, events) = make([
            ["op": "el", "id": 10, "tag": "input", "attrs": [["type", "range"], ["disabled", ""], ["value", "25"]]],
            ["op": "ins", "parent": 7, "id": 10]
        ])
        defer { renderer.destroy(); window.orderOut(nil) }
        click(renderer, 10)
        renderer.finishRange(at: CGPoint(x: 500, y: 100))
        XCTAssertNil(renderer.dom.node(10)?.formValue)
        XCTAssertFalse(events().contains { $0["type"] as? String == "input" })
    }

    func testPointerCaptureDeliversReleaseOutsideAndResetsForNextGesture() {
        let (renderer, window, events) = make([
            ["op": "el", "id": 10, "tag": "div", "attrs": [["style", "width:100px;height:60px"]]],
            ["op": "ins", "parent": 7, "id": 10]
        ])
        defer { renderer.destroy(); window.orderOut(nil) }
        renderer.listened = ["pointerdown", "pointermove", "pointerup"]
        click(renderer, 10)
        renderer.send("{\"type\":\"ops\",\"ops\":[{\"op\":\"call\",\"id\":10,\"method\":\"setPointerCapture\",\"args\":[1]}]}")
        let event = NSEvent.mouseEvent(with: .leftMouseUp, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 0)!
        renderer.handleMouseDragged(CGPoint(x: 500, y: 500), event: event)
        renderer.handleMouseUp(CGPoint(x: 500, y: 500), event: event)
        XCTAssertEqual(events().last { $0["type"] as? String == "pointermove" }?["targetId"] as? Int, 10)
        let types = events().compactMap { $0["type"] as? String }
        XCTAssertLessThan(types.lastIndex(of: "pointermove")!, types.lastIndex(of: "pointerup")!)
        XCTAssertEqual(events().last { $0["type"] as? String == "pointerup" }?["targetId"] as? Int, 10)
        XCTAssertEqual(events().filter { $0["type"] as? String == "lostpointercapture" }.count, 1)
        renderer.handleMouseDown(CGPoint(x: 300, y: 200), event: event)
        renderer.handleMouseUp(CGPoint(x: 300, y: 200), event: event)
        XCTAssertNotEqual(events().last { $0["type"] as? String == "pointerup" }?["targetId"] as? Int, 10)
    }

    func testNumberInputKeepsItsColorsAndEditorWhileTyping() throws {
        let (renderer, window, events) = make([
            ["op": "el", "id": 10, "tag": "input", "attrs": [["type", "number"], ["placeholder", "Calories"], ["style", "color:rgb(220,230,240);background:#292929;padding:12px;border:0;border-radius:12px"]]],
            ["op": "ins", "parent": 7, "id": 10, "before": NSNull()],
        ])
        defer { renderer.destroy(); window.orderOut(nil) }
        window.appearance = NSAppearance(named: .darkAqua)
        click(renderer, 10)
        let view = renderer.view as! WidgetDocumentView
        pump(2, until: { view.formEditor?.field?.currentEditor() != nil })
        let editor = try XCTUnwrap(view.formEditor?.field?.currentEditor() as? NSTextView)
        let color = NSColor(srgbRed: 220 / 255.0, green: 230 / 255.0, blue: 240 / 255.0, alpha: 1)
        XCTAssertFalse(editor.drawsBackground)
        XCTAssertEqual(editor.backgroundColor.alphaComponent, 0)
        XCTAssertFalse(editor.usesAdaptiveColorMappingForDarkAppearance)
        if let clip = editor.superview as? NSClipView {
            XCTAssertFalse(clip.drawsBackground)
        }
        XCTAssertEqual(editor.textColor?.usingColorSpace(.sRGB), color)
        XCTAssertEqual(editor.insertionPointColor.usingColorSpace(.sRGB), color)
        editor.insertText("1", replacementRange: NSRange(location: NSNotFound, length: 0))
        pump(1, until: { events().contains { $0["type"] as? String == "input" } })
        XCTAssertEqual(renderer.dom.node(10)?.formValue, "1")
        XCTAssertTrue(window.firstResponder === editor)
        XCTAssertFalse(editor.drawsBackground)
        XCTAssertEqual(editor.textColor?.usingColorSpace(.sRGB), color)
    }

    func testEditorClipDoesNotInterceptButtonsOutsideTheTextField() throws {
        let (renderer, window, _) = make([
            ["op": "el", "id": 30, "tag": "div", "attrs": [["style", "height:180px;overflow:auto"]]],
            ["op": "ins", "id": 30, "parent": 7],
            ["op": "el", "id": 31, "tag": "textarea", "attrs": [["style", "display:block;width:200px;height:40px"]]],
            ["op": "ins", "id": 31, "parent": 30],
            ["op": "el", "id": 32, "tag": "button", "attrs": [["style", "display:block;width:100px;height:40px;margin-top:20px"]]],
            ["op": "ins", "id": 32, "parent": 30],
        ])
        defer { renderer.destroy(); window.orderOut(nil) }
        click(renderer, 31)
        let view = try XCTUnwrap(renderer.view as? WidgetDocumentView)
        pump(2, until: { view.formEditor != nil })
        XCTAssertNotNil(view.formEditor)
        let button = try XCTUnwrap(renderer.elementBounds()[32])
        let field = try XCTUnwrap(renderer.elementBounds()[31])
        let buttonPoint = view.convert(CGPoint(x: button.midX, y: button.midY), to: view.superview)
        let fieldPoint = view.convert(CGPoint(x: field.midX, y: field.midY), to: view.superview)
        XCTAssertTrue(view.hitTest(buttonPoint) === view)
        XCTAssertFalse(view.hitTest(fieldPoint) === view)
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
        shown = []
        let space = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: " ", charactersIgnoringModifiers: " ", isARepeat: false, keyCode: 49)!
        renderer.handleKey("keydown", event: WidgetKeyEvent(space))
        pump(1, until: { !shown.isEmpty })
        XCTAssertEqual(shown.first { $0.selected }?.title, "Blue")
    }

    func testAnEditorInsideAScrollerFollowsItsOffsetAndIsClippedToIt() {
        let (renderer, window, _) = make([
            ["op": "el", "id": 30, "tag": "div", "attrs": [["style", "overflow:auto;height:60px;margin-top:20px"]]], ["op": "ins", "parent": 7, "id": 30, "before": NSNull()],
            ["op": "el", "id": 31, "tag": "div", "attrs": [["style", "height:40px"]]], ["op": "ins", "parent": 30, "id": 31, "before": NSNull()],
            ["op": "el", "id": 32, "tag": "input", "attrs": [["value", "x"]]], ["op": "ins", "parent": 30, "id": 32, "before": NSNull()],
            ["op": "el", "id": 33, "tag": "div", "attrs": [["style", "height:200px"]]], ["op": "ins", "parent": 30, "id": 33, "before": NSNull()],
        ])
        defer { renderer.destroy(); window.orderOut(nil) }
        renderer.send("{\"type\":\"ops\",\"ops\":[{\"op\":\"prop\",\"id\":30,\"name\":\"scrollTop\",\"value\":30}]}")
        pump(1, until: { renderer.scrolling.offset(30).y == 30 })
        let laid = renderer.elementBounds()[32]!
        let box = renderer.scrolling.geometries[30]!.viewport
        let point = CGPoint(x: laid.midX, y: laid.midY)
        let event = NSEvent.mouseEvent(with: .leftMouseDown, location: renderer.view.convert(point, to: nil), modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
        renderer.handleMouseDown(point, event: event)
        let view = renderer.view as! WidgetDocumentView
        pump(2, until: { view.formEditor != nil })
        guard let editor = view.formEditor else {
            return XCTFail("no editor")
        }
        XCTAssertEqual(editor.container.frame, box)
        XCTAssertFalse(editor.container.isHidden)
        let field = editor.view.frame.offsetBy(dx: box.minX, dy: box.minY)
        XCTAssertTrue(field.minY > laid.minY && field.maxY < laid.maxY && laid.minY < 60, "\(field) \(laid)")
        renderer.send("{\"type\":\"ops\",\"ops\":[{\"op\":\"prop\",\"id\":30,\"name\":\"scrollTop\",\"value\":150}]}")
        pump(1, until: { editor.container.isHidden })
        XCTAssertTrue(editor.container.isHidden)
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
        XCTAssertTrue(field.superview?.superview === view)
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
        XCTAssertNil(field.superview?.superview)
        let tail = events().map { $0["type"] as? String ?? "" }.filter { ["change", "blur"].contains($0) }
        XCTAssertEqual(tail, ["change", "blur"])
        let html = renderer.dom.serialize(theme: "light")
        XCTAssertTrue(html.contains("value=\"hello\""), html)
        XCTAssertTrue(html.contains("placeholder=\"Name\""), html)
    }
}
