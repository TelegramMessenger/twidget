import XCTest
import LiteHTML
@testable import TwidgetRenderer

final class CompositorClipTests: XCTestCase {
    private let track = WidgetCompositor.Clip(rect: CGRect(x: 0, y: 0, width: 400, height: 120), radii: [CGFloat](repeating: 12, count: 8))

    func testCornerFreeBandsHoldTheElement() {
        let both = WidgetCompositor.cornerFreeBands(track, containing: CGRect(x: 100, y: 40, width: 40, height: 40))
        XCTAssertEqual(both, [CGRect(x: 0.5, y: 12.5, width: 399, height: 95), CGRect(x: 12.5, y: 0.5, width: 375, height: 119)])
        XCTAssertEqual(WidgetCompositor.cornerFreeBands(track, containing: CGRect(x: 100, y: 2, width: 40, height: 100)), [CGRect(x: 12.5, y: 0.5, width: 375, height: 119)])
        XCTAssertEqual(WidgetCompositor.cornerFreeBands(track, containing: CGRect(x: 2, y: 40, width: 40, height: 40)), [CGRect(x: 0.5, y: 12.5, width: 399, height: 95)])
        XCTAssertTrue(WidgetCompositor.cornerFreeBands(track, containing: CGRect(x: 0, y: 0, width: 40, height: 40)).isEmpty)
    }

    func testClipIsInertOnlyAwayFromEdgesAndCorners() {
        let box = CGRect(x: 0, y: 40, width: 40, height: 40)
        XCTAssertTrue(WidgetCompositor.clipIsInert(track, reach: CGRect(x: 1, y: 13, width: 300, height: 94), box: box, ink: box, scale: 1))
        XCTAssertFalse(WidgetCompositor.clipIsInert(track, reach: CGRect(x: 0, y: 40, width: 300, height: 40), box: box, ink: box, scale: 1))
        XCTAssertFalse(WidgetCompositor.clipIsInert(track, reach: CGRect(x: 2, y: 2, width: 40, height: 40), box: box, ink: box, scale: 1))
        XCTAssertFalse(WidgetCompositor.clipIsInert(track, reach: CGRect(x: 360, y: 40, width: 60, height: 40), box: box, ink: box, scale: 1))
        XCTAssertFalse(WidgetCompositor.clipIsInert(track, reach: CGRect(x: 4, y: 40, width: 300, height: 40), box: box, ink: box.insetBy(dx: -6, dy: -6), scale: 1))
        XCTAssertFalse(WidgetCompositor.clipIsInert(track, reach: nil, box: box, ink: box, scale: 1))
        let square = WidgetCompositor.Clip(rect: track.rect, radii: [CGFloat](repeating: 0, count: 8))
        XCTAssertTrue(WidgetCompositor.clipIsInert(square, reach: CGRect(x: 0.5, y: 0.5, width: 399, height: 119), box: box, ink: box, scale: 1))
    }

    private func snapshot(_ style: String, scale: CGFloat) -> (CGImage, CGRect, CGRect)? {
        let document = LHDocument(html: "<div style='padding:30px'><div data-x='a' style='width:40px;height:40px;\(style)'></div></div>", css: nil)
        _ = document.layout(maxWidth: 200, preferredWidth: 200)
        var ink = CGRect.zero, box = CGRect.zero
        guard let image = document.copyImageOfElement(attribute: "data-x", value: "a", scale: scale, inkBounds: &ink, borderBox: &box) else {
            return nil
        }
        return (image, ink, box)
    }

    func testElementSnapshotsCropToPaintedPixels() throws {
        let (image, ink, box) = try XCTUnwrap(snapshot("border-radius:20px;background:#3390ec", scale: 2))
        XCTAssertEqual(ink, box)
        XCTAssertEqual(image.width, 80)
        XCTAssertEqual(image.height, 80)
        let (shadowed, shadowInk, shadowBox) = try XCTUnwrap(snapshot("background:#3390ec;box-shadow:0 4px 10px rgba(0,0,0,0.5)", scale: 2))
        XCTAssertTrue(shadowInk.contains(shadowBox))
        XCTAssertGreaterThan(shadowInk.width, shadowBox.width)
        XCTAssertLessThan(shadowInk.width, shadowBox.width + 96)
        XCTAssertEqual(CGFloat(shadowed.width), shadowInk.width * 2)
        let (_, transparentInk, transparentBox) = try XCTUnwrap(snapshot("", scale: 2))
        XCTAssertEqual(transparentInk, transparentBox)
    }

    func testFractionalScaleCropsOnWholePixels() throws {
        let (image, ink, box) = try XCTUnwrap(snapshot("background:#3390ec", scale: 2.5))
        XCTAssertTrue(ink.contains(box))
        XCTAssertEqual(CGFloat(image.width), ink.width * 2.5, accuracy: 0.001)
        XCTAssertEqual(CGFloat(image.height), ink.height * 2.5, accuracy: 0.001)
        XCTAssertLessThan(ink.width, box.width + 8)
    }
}
