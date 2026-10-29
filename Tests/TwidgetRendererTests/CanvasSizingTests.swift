import XCTest
import AppKit
import LiteHTML
import WidgetSandbox
@testable import TwidgetRenderer

final class CanvasSizingTests: XCTestCase {
    private func bounds(_ markup: String, width: CGFloat = 440) -> CGRect {
        let document = LHDocument(html: "<style>body{margin:0}</style>" + markup, css: nil)
        _ = document.layout(maxWidth: width, preferredWidth: width)
        return document.bounds(matching: "#target")
    }

    func testResponsiveCanvasPreservesBitmapAspectRatio() {
        for width: CGFloat in [320, 390, 400, 440, 500] {
            let box = bounds("<canvas id=target width=400 height=500 style='display:block;width:100%'></canvas>", width: width)
            XCTAssertEqual(box.width, width, accuracy: 0.01)
            XCTAssertEqual(box.height, width * 1.25, accuracy: 0.01)
        }
    }

    func testCanvasIntrinsicSizeWithoutDrawing() {
        for (attributes, size) in [("", CGSize(width: 300, height: 150)), ("width=400 height=500", CGSize(width: 400, height: 500)), ("width=200", CGSize(width: 200, height: 150)), ("width=-1 height=invalid", CGSize(width: 300, height: 150)), ("width=0 height=0", .zero)] {
            let box = bounds("<canvas id=target \(attributes) style='display:block'></canvas>")
            XCTAssertEqual(box.width, size.width, accuracy: 0.01, attributes)
            XCTAssertEqual(box.height, size.height, accuracy: 0.01, attributes)
        }
    }

    func testCanvasCSSHeightAndExplicitStretching() {
        let height = bounds("<canvas id=target width=400 height=500 style='display:block;height:250px'></canvas>")
        XCTAssertEqual(height.width, 200, accuracy: 0.01)
        XCTAssertEqual(height.height, 250, accuracy: 0.01)
        let stretched = bounds("<canvas id=target width=400 height=500 style='display:block;width:300px;height:250px'></canvas>")
        XCTAssertEqual(stretched.width, 300, accuracy: 0.01)
        XCTAssertEqual(stretched.height, 250, accuracy: 0.01)
        let clamped = bounds("<canvas id=target width=400 height=500 style='display:block;max-width:200px'></canvas>")
        XCTAssertEqual(clamped.width, 200, accuracy: 0.01)
        XCTAssertEqual(clamped.height, 250, accuracy: 0.01)
    }

    func testImagePresentationDimensionsStillApply() {
        let intrinsic = bounds("<img id=target width=400 height=500 style='display:block'>")
        XCTAssertEqual(intrinsic.width, 400, accuracy: 0.01)
        XCTAssertEqual(intrinsic.height, 500, accuracy: 0.01)
        let responsive = bounds("<img id=target width=400 height=500 style='display:block;width:100%'>")
        XCTAssertEqual(responsive.width, 440, accuracy: 0.01)
        XCTAssertEqual(responsive.height, 500, accuracy: 0.01)
    }

    private func pump(_ seconds: TimeInterval) {
        let end = Date().addingTimeInterval(seconds)
        while Date() < end { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    }

    func testUpClickUsesResponsiveCanvasCoordinates() throws {
        let html = #"""
        <style>body{margin:0}canvas{display:block;width:100%}</style><canvas id=c width=400 height=500></canvas>
        <script>
        var cv=document.getElementById('c'),x=cv.getContext('2d'),run=1,q=[],d=[1,0];
        x.fillRect(165,346,70,48);
        function mid(){run=run==1?2:1}
        function turn(a,b){if(run!=1)return;var l=q.length?q[q.length-1]:d;if(!(a*l[0]+b*l[1])&&q.length<3)q.push([a,b])}
        cv.addEventListener('pointerdown',function(e){var r=cv.getBoundingClientRect(),k=400/r.width,px=(e.clientX-r.left)*k,py=(e.clientY-r.top)*k;
        if(py<344)mid();else{var dx=px-200,dy=py-420;if(Math.abs(dx)<35&&Math.abs(dy)<24)mid();else Math.abs(dx)>Math.abs(dy)*1.4?turn(dx<0?-1:1,0):turn(0,dy<0?-1:1)}
        document.documentElement.setAttribute('data-conformance',JSON.stringify({run:run,q:q,y:py}));});
        </script>
        """#
        for width: CGFloat in [320, 390, 400, 440, 500] {
            let session = try ConformanceRunnerTests.Session(html: html, theme: "light", features: [], width: width, maximumHeight: 900)
            defer { session.close() }
            let end = Date().addingTimeInterval(5)
            while !session.host.diagnostics.rendered && Date() < end { pump(0.01) }
            let renderer = session.probe.inner
            let canvas = try XCTUnwrap(renderer.dom.nodes.values.first { $0.attribute("id") == "c" })
            let box = try XCTUnwrap(renderer.elementBounds()[renderer.dom.publicId(canvas)])
            let point = CGPoint(x: box.midX, y: box.minY + box.height * 370 / 500)
            session.tap(point, pump: pump)
            let deadline = Date().addingTimeInterval(2)
            while session.probe.published == nil && Date() < deadline { pump(0.01) }
            let result = try XCTUnwrap(session.probe.published)
            XCTAssertEqual(result["run"] as? Int, 1, "width \(width)")
            XCTAssertEqual(result["q"] as? [[Int]], [[0, -1]], "width \(width)")
            XCTAssertEqual(result["y"] as? Double ?? 0, 370, accuracy: 0.01)
            XCTAssertNil(session.host.diagnostics.dead)
        }
    }
}
