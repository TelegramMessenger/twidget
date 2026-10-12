import XCTest
@testable import TwidgetRenderer

final class CanvasFontTests: XCTestCase {
    func testRepeatedFontStringsResolveOnce() {
        let first = CSSFont.make("bold 13px -apple-system")
        let second = CSSFont.make("bold 13px -apple-system")
        XCTAssertTrue(first === second)
        XCTAssertEqual(CTFontGetSize(first), 13)
    }

    func testCachedFontsMatchTheirDescription() {
        for css in ["italic 600 21.5px Georgia, serif", "12px monospace", "700 9px system-ui", "40px serif"] {
            let font = CSSFont.make(css)
            let again = CSSFont.make(css)
            XCTAssertEqual(CTFontCopyPostScriptName(font) as String, CTFontCopyPostScriptName(again) as String, css)
            XCTAssertEqual(CTFontGetSize(font), CTFontGetSize(again), css)
        }
        XCTAssertEqual(CTFontGetSize(CSSFont.make("italic 600 21.5px Georgia, serif")), 21.5)
        XCTAssertTrue(CTFontGetSymbolicTraits(CSSFont.make("italic 600 21.5px Georgia, serif")).contains(.traitItalic))
    }

    func testCacheStaysBounded() {
        for index in 0..<300 {
            _ = CSSFont.make("\(10 + Double(index) * 0.01)px sans-serif")
        }
        XCTAssertLessThanOrEqual(CSSFont.cachedCount, CSSFont.cacheLimit)
    }
}
