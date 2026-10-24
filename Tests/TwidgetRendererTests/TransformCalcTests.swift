import XCTest
@testable import TwidgetRenderer

final class TransformCalcTests: XCTestCase {
    func testCarouselPositionsAndDragsAtDifferentWidths() throws {
        for width in [280.0, 400, 640] {
            for index in 0...4 {
                for drag in [-120.0, 0, 75] {
                    let text = "translateX(calc(11% - \(index) * (78% + 10px) + \(drag)px))"
                    let resolved = try XCTUnwrap(CSSTransform.resolve(text) { _ in nil })
                    let actual = try XCTUnwrap(CSSTransform.parse(resolved, box: CGSize(width: width, height: 200)))
                    let expected = width * 0.11 - Double(index) * (width * 0.78 + 10) + drag
                    XCTAssertEqual(actual.tx, expected, accuracy: 0.00001, text)
                    XCTAssertEqual(actual.ty, 0)
                }
            }
        }
    }

    func testTranslationsResolveEachAxisAndNestedCalc() throws {
        let text = "translate(calc(50% - calc(2 * 10px)), calc((100% + 8px) / 2)) scale(2)"
        let resolved = try XCTUnwrap(CSSTransform.resolve(text) { _ in nil })
        for size in [CGSize(width: 400, height: 100), CGSize(width: 200, height: 300)] {
            let transform = try XCTUnwrap(CSSTransform.parse(resolved, box: size))
            XCTAssertEqual(transform.tx, size.width * 0.5 - 20, accuracy: 0.00001)
            XCTAssertEqual(transform.ty, (size.height + 8) / 2, accuracy: 0.00001)
            XCTAssertEqual(transform.a, 2)
        }
        let spaced = try XCTUnwrap(CSSTransform.parse("translate(calc(50% - 20px) calc(25% + 2px))", box: CGSize(width: 400, height: 100)))
        XCTAssertEqual(spaced.tx, 180)
        XCTAssertEqual(spaced.ty, 27)
    }

    func testCalcVariablesAndMultipleFunctions() throws {
        let text = "translateX(calc(var(--offset) * (50% + 10px))) translateY(calc(25% - 4px))"
        let resolved = try XCTUnwrap(CSSTransform.resolve(text) { $0 == "--offset" ? "2" : nil })
        let transform = try XCTUnwrap(CSSTransform.parse(resolved, box: CGSize(width: 400, height: 200)))
        XCTAssertEqual(transform.tx, 420)
        XCTAssertEqual(transform.ty, 46)
        XCTAssertEqual(Calc.evaluate("50% / 2"), "25.000000%")
        XCTAssertEqual(Calc.evaluate("10px - -5px"), "15.000000px")
    }

    func testInvalidDimensionsAndArithmeticAreRejected() {
        for expression in ["50% + 10deg", "50% * 2px", "10px / 0", "1e308 * 1e308", "50% +", "calc(1px", "calc(1px) junk"] {
            XCTAssertNil(Calc.evaluate(expression), expression)
            XCTAssertNil(CSSTransform.parse("translateX(calc(\(expression)))", box: CGSize(width: 400, height: 200)), expression)
        }
    }
}
