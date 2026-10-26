import AppKit
import XCTest
import WidgetSandbox
@testable import TwidgetRenderer

final class ScopedWidgetMotionTests: XCTestCase {
    private func pump(_ seconds: TimeInterval, until done: () -> Bool = { false }) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done() && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.005))
        }
    }

    func testSampleTabsAnimateChangedPanelsAndKeepHeadersOpaque() throws {
        _ = NSApplication.shared
        let samples: [(String, String, String, String, String)] = [
            ("01-design-crew/04-priya-bare.html", "data-act", "filter", "open", "list"),
            ("07-shop/08-shop-bare.html", "data-action", "sort", "newest", "list"),
            ("09-code-review/09-me.html", "data-action", "tab", "1", "rows"),
            ("12-motion-lab/13-mo-bare.html", "data-action", "pay", "", "pay")
        ]
        for (file, attribute, action, value, panelID) in samples {
            let path = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../../widget-sandbox/corpus/htmlbubbles-ported/\(file)")
            let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
            let host = try WidgetHost(html: String(contentsOf: path), theme: WidgetTheme(colorScheme: .light), width: 400, rendererFactory: { _, _, _ in renderer })
            defer { host.destroy() }
            host.view.frame = CGRect(x: 0, y: 0, width: 400, height: 700)
            pump(10) { host.diagnostics.rendered && renderer.document != nil }
            pump(0.3)
            XCTAssertTrue(host.diagnostics.rendered, file)
            XCTAssertNil(host.diagnostics.dead, file)
            let root = try XCTUnwrap(renderer.dom.nodes.values.first { ["root", "w"].contains($0.attribute("id") ?? "") }, file)
            let classes = root.attribute("class")
            let target = try XCTUnwrap(renderer.dom.nodes.values.first {
                $0.attribute(attribute) == action && (value.isEmpty || $0.attribute("data-value") == value)
            }, file)
            renderer.noteUserInput()
            renderer.onMessage?("{\"type\":\"event\",\"trusted\":true,\"ev\":{\"type\":\"click\",\"targetId\":\(renderer.dom.publicId(target)),\"bubbles\":true,\"button\":0}}")
            let panel = try XCTUnwrap(renderer.dom.nodes.values.first { $0.attribute("id") == panelID }, file)
            pump(0.03)
            renderer.postFrame(time: 1000)
            pump(0.25) { (panel.attribute("class") ?? "").split(separator: " ").contains { $0.hasPrefix("tr-") || $0.hasPrefix("fx") } }
            XCTAssertEqual(root.attribute("class"), classes, file)
            XCTAssertFalse(renderer.animatedElementIDs.contains(renderer.dom.publicId(root)), file)
            XCTAssertTrue((panel.attribute("class") ?? "").split(separator: " ").contains { $0.hasPrefix("tr-") || $0.hasPrefix("fx") }, file)
            XCTAssertNotNil(renderer.snapshot(), file)
            pump(0.4)
            XCTAssertNil(host.diagnostics.dead, file)
            XCTAssertEqual(host.diagnostics.logs.filter { $0.level == "error" }.map(\.text), [], file)
        }
    }
}
