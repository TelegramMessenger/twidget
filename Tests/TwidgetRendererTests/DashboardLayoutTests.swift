import XCTest
import LiteHTML

final class DashboardLayoutTests: XCTestCase {
    private func document(_ html: String, width: CGFloat = 400) -> LHDocument {
        let doc = LHDocument(html: "<style>body{margin:0}</style>" + html, css: nil)
        _ = doc.layout(maxWidth: width, preferredWidth: width)
        return doc
    }

    private func pixel(_ image: CGImage, x: Int, y: Int) -> [UInt8] {
        var bytes = [UInt8](repeating: 0, count: 4)
        let ctx = CGContext(data: &bytes, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(image, in: CGRect(x: -x, y: y - image.height + 1, width: image.width, height: image.height))
        return bytes
    }

    func testGridCardsFillAColumnFlexSheet() {
        for sizing in ["content-box", "border-box"] {
            for columns in ["1fr 1fr", "repeat(2,minmax(0,1fr))", "repeat(auto-fit,minmax(100px,1fr))"] {
                let doc = document("""
                <style>*{box-sizing:\(sizing)}.sheet{display:flex;flex-direction:column;width:360px;padding:10px}.grid{display:grid;grid-template-columns:\(columns);gap:10px}.card{padding:10px;background:red}</style>
                <div class=sheet><div class=grid><div class=card id=a>One</div><div class=card id=b>Two</div></div></div>
                """)
                let grid = doc.bounds(matching: ".grid")
                let a = doc.bounds(matching: "#a"), b = doc.bounds(matching: "#b")
                XCTAssertEqual(grid.width, sizing == "border-box" ? 340 : 360, accuracy: 0.1, "\(sizing) \(columns)")
                XCTAssertEqual(a.width, (grid.width - 10) / 2, accuracy: 0.1, "\(sizing) \(columns)")
                XCTAssertEqual(b.maxX, grid.maxX, accuracy: 0.1, "\(sizing) \(columns)")
            }
        }
    }

    func testPaddedGridCardsKeepTheTrackWidth() {
        for sizing in ["content-box", "border-box"] {
            let doc = document("""
            <style>*{box-sizing:\(sizing)}.grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;width:340px;padding:20px}.card{display:grid;grid-template-columns:1fr auto;padding:12px;border:1px solid}.content{min-width:0}</style>
            <div class=grid><div class=card id=a><div class=content>A long heading for a card</div><div>12</div></div><div class=card id=b><div class=content>Second card</div><div>34</div></div></div>
            """)
            let grid = doc.bounds(matching: ".grid")
            let a = doc.bounds(matching: "#a"), b = doc.bounds(matching: "#b")
            XCTAssertEqual(a.width, (grid.width - 50) / 2, accuracy: 0.1, sizing)
            XCTAssertEqual(b.maxX, grid.maxX - 20, accuracy: 0.1, sizing)
        }
    }

    func testGridCardMinimumWidthOverridesItsLongHeading() {
        for minimum in ["min-width:0", "overflow:hidden"] {
            let doc = document("""
            <div style='display:grid;grid-template-columns:1fr 1fr;gap:10px;width:340px'><div id=a style='\(minimum);padding:10px'>AReallyLongUnbreakableDashboardCardHeadingThatMustNotWidenTheSheet</div><div id=b>Two</div></div>
            """)
            XCTAssertEqual(doc.bounds(matching: "#a").width, 165, accuracy: 0.1, minimum)
            XCTAssertEqual(doc.bounds(matching: "#b").maxX, 340, accuracy: 0.1, minimum)
        }
    }

    func testStickyHeaderScrollsInFlowThenSticksAndRemainsHittable() throws {
        let doc = document("""
        <div id=scroller style='height:100px;overflow:auto'><div style='height:40px'></div><div id=header style='position:sticky;top:5px;height:20px;background:red;z-index:1'></div><div style='height:400px;background:blue'></div></div>
        """, width: 120)
        XCTAssertEqual(doc.bounds(matching: "#header").minY, 40, accuracy: 0.1)
        _ = doc.setScrollOffset(CGPoint(x: 0, y: 80), attribute: "id", value: "scroller")
        XCTAssertEqual(doc.bounds(matching: "#header").minY, 5, accuracy: 0.1)
        let image = try XCTUnwrap(doc.copyImage(maxWidth: 120, rect: CGRect(x: 0, y: 0, width: 120, height: 100), scale: 1))
        XCTAssertEqual(pixel(image, x: 50, y: 10), [255, 0, 0, 255])
        XCTAssertEqual(doc.attribute("id", at: CGPoint(x: 50, y: 10)), "header")
        XCTAssertEqual((doc.scrollers(byAttribute: "id")["scroller"]?["sticky"] as? NSNumber)?.boolValue, true)
    }

    func testStickyHeaderStopsAtItsContainingBlock() {
        let doc = document("""
        <div id=scroller style='height:100px;overflow:auto'><div style='height:120px'><div id=header style='position:sticky;top:0;height:20px'></div></div><div style='height:400px'></div></div>
        """, width: 120)
        _ = doc.setScrollOffset(CGPoint(x: 0, y: 110), attribute: "id", value: "scroller")
        XCTAssertEqual(doc.bounds(matching: "#header").minY, -10, accuracy: 0.1)
    }

    func testClipPathsMaskBackgroundDescendantsAndHitTesting() throws {
        for shape in ["polygon(50% 0,100% 100%,0 100%)", "circle(40% at 50% 50%)", "ellipse(40% 40% at center center)", "inset(10% round 12px)"] {
            let doc = document("<div id=shape style='width:100px;height:100px;background:red;clip-path:\(shape)'><div style='width:100px;height:100px;background:blue'></div></div>", width: 100)
            let image = try XCTUnwrap(doc.copyImage(maxWidth: 100, rect: CGRect(x: 0, y: 0, width: 100, height: 100), scale: 1))
            XCTAssertEqual(pixel(image, x: 2, y: 2)[3], 0, shape)
            XCTAssertEqual(pixel(image, x: 50, y: 50), [0, 0, 255, 255], shape)
            XCTAssertNil(doc.attribute("id", at: CGPoint(x: 2, y: 2)), shape)
            XCTAssertEqual(doc.attribute("id", at: CGPoint(x: 50, y: 50)), "shape", shape)
        }
    }
    func testHorizontalStickyAndItsChildrenUseScrolledCoordinates() {
        let doc = document("""
        <div id=scroller style='width:100px;height:60px;overflow:auto'><div style='width:400px;height:60px'><div id=header style='position:sticky;left:5px;margin-left:30px;width:20px;height:20px;background:red'><div id=child style='height:10px'></div></div></div></div>
        """, width: 120)
        _ = doc.setScrollOffset(CGPoint(x: 80, y: 0), attribute: "id", value: "scroller")
        XCTAssertEqual(doc.bounds(matching: "#header").minX, 5, accuracy: 0.1)
        XCTAssertEqual(doc.bounds(matching: "#child").minX, 5, accuracy: 0.1)
        XCTAssertEqual(doc.attribute("id", at: CGPoint(x: 10, y: 5)), "child")
    }

    func testClipChangesInvalidateTheirPaintAndClipTheRoot() throws {
        let before = document("<div style='width:100px;height:100px;background:red;clip-path:inset(0 50% 0 0)'></div>", width: 100)
        let after = document("<div style='width:100px;height:100px;background:red;clip-path:inset(0 0 0 50%)'></div>", width: 100)
        XCTAssertFalse(after.displayList(maxWidth: 100, scale: 2).changedRects(since: before.displayList(maxWidth: 100, scale: 2)).isEmpty)
        let root = document("<style>html{clip-path:inset(10px)}</style><div style='height:100px;background:red'></div>", width: 100)
        let image = try XCTUnwrap(root.copyImage(maxWidth: 100, rect: CGRect(x: 0, y: 0, width: 100, height: 100), scale: 1))
        XCTAssertEqual(pixel(image, x: 5, y: 5)[3], 0)
        XCTAssertEqual(pixel(image, x: 50, y: 50)[3], 255)
    }

}
