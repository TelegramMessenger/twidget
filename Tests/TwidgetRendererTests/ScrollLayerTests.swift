import XCTest
import LiteHTML
@testable import TwidgetRenderer

final class ScrollLayerTests: XCTestCase {
    private func list(rows: Int = 100) -> LHDocument {
        let items = (0..<rows).map { "<div style='height:20px;background:\($0 % 2 == 0 ? "#f00" : "#00f")'>\($0)</div>" }.joined()
        let document = LHDocument(html: "<body style='margin:0;background:#fff'><div style='height:10px'></div><div id='list' style='height:100px;overflow:auto'>\(items)</div></body>", css: nil)
        _ = document.layout(maxWidth: 120, preferredWidth: 120)
        return document
    }

    func testScrollersReportViewportAndContent() throws {
        let document = list()
        let info = try XCTUnwrap(document.scrollers(byAttribute: "id")["list"])
        XCTAssertEqual((info["viewport"] as? NSValue)?.rectValue, CGRect(x: 0, y: 10, width: 120, height: 100))
        XCTAssertEqual((info["content"] as? NSValue)?.sizeValue.height, 2000)
    }

    func testOffsetsClampAndDriveHitTesting() {
        let document = list()
        XCTAssertEqual(document.setScrollOffset(CGPoint(x: 0, y: 5000), attribute: "id", value: "list"), CGPoint(x: 0, y: 1900))
        XCTAssertEqual(document.setScrollOffset(CGPoint(x: 0, y: 410), attribute: "id", value: "list"), CGPoint(x: 0, y: 410))
        document.setPaintExclusions([], attribute: "id")
        XCTAssertEqual(document.setScrollOffset(CGPoint(x: 0, y: -3), attribute: "id", value: "list"), .zero)
    }

    private func red(_ image: CGImage, _ y: Int, scale: Int) -> Int {
        var data = [UInt8](repeating: 0, count: 4)
        let ctx = CGContext(data: &data, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(image, in: CGRect(x: -100 * scale, y: (y * scale) - image.height + 1, width: image.width, height: image.height))
        return Int(data[0])
    }

    func testScrollContentRastersInContentCoordinates() throws {
        let document = list()
        _ = document.setScrollOffset(CGPoint(x: 0, y: 300), attribute: "id", value: "list")
        let tile = try XCTUnwrap(document.copyScrollContent(attribute: "id", value: "list", rect: CGRect(x: 0, y: 40, width: 120, height: 60), scale: 2))
        XCTAssertEqual(tile.height, 120)
        XCTAssertEqual(red(tile, 5, scale: 2), 255)
        XCTAssertEqual(red(tile, 25, scale: 2), 0)
        document.setScrollContentExclusions(["list"], attribute: "id")
        let page = try XCTUnwrap(document.copyImage(maxWidth: 120, rect: CGRect(x: 0, y: 0, width: 120, height: 120), scale: 1))
        XCTAssertEqual(red(page, 50, scale: 1), 255)
        var data = [UInt8](repeating: 0, count: 4)
        let ctx = CGContext(data: &data, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(page, in: CGRect(x: -100, y: 50 - page.height + 1, width: page.width, height: page.height))
        XCTAssertEqual(Array(data.prefix(3)), [255, 255, 255])
    }

    func testRubberBandingResistsAndSettles() {
        let geometry = WidgetScrollGeometry(viewport: CGRect(x: 0, y: 0, width: 100, height: 200), content: CGSize(width: 100, height: 1000), radii: [CGFloat](repeating: 0, count: 8))
        XCTAssertEqual(geometry.maxOffset, CGPoint(x: 0, y: 800))
        XCTAssertEqual(geometry.clamp(CGPoint(x: 5, y: -20)), .zero)
        let pulled = geometry.banded(CGPoint(x: 0, y: -100))
        XCTAssertLessThan(pulled.y, 0)
        XCTAssertGreaterThan(pulled.y, -100)
        XCTAssertLessThan(abs(geometry.banded(CGPoint(x: 0, y: -10_000)).y), 200)
        XCTAssertEqual(geometry.banded(CGPoint(x: 0, y: 400)).y, 400)
        XCTAssertGreaterThan(geometry.banded(CGPoint(x: 0, y: 900)).y, 800)
    }
}
