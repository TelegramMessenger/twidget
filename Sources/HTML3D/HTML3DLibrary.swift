import Foundation
import HTMLScript

extension HTML3DSession: HTMLScriptLibrary {
    public static var threeSource: () -> String? = {
        guard let url = Bundle.module.url(forResource: "three", withExtension: "js") else {
            return nil
        }
        return try? String(contentsOf: url, encoding: .utf8)
    }

    public static func register() {
        HTMLScript.libraryFactories["three"] = { HTML3DSession() }
    }

    public func install(in context: HTMLScriptContext, limit: TimeInterval) throws {
        guard let jsc = context as? JSCScriptContext else {
            throw HTMLScriptError(.exception, "app:three needs the JavaScriptCore runtime")
        }
        guard let library = HTML3DSession.threeSource() else {
            return
        }
        let bridge = install(in: jsc.context)
        try context.evaluate(library, name: "three.js", timeLimit: limit)
        try jsc.callExpression(html3DThreeAdapter, [bridge])
    }
}

extension HTMLScript {
    public var scene3D: HTML3DSession? {
        return library(HTML3DSession.self)
    }
}
