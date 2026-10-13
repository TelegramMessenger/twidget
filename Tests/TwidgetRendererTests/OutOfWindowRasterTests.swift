import XCTest
import AppKit
@testable import TwidgetRenderer

final class OutOfWindowRasterTests: XCTestCase {
    private final class Queue {
        private let lock = NSLock()
        private var blocks: [() -> Void] = []

        func add(_ block: @escaping () -> Void) {
            lock.lock()
            blocks.append(block)
            lock.unlock()
        }

        var count: Int {
            lock.lock()
            defer { lock.unlock() }
            return blocks.count
        }

        func drain() {
            while true {
                lock.lock()
                guard !blocks.isEmpty else {
                    lock.unlock()
                    return
                }
                let block = blocks.removeFirst()
                lock.unlock()
                block()
            }
        }
    }

    private func pump(_ seconds: TimeInterval, queue: Queue?, until done: () -> Bool) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done() && Date() < deadline {
            queue?.drain()
            RunLoop.main.run(until: Date().addingTimeInterval(0.005))
        }
    }

    private func ops(_ body: String) -> String {
        return "{\"type\":\"ops\",\"ops\":[\(body)]}"
    }

    func testRasterFinishingOutOfTheWindowKeepsItsDamage() {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 500), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.orderFrontRegardless()
        defer { window.orderOut(nil) }
        let queue = Queue()
        let renderer = LiteHTMLWidgetRenderer(theme: "light", width: 400)
        renderer.isOnExecutor = { Thread.isMainThread }
        renderer.executorIsMain = { false }
        renderer.schedule = { queue.add($0) }
        renderer.scheduleAfter = { delay, block in DispatchQueue.main.asyncAfter(deadline: .now() + delay) { queue.add(block) } }
        renderer.onMessage = { _ in }
        renderer.view.frame = NSRect(x: 0, y: 0, width: 400, height: 400)
        window.contentView?.addSubview(renderer.view)
        renderer.load()
        renderer.send(ops("{\"op\":\"el\",\"id\":2,\"tag\":\"html\",\"attrs\":[],\"adopt\":\"html\"},{\"op\":\"el\",\"id\":7,\"tag\":\"body\",\"attrs\":[[\"style\",\"margin:0\"]],\"adopt\":\"body\"},{\"op\":\"el\",\"id\":9,\"tag\":\"div\",\"attrs\":[[\"style\",\"height:60px;margin:20px;border-radius:8px;background-color:#d33\"]]},{\"op\":\"ins\",\"parent\":7,\"id\":9,\"before\":null},{\"op\":\"el\",\"id\":11,\"tag\":\"div\",\"attrs\":[[\"style\",\"height:20px;margin:300px 20px 0 20px;background-color:#333\"]]},{\"op\":\"ins\",\"parent\":7,\"id\":11,\"before\":null}"))
        pump(5, queue: queue, until: { renderer.tileLayerCount > 0 && !renderer.tilesPreparing && renderer.staleTileCount == 0 && queue.count == 0 })
        XCTAssertGreaterThan(renderer.tileLayerCount, 0)
        XCTAssertEqual(renderer.staleTileCount, 0)

        renderer.send(ops("{\"op\":\"attr\",\"id\":9,\"name\":\"style\",\"value\":\"height:60px;margin:20px;border-radius:8px;background-color:#33d\"}"))
        pump(3, queue: queue, until: { renderer.tilesPreparing })
        XCTAssertTrue(renderer.tilesPreparing)
        pump(3, queue: nil, until: { queue.count > 0 })
        renderer.view.removeFromSuperview()
        queue.drain()
        window.contentView?.addSubview(renderer.view)

        renderer.send(ops("{\"op\":\"attr\",\"id\":11,\"name\":\"style\",\"value\":\"height:20px;margin:300px 20px 0 20px;background-color:#555\"}"))
        pump(5, queue: queue, until: { !renderer.tilesPreparing && queue.count == 0 && renderer.staleTileCount == 0 })
        XCTAssertEqual(renderer.staleTileCount, 0)
        renderer.destroy()
    }
}
