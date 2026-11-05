import XCTest
import LiteHTML

final class V030LayoutTests: XCTestCase {
    private func document(_ source: String, width: CGFloat = 374) -> LHDocument {
        let doc = LHDocument(html: "<style>body{margin:0;font:13px system-ui}*{box-sizing:border-box}</style>" + source, css: nil)
        _ = doc.layout(maxWidth: width, preferredWidth: width)
        return doc
    }

    func testAspectRatioUsesClampedWidthAndGridMarksStayInEqualCells() {
        for marked in [false, true] {
            let cells = (0..<9).map { i in "<button id=c\(i)>\(marked && i == 4 ? "<svg id=mark viewBox='0 0 24 24'><circle cx=12 cy=12 r=10 /></svg>" : "")</button>" }.joined()
            let doc = document("""
            <style>#board{position:relative;max-width:264px;margin:auto;aspect-ratio:1}#grid{position:absolute;inset:0;display:grid;grid-template:repeat(3,1fr)/repeat(3,1fr)}button{padding:0;border:0;display:grid;place-items:center}svg{width:58%;height:58%}</style>
            <div id=board><div id=grid>\(cells)</div></div>
            """)
            let board = doc.bounds(matching: "#board")
            XCTAssertEqual(board.width, 264, accuracy: 0.1)
            XCTAssertEqual(board.height, 264, accuracy: 0.1)
            for i in 0..<9 {
                let cell = doc.bounds(matching: "#c\(i)")
                XCTAssertEqual(cell.width, 88, accuracy: 0.1)
                XCTAssertEqual(cell.height, 88, accuracy: 0.1)
                XCTAssertEqual(cell.minX - board.minX, CGFloat(i % 3) * 88, accuracy: 0.1)
                XCTAssertEqual(cell.minY - board.minY, CGFloat(i / 3) * 88, accuracy: 0.1)
            }
            if marked {
                let cell = doc.bounds(matching: "#c4")
                let mark = doc.bounds(matching: "#mark")
                XCTAssertEqual(mark.width, 51.04, accuracy: 0.1)
                XCTAssertEqual(mark.height, 51.04, accuracy: 0.1)
                XCTAssertEqual(mark.midX, cell.midX, accuracy: 0.1)
                XCTAssertEqual(mark.midY, cell.midY, accuracy: 0.1)
            }
        }
    }

    func testGridSpanningPercentageFillDoesNotEnlargeAutoCountColumn() {
        for percent in [0, 35, 100] {
            let doc = document("""
            <style>#row{display:grid;grid-template-columns:52px 1fr auto;align-items:center;width:100%;padding:8px 8px 10px 0;border:0}.bar{grid-column:2/4;height:6px}.bar i{display:block;height:100%;width:\(percent)%}</style>
            <button id=row><span>50%</span><span>Option</span><span id=count>248</span><span class=bar id=bar><i></i></span></button>
            """)
            let row = doc.bounds(matching: "#row")
            let count = doc.bounds(matching: "#count")
            let bar = doc.bounds(matching: "#bar")
            XCTAssertEqual(count.maxX, row.maxX - 8, accuracy: 0.2)
            XCTAssertEqual(bar.minX, row.minX + 52, accuracy: 0.2)
            XCTAssertEqual(bar.maxX, row.maxX - 8, accuracy: 0.2)
        }
    }
    func testSVGFillBoxScalesAroundTheGeometryAfterItMoves() {
        for center in [CGPoint(x: 180, y: 20), CGPoint(x: 130, y: 70)] {
            let doc = document("""
            <svg style='display:block' width=240 height=100 viewBox='0 0 240 100'><circle id=dot cx='\(center.x)' cy='\(center.y)' r=3 style='transform-box:fill-box;transform-origin:center;transform:scale(3)'/></svg>
            """)
            let dot = doc.svgBox(attribute: "id", value: "dot")
            XCTAssertEqual(dot.midX, center.x, accuracy: 0.1)
            XCTAssertEqual(dot.midY, center.y, accuracy: 0.1)
            XCTAssertEqual(dot.width, 18, accuracy: 0.1)
            XCTAssertEqual(dot.height, 18, accuracy: 0.1)
        }
    }

    func testFillBoxUsesCurveExtremaRatherThanControlPoints() {
        let doc = document("""
        <svg width=300 height=200 viewBox='-50 -50 300 200'><path id=curve d='M0 0 Q100 100 200 0' style='transform-box:fill-box;transform-origin:center;transform:scale(2)'/></svg>
        """)
        let curve = doc.svgBox(attribute: "id", value: "curve")
        XCTAssertEqual(curve.midX, 150, accuracy: 0.2)
        XCTAssertEqual(curve.midY, 75, accuracy: 0.2)
        XCTAssertEqual(curve.width, 400, accuracy: 0.2)
        XCTAssertEqual(curve.height, 100, accuracy: 0.2)
    }

}
