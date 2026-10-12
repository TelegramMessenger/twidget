import XCTest
import AppKit
@testable import TwidgetRenderer

final class HiddenTileTests: XCTestCase {
    private func pump(_ seconds: TimeInterval, until done: () -> Bool = { false }) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done() && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
    }

    func testHiddenRendererDropsItsTilesAndRedrawsThemWhenShown() {
        _ = NSApplication.shared
        let delay = LiteHTMLWidgetRenderer.hiddenTrimDelay
        LiteHTMLWidgetRenderer.hiddenTrimDelay = 0.05
        defer { LiteHTMLWidgetRenderer.hiddenTrimDelay = delay }
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 500), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.orderFrontRegardless()
        defer { window.orderOut(nil) }
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
        renderer.onMessage = { _ in }
        renderer.view.frame = NSRect(x: 0, y: 0, width: 400, height: 400)
        window.contentView?.addSubview(renderer.view)
        renderer.load()
        func text(_ value: String) -> String {
            return "{\"type\":\"ops\",\"ops\":[{\"op\":\"text\",\"id\":20,\"data\":\"\(value)\"}]}"
        }
        renderer.send("{\"type\":\"ops\",\"ops\":[{\"op\":\"el\",\"id\":2,\"tag\":\"html\",\"attrs\":[],\"adopt\":\"html\"},{\"op\":\"el\",\"id\":7,\"tag\":\"body\",\"attrs\":[],\"adopt\":\"body\"},{\"op\":\"el\",\"id\":9,\"tag\":\"div\",\"attrs\":[[\"style\",\"height:360px;background:#3a7\"]]},{\"op\":\"ins\",\"parent\":7,\"id\":9,\"before\":null},{\"op\":\"text\",\"id\":20,\"data\":\"one\"},{\"op\":\"ins\",\"parent\":9,\"id\":20,\"before\":null}]}")
        pump(3, until: { renderer.tileLayerCount > 0 && renderer.drawCount > 0 })
        let shownTiles = renderer.tileLayerCount
        XCTAssertGreaterThan(shownTiles, 0)
        renderer.setVisible(false)
        pump(2, until: { renderer.tileLayerCount == 0 })
        XCTAssertEqual(renderer.tileLayerCount, 0)
        let drawsWhileHidden = renderer.drawCount
        renderer.send(text("two"))
        pump(0.3)
        XCTAssertEqual(renderer.tileLayerCount, 0)
        XCTAssertEqual(renderer.drawCount, drawsWhileHidden)
        renderer.setVisible(true)
        pump(3, until: { renderer.tileLayerCount == shownTiles && renderer.drawCount > drawsWhileHidden })
        XCTAssertEqual(renderer.tileLayerCount, shownTiles)
        XCTAssertGreaterThan(renderer.drawCount, drawsWhileHidden)
        renderer.destroy()
    }
}
