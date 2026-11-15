import Foundation
import HTMLScript
@_exported import Twidget3D

extension HTML3DSession: HTMLScriptLibrary {
    public static var threeSource: () -> String? = {
        return Twidget3D.threeSource()
    }

    public static func register() {
        HTMLScript.libraryFactories["three"] = { HTML3DSession() }
    }

    public func install(in context: HTMLScriptContext, limit: TimeInterval) throws {
        guard let library = HTML3DSession.threeSource() else {
            return
        }
        if let jsc = context as? JSCScriptContext {
            let bridge = install(in: jsc.context)
            try context.evaluate(library, name: "three.js", timeLimit: limit)
            try jsc.callExpression(html3DThreeAdapter, [bridge])
            return
        }
        throw HTMLScriptError(.exception, "app:three is not available on this JavaScript runtime")
    }
}

extension HTMLScript {
    public var scene3D: HTML3DSession? {
        return library(HTML3DSession.self)
    }
}
