import AppKit
import XCTest
import WidgetSandbox
@testable import TwidgetRenderer

final class MediaControlTests: XCTestCase {
    private func pump(_ seconds: TimeInterval, until done: () -> Bool = { false }) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done() && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.005))
        }
    }

    func testMusicControlsKeepPortableIconsAndRespondThroughSVGChildren() throws {
        _ = NSApplication.shared
        let path = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../../widget-sandbox/corpus/htmlbubbles-ported/12-motion-lab/05-mo-bare.html")
        let html = try String(contentsOf: path)
        for scheme in [WidgetTheme.ColorScheme.light, .dark] {
            let renderer = LiteHTMLWidgetRenderer(theme: scheme == .light ? "light" : "dark", width: 360)
            let host = try WidgetHost(html: html, theme: WidgetTheme(colorScheme: scheme), width: 360, rendererFactory: { _, _, _ in renderer })
            defer { host.destroy() }
            host.view.frame = CGRect(x: 0, y: 0, width: 360, height: 400)
            pump(10) { host.diagnostics.rendered && renderer.dom.nodes.values.contains { $0.attribute("id") == "playIcon" } }
            pump(0.4)
            XCTAssertTrue(host.diagnostics.rendered)
            XCTAssertNil(host.diagnostics.dead)
            XCTAssertEqual(host.diagnostics.logs.filter { $0.level == "error" }.map(\.text), [])
            func save(_ state: String) throws {
                guard let directory = ProcessInfo.processInfo.environment["TW_MEDIA_CONTROL_OUT"] else { return }
                let output = URL(fileURLWithPath: directory)
                try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
                let image = try XCTUnwrap(renderer.snapshot())
                let representation = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
                let data = try XCTUnwrap(representation.representation(using: .png, properties: [:]))
                try data.write(to: output.appendingPathComponent("music-\(scheme.rawValue)-\(state).png"))
            }
            try save("playing")
            let controls = renderer.dom.nodes.values.filter { ["play", "prev", "next", "shuffle", "repeat"].contains($0.attribute("data-action") ?? "") }
            XCTAssertEqual(controls.count, 5)
            for control in controls {
                let svg = try XCTUnwrap(control.children.first { $0.tag == "svg" })
                XCTAssertNotNil(svg.children.first { $0.tag == "path" })
                XCTAssertFalse(control.children.contains { $0.isText && !$0.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty })
            }
            let icon = try XCTUnwrap(renderer.dom.nodes.values.first { $0.attribute("id") == "playIcon" })
            let play = try XCTUnwrap(icon.parent?.parent)
            XCTAssertEqual(play.attribute("aria-label"), "Pause")
            XCTAssertEqual(icon.attribute("d"), "M6 4h4v16H6zm8 0h4v16h-4z")
            func click(_ node: WidgetNode) {
                renderer.noteUserInput()
                renderer.onMessage?("{\"type\":\"event\",\"trusted\":true,\"ev\":{\"type\":\"click\",\"targetId\":\(renderer.dom.publicId(node)),\"bubbles\":true,\"button\":0}}")
            }
            click(icon)
            pump(1) { play.attribute("aria-label") == "Play" }
            XCTAssertEqual(icon.attribute("d"), "M7 4v16l13-8z")
            let shuffle = try XCTUnwrap(controls.first { $0.attribute("data-action") == "shuffle" })
            let shufflePath = try XCTUnwrap(shuffle.children.first?.children.first)
            click(shufflePath)
            pump(1) { shuffle.attribute("aria-pressed") == "true" }
            XCTAssertEqual(shuffle.attribute("aria-pressed"), "true")
            XCTAssertTrue(shuffle.attribute("class")?.contains("on") == true)
            let repeatControl = try XCTUnwrap(controls.first { $0.attribute("data-action") == "repeat" })
            click(try XCTUnwrap(repeatControl.children.first?.children.first))
            pump(1) { repeatControl.attribute("aria-pressed") == "true" }
            XCTAssertEqual(repeatControl.attribute("aria-pressed"), "true")
            let queueLink = try XCTUnwrap(renderer.dom.nodes.values.first { $0.attribute("data-action") == "queue" })
            let queue = try XCTUnwrap(renderer.dom.nodes.values.first { $0.attribute("id") == "list" })
            click(queueLink)
            pump(1) { queue.attribute("hidden") == nil }
            XCTAssertNil(queue.attribute("hidden"))
            click(queueLink)
            pump(1) { queue.attribute("hidden") != nil }
            XCTAssertNotNil(queue.attribute("hidden"))
            let next = try XCTUnwrap(controls.first { $0.attribute("data-action") == "next" })
            let art = try XCTUnwrap(renderer.dom.nodes.values.first { $0.attribute("id") == "art" })
            click(next)
            pump(1) { art.attribute("class")?.contains("tr-slide-left") == true }
            XCTAssertTrue(art.attribute("class")?.contains("tr-slide-left") == true)
            XCTAssertNotNil(renderer.snapshot())
            try save("paused-toggles")
        }
    }
}
