import XCTest
@testable import TwidgetRenderer

final class TransitionHandoffTests: XCTestCase {
    private func animator(_ transition: String, property: String = "transform", from: String = "translateX(0px)", to: String = "translateX(190px)") -> WidgetAnimator {
        let animator = WidgetAnimator()
        animator.update(declarations: [7: ["transition": transition, property: from]], now: 9)
        animator.update(declarations: [7: ["transition": transition, property: to]], now: 10)
        return animator
    }

    func testTransformTransitionBakesAndReleasesTheFrameClock() throws {
        let animator = animator("transform 1500ms linear(0, .3, 1.1, 1)")
        let baked = try XCTUnwrap(animator.bakeable(7))
        XCTAssertEqual(baked.start, 10)
        XCTAssertEqual(baked.spec.duration, 1.5)
        XCTAssertEqual(baked.spec.fill, "both")
        XCTAssertFalse(animator.isActive(excluding: [7]))
        XCTAssertEqual(animator.endTime(7), 11.5)
        let track = try XCTUnwrap(WidgetCompositor.bake(baked, base: animator.base(7), box: CGSize(width: 12, height: 12), origin: CGPoint(x: 6, y: 6)))
        XCTAssertGreaterThan(track.transforms.map(\.tx).max() ?? 0, 190)
        let reach = animator.reachableValues(7).compactMap { CSSTransform.parse($0.transform, box: .zero)?.tx }
        XCTAssertGreaterThan(reach.max() ?? 0, 208)
        for step in 0..<100 {
            let progress = Double(step) / 100
            let now = 10 + progress * 1.5
            let expected = animator.sample(now: now)[7]?.first { $0.0 == "transform" }?.1
            let actual = CSSTransform.parse(baked.value(progress).transform, box: .zero)
            let sampled = CSSTransform.parse(expected, box: .zero)
            XCTAssertEqual(actual?.tx ?? -1, sampled?.tx ?? -2, accuracy: 0.002)
            XCTAssertEqual(animator.peek(7, at: now)?.transform, expected)
        }
    }

    func testCompletionEventStillFiresExactlyOnce() throws {
        let animator = animator("opacity 1s linear", property: "opacity", from: "0", to: "1")
        var events: [String] = []
        animator.onEvent = { kind, _, property, duration in events.append("\(kind):\(property):\(duration)") }
        XCTAssertNotNil(animator.bakeable(7))
        _ = animator.sample(now: 11.002)
        _ = animator.sample(now: 12)
        XCTAssertEqual(events, ["transitionend:opacity:1.0"])
        XCTAssertNil(animator.bakeable(7))
        XCTAssertFalse(animator.isActive)
    }

    func testPositiveAndNegativeDelaysPreserveTiming() throws {
        for delay in [-0.4, 0.4] {
            let animator = animator("opacity 1s linear \(delay)s", property: "opacity", from: "0", to: "1")
            let baked = try XCTUnwrap(animator.bakeable(7))
            XCTAssertEqual(baked.spec.delay, delay)
            XCTAssertEqual(animator.endTime(7) ?? 0, 11 + delay, accuracy: 1e-9)
            XCTAssertEqual(Double(animator.peek(7, at: 10)?.opacity ?? "") ?? -1, max(0, -delay), accuracy: 1e-9)
        }
    }

    func testReversalStartsAtCurrentPositionAndChangesSignature() throws {
        let animator = animator("transform 1s linear")
        let original = try XCTUnwrap(animator.bakeable(7))
        animator.update(declarations: [7: ["transition": "transform 1s linear", "transform": "translateX(0px)"]], now: 10.4)
        let reversed = try XCTUnwrap(animator.bakeable(7))
        XCTAssertNotEqual(original.spec.name, reversed.spec.name)
        XCTAssertEqual(CSSTransform.parse(reversed.value(0).transform, box: .zero)?.tx ?? -1, 76, accuracy: 0.001)
        XCTAssertEqual(CSSTransform.parse(reversed.value(0.5).transform, box: .zero)?.tx ?? -1, 38, accuracy: 0.001)
        XCTAssertEqual(animator.endTime(7) ?? 0, 11.4, accuracy: 1e-9)
    }

    func testReplayWithoutTransitionCancelsNativeTrack() {
        let animator = animator("transform 1s linear")
        animator.update(declarations: [7: ["transition": "none", "transform": "translateX(0px)"]], now: 10.3)
        XCTAssertNil(animator.bakeable(7))
        XCTAssertFalse(animator.isActive)
    }

    func testMixedAndLayoutTransitionsKeepExistingSampling() {
        XCTAssertNil(animator("left 1s linear", property: "left", from: "0px", to: "100px").bakeable(7))
        let animator = WidgetAnimator()
        let before = ["transition": "all 1s linear", "transform": "translateX(0px)", "opacity": "0"]
        var after = before
        after["transform"] = "translateX(100px)"
        after["opacity"] = "1"
        animator.update(declarations: [7: before], now: 9)
        animator.update(declarations: [7: after], now: 10)
        XCTAssertNil(animator.bakeable(7))
        XCTAssertTrue(animator.isActive(excluding: []))
    }
}
