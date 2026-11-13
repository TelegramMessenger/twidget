import AppKit
import XCTest
import WidgetSandbox
@testable import TwidgetRenderer

final class WidgetAnimationSecurityTests: XCTestCase {
    private let width: CGFloat = 320

    private func prepare(_ html: String) throws -> WidgetPrepared {
        _ = NSApplication.shared
        let pool = WidgetShellPool.shared
        let previousFactory = pool.factory
        pool.factory = { width in
            let renderer = LiteHTMLWidgetRenderer(theme: "light", width: width)
            renderer.preparesOffscreen = true
            return renderer
        }
        pool.prewarm(width: width)
        defer { pool.factory = previousFactory }
        let deadline = Date().addingTimeInterval(5)
        while !pool.available(width: width) && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
        let prepared = try XCTUnwrap(WidgetPreparer.prepare(html: html, theme: WidgetTheme(colorScheme: .light), width: width, options: WidgetHost.Options(), tracked: false))
        let readyDeadline = Date().addingTimeInterval(5)
        while !prepared.isReady && Date() < readyDeadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
        XCTAssertTrue(prepared.isReady)
        XCTAssertNil(prepared.host.diagnostics.dead)
        return prepared
    }

    func testReportedWidgetPrepares() throws {
        let prepared = try prepare(#"<style>@keyframes k{to{opacity:.5}}</style><div style="animation:k nans">x</div>"#)
        defer { prepared.discard() }
        XCTAssertGreaterThan(prepared.height, 0)
    }

    func testControlWidgetPrepares() throws {
        let prepared = try prepare(#"<style>@keyframes k{to{opacity:.5}}</style><div style="animation:k 1s">x</div>"#)
        defer { prepared.discard() }
        XCTAssertGreaterThan(prepared.height, 0)
    }

    func testTimeValidationPreservesNegativeDelays() throws {
        for value in ["nans", "nanms", "infs", "-infs", "1e309s", "calc(1s / 0)"] {
            XCTAssertNil(CSSTime.parse(value), value)
        }
        XCTAssertEqual(CSSTime.parse("-250ms"), -0.25)
        XCTAssertEqual(CSSTime.parse("calc(1s - 2s)"), -1)
        XCTAssertEqual(CSSTime.parse("1e3ms"), 1)
        XCTAssertTrue(CSSAnimationSpec.list(["animation": "k -1s"]).isEmpty)
        XCTAssertTrue(CSSAnimationSpec.list(["animation": "k 1s -1e30 forwards"]).isEmpty)
        XCTAssertTrue(CSSAnimationSpec.list(["animation-name": "k", "animation-iteration-count": "-1e30"]).isEmpty)
        XCTAssertEqual(CSSAnimationSpec.list(["animation": "k 1s -250ms infinite"]).first?.iterations, .infinity)
        XCTAssertEqual(CSSAnimationSpec.list(["animation": "k 1s -250ms infinite"]).first?.delay, -0.25)
    }

    func testBothSamplersHandleInvalidTimingAndPreserveValidMotion() throws {
        for declarations in [
            ["animation": "k nans"],
            ["animation-name": "k", "animation-duration": "nans"],
            ["animation": "k 1s -1e30 forwards"],
            ["animation-name": "k", "animation-iteration-count": "nan", "animation-fill-mode": "forwards"],
            ["animation": "k 1s linear -250ms infinite"],
        ] {
            let animator = WidgetAnimator()
            animator.setStyleSheets(["@keyframes k { from { opacity: 0 } to { opacity: 1 } }"])
            animator.update(declarations: [1: declarations], now: 100)
            _ = animator.peek(1, at: 100.25)
            _ = animator.sample(now: 100.25)
            _ = animator.peek(1, at: 102)
            _ = animator.sample(now: 102)
        }
        let animator = WidgetAnimator()
        animator.setStyleSheets(["@keyframes k { from { opacity: 0 } to { opacity: 1 } }"])
        animator.update(declarations: [1: ["animation": "k 1s linear -250ms infinite"]], now: 100)
        XCTAssertEqual(animator.peek(1, at: 100.25)?.opacity, "0.5")
        XCTAssertEqual(animator.sample(now: 100.25)[1]?.first?.1, "0.5")
    }

}
