import XCTest
@testable import TwidgetRenderer

final class FormValueTests: XCTestCase {
    private func makeDOM() -> WidgetDOM {
        let dom = WidgetDOM()
        dom.apply(["op": "el", "id": 1, "tag": "html", "adopt": "html"])
        dom.apply(["op": "el", "id": 2, "tag": "head", "adopt": "head"])
        dom.apply(["op": "el", "id": 3, "tag": "body", "adopt": "body"])
        return dom
    }

    private func add(_ dom: WidgetDOM, _ id: Int, _ tag: String, parent: Int = 3, attrs: [[Any]] = []) {
        dom.apply(["op": "el", "id": id, "tag": tag, "attrs": attrs])
        dom.apply(["op": "ins", "id": id, "parent": parent])
    }

    func testInputValueAndCheckedPropertiesReplaceTheAttributes() {
        let dom = makeDOM()
        add(dom, 10, "input", attrs: [["value", "attr"]])
        add(dom, 11, "input", attrs: [["type", "checkbox"], ["checked", ""]])
        dom.apply(["op": "prop", "id": 10, "name": "value", "value": "typed \"x\""])
        dom.apply(["op": "prop", "id": 11, "name": "checked", "value": false])
        let html = dom.serialize(theme: "light")
        XCTAssertTrue(html.contains("value=\"typed &quot;x&quot;\""), html)
        XCTAssertFalse(html.contains("value=\"attr\""), html)
        XCTAssertFalse(html.contains("checked"), html)
    }

    func testTextareaValueReplacesItsTextAndKeepsALeadingNewline() {
        let dom = makeDOM()
        add(dom, 10, "textarea")
        dom.apply(["op": "text", "id": 11, "data": "default"])
        dom.apply(["op": "ins", "id": 11, "parent": 10])
        dom.apply(["op": "prop", "id": 10, "name": "value", "value": "\nline <b>"])
        let html = dom.serialize(theme: "light")
        XCTAssertTrue(html.contains(">\n\nline &lt;b&gt;</textarea>"), html)
        XCTAssertFalse(html.contains("default"), html)
    }

    func testSelectValueAndIndexMarkTheChosenOption() {
        let dom = makeDOM()
        add(dom, 10, "select")
        add(dom, 11, "option", parent: 10, attrs: [["selected", ""]])
        dom.apply(["op": "text", "id": 12, "data": "One"])
        dom.apply(["op": "ins", "id": 12, "parent": 11])
        add(dom, 13, "option", parent: 10, attrs: [["value", "b"]])
        dom.apply(["op": "prop", "id": 10, "name": "value", "value": "b"])
        var html = dom.serialize(theme: "light")
        XCTAssertTrue(html.contains("value=\"b\" selected"), html)
        XCTAssertFalse(html.contains("data-lh-id=\"11\" selected"), html)
        dom.apply(["op": "prop", "id": 10, "name": "selectedIndex", "value": 0])
        html = dom.serialize(theme: "light")
        XCTAssertTrue(html.contains("data-lh-id=\"11\" selected"), html)
        XCTAssertFalse(html.contains("value=\"b\" selected"), html)
    }

    func testAFormPropertyInvalidatesThePatchLog() {
        let dom = makeDOM()
        add(dom, 10, "input")
        _ = dom.serialize(theme: "light")
        dom.resetPatchLog()
        dom.apply(["op": "prop", "id": 10, "name": "value", "value": "x"])
        XCTAssertFalse(dom.patchLog.valid)
    }
}
