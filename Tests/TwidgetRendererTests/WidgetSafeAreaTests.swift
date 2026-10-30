import XCTest
import AppKit
import WidgetSandbox
@testable import TwidgetRenderer

final class WidgetSafeAreaTests: XCTestCase {
    private func pump(_ seconds: TimeInterval, until done: () -> Bool = { false }) {
        let end = Date().addingTimeInterval(seconds)
        while !done() && Date() < end { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    }

    func testSafeAreaVariablesRejectInvalidValues() {
        let values = Dictionary(uniqueKeysWithValues: WidgetThemeStyle.contentSafeArea(["top": true, "right": "5", "bottom": 64.01, "left": -1, "cornerLeft": Double.nan, "cornerRight": 200]))
        XCTAssertEqual(values["--tg-content-safe-area-inset-top"], "0px")
        XCTAssertEqual(values["--tg-content-safe-area-inset-right"], "0px")
        XCTAssertEqual(values["--tg-content-safe-area-inset-bottom"], "0px")
        XCTAssertEqual(values["--tg-content-safe-area-inset-left"], "0px")
        XCTAssertEqual(values["--tg-content-safe-area-corner-left"], "0px")
        XCTAssertEqual(values["--tg-content-safe-area-corner-right"], "200px")
        XCTAssertEqual(WidgetThemeStyle.contentSafeArea(nil).count, 6)
    }

    func testBootUpdatesThemeAndCaptionTransitions() throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 390, height: 500), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        var options = WidgetHost.Options()
        options.contentSafeArea = WidgetContentSafeArea(bottom: 24, cornerRight: 80)
        var made: LiteHTMLWidgetRenderer?
        let html = """
        <style>:root{--tg-content-safe-area-inset-bottom:999px !important}body{margin:0}#band{width:env(safe-area-inset-bottom);height:10px}#row{position:relative;height:100px}#corner{position:absolute;left:var(--tg-content-safe-area-corner-left);right:var(--tg-content-safe-area-corner-right);height:10px}</style>
        <div id=band></div><div id=row><div id=corner></div></div>
        <script>var calls=0;function report(){document.documentElement.setAttribute('data-safe',JSON.stringify({area:TelegramWidget.contentSafeAreaInset,calls:calls,frozen:Object.isFrozen(TelegramWidget.contentSafeAreaInset)}));}TelegramWidget.onEvent('contentSafeAreaChanged',function(){calls++;report();});report();</script>
        """
        let host = try WidgetHost(html: html, theme: "light", width: 390, options: options, rendererFactory: { _, theme, width in
            let renderer = LiteHTMLWidgetRenderer(theme: theme, width: width)
            made = renderer
            return renderer
        })
        defer { host.destroy(); window.close() }
        let renderer = try XCTUnwrap(made)
        host.view.frame = window.contentView!.bounds
        window.contentView?.addSubview(host.view)
        func ready(_ bottom: Int, _ calls: Int) -> Bool {
            guard let value = renderer.dom.html.attribute("data-safe"), let report = try? JSONSerialization.jsonObject(with: Data(value.utf8)) as? [String: Any], let area = report["area"] as? [String: Int] else { return false }
            return area["bottom"] == bottom && report["calls"] as? Int == calls && report["frozen"] as? Bool == true && abs((renderer.document?.bounds(matching: "#band").width ?? -1) - CGFloat(bottom)) < 0.5
        }
        pump(10, until: { ready(24, 0) })
        XCTAssertTrue(ready(24, 0), host.diagnostics.lastError ?? "boot")
        XCTAssertNil(host.diagnostics.dead)
        let bandedHeight = host.diagnostics.height
        XCTAssertEqual(renderer.document?.bounds(matching: "#corner").width ?? 0, 310, accuracy: 0.5)
        host.setTheme(WidgetTheme(colorScheme: .dark))
        pump(0.3)
        XCTAssertTrue(ready(24, 0))
        XCTAssertEqual(Dictionary(uniqueKeysWithValues: renderer.dom.rootVariables)["--tg-content-safe-area-inset-bottom"], "24px")
        host.setContentSafeArea(WidgetContentSafeArea())
        pump(5, until: { ready(0, 1) })
        XCTAssertTrue(ready(0, 1))
        pump(0.2)
        XCTAssertEqual(host.diagnostics.height, bandedHeight - 24, accuracy: 0.5)
        host.setContentSafeArea(WidgetContentSafeArea(bottom: 24, cornerLeft: 90))
        pump(5, until: { ready(24, 2) })
        XCTAssertTrue(ready(24, 2))
        XCTAssertEqual(renderer.document?.bounds(matching: "#corner").minX ?? 0, 90, accuracy: 0.5)
        XCTAssertEqual(renderer.document?.bounds(matching: "#corner").width ?? 0, 300, accuracy: 0.5)
        host.setContentSafeArea(WidgetContentSafeArea(bottom: 24, cornerLeft: 90))
        pump(0.2)
        XCTAssertTrue(ready(24, 2))
    }
    func testSafeAreaCorpusAcrossCaptionLayoutsAndThemes() throws {
        _ = NSApplication.shared
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../../widget-sandbox/corpus/safe-area").standardized
        let files = try FileManager.default.subpathsOfDirectory(atPath: root.path).filter { $0.hasSuffix(".html") }.sorted()
        XCTAssertEqual(files.count, 7)
        var results: [[String: Any]] = []
        let output = ProcessInfo.processInfo.environment["TW_SAFE_AREA_OUT"].map { URL(fileURLWithPath: $0) }
        if let output { try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true) }
        for theme in ["light", "dark"] {
            for file in files {
                let html = try String(contentsOf: root.appendingPathComponent(file))
                let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 320, height: 600), styleMask: [.borderless], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                var options = WidgetHost.Options()
                options.contentSafeArea = WidgetContentSafeArea(bottom: 24, cornerRight: 64)
                var made: LiteHTMLWidgetRenderer?
                let host = try WidgetHost(html: html, theme: theme, width: 320, options: options, rendererFactory: { _, theme, width in
                    let renderer = LiteHTMLWidgetRenderer(theme: theme, width: width)
                    made = renderer
                    return renderer
                })
                let renderer = try XCTUnwrap(made)
                host.view.frame = window.contentView!.bounds
                window.contentView?.addSubview(host.view)
                host.onChange = { [weak host] diagnostics in
                    guard let host, abs(host.view.frame.height - diagnostics.height) > 0.5 else { return }
                    host.view.frame.size.height = max(50, min(600, diagnostics.height))
                }
                pump(10, until: { host.diagnostics.loaded && host.diagnostics.rendered || host.diagnostics.dead != nil })
                for (mode, area) in [("none", WidgetContentSafeArea(bottom: 24, cornerRight: 64)), ("below", WidgetContentSafeArea()), ("above", WidgetContentSafeArea(bottom: 24, cornerRight: 64)), ("rtl", WidgetContentSafeArea(bottom: 24, cornerLeft: 64))] {
                    host.setContentSafeArea(area)
                    pump(0.35)
                    XCTAssertTrue(host.diagnostics.loaded, file)
                    XCTAssertNil(host.diagnostics.dead, file)
                    XCTAssertNil(host.diagnostics.lastError, file)
                    let vars = Dictionary(uniqueKeysWithValues: renderer.dom.rootVariables)
                    XCTAssertEqual(vars["--tg-content-safe-area-inset-bottom"], "\(Int(area.bottom))px", file)
                    XCTAssertEqual(vars["--tg-content-safe-area-corner-left"], "\(Int(area.cornerLeft))px", file)
                    XCTAssertEqual(vars["--tg-content-safe-area-corner-right"], "\(Int(area.cornerRight))px", file)
                    let name = (file as NSString).lastPathComponent.replacingOccurrences(of: ".html", with: "") + "-" + theme + "-" + mode
                    var result: [String: Any] = ["file": file, "theme": theme, "caption": mode, "height": host.diagnostics.height, "bottom": area.bottom, "cornerLeft": area.cornerLeft, "cornerRight": area.cornerRight]
                    if let output {
                        let image = try XCTUnwrap(renderer.snapshot(), name)
                        let data = try XCTUnwrap(image.tiffRepresentation)
                        let rep = try XCTUnwrap(NSBitmapImageRep(data: data))
                        let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
                        try png.write(to: output.appendingPathComponent(name + ".png"))
                        result["image"] = name + ".png"
                    }
                    results.append(result)
                }
                host.destroy()
                window.close()
            }
        }
        if let output { try JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys]).write(to: output.appendingPathComponent("results.json")) }
    }
}
