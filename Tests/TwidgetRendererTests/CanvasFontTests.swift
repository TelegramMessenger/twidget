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

final class CanvasColorTests: XCTestCase {
    func testCachedColorsMatchAFreshParse() {
        for text in ["hsl(210,90%,60%)", "rgba(11,16,32,0.35)", "#ff3b30", "red", "not-a-colour", "hsl(400deg 50% 50% / 0.5)"] {
            let first = CSSColor.parse(text)
            let second = CSSColor.parse(text)
            XCTAssertEqual(first?.components, second?.components, text)
            XCTAssertEqual(first == nil, second == nil, text)
        }
        XCTAssertNil(CSSColor.parse("not-a-colour"))
        XCTAssertEqual(CSSColor.parse("rgba(11,16,32,0.35)")?.alpha ?? 0, 0.35, accuracy: 0.0001)
    }

    func testColorCacheStaysBounded() {
        for index in 0..<2000 {
            _ = CSSColor.parse("hsl(\(index),50%,50%)")
        }
        XCTAssertLessThanOrEqual(CSSColor.cachedCount, CSSColor.cacheLimit)
    }
}
