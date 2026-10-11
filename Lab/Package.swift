// swift-tools-version:5.5
import PackageDescription

let package = Package(
    name: "LiteHTMLLab",
    platforms: [.macOS(.v10_15)],
    dependencies: [
        .package(name: "LiteHTMLKit", path: ".."),
    ],
    targets: [
        .executableTarget(
            name: "LiteHTMLLab",
            dependencies: [.product(name: "LiteHTML", package: "LiteHTMLKit"), .product(name: "HTMLScript", package: "LiteHTMLKit"), .product(name: "HTMLScriptQuickJS", package: "LiteHTMLKit"), .product(name: "HTML3D", package: "LiteHTMLKit")],
            path: "Sources/LiteHTMLLab",
            swiftSettings: [.unsafeFlags(["-Ounchecked"])]),
        .executableTarget(
            name: "EngineLab",
            dependencies: [.product(name: "HTMLSandbox", package: "LiteHTMLKit"), .product(name: "HTMLScriptQuickJS", package: "LiteHTMLKit")],
            path: "Sources/EngineLab",
            swiftSettings: [.unsafeFlags(["-Ounchecked"])],
            linkerSettings: [.linkedFramework("JavaScriptCore")]),
        .executableTarget(
            name: "PresentLab",
            path: "Sources/PresentLab",
            swiftSettings: [.unsafeFlags(["-Ounchecked"])],
            linkerSettings: [.linkedFramework("Metal"), .linkedFramework("QuartzCore")]),
        .executableTarget(
            name: "WebKitLab",
            path: "Sources/WebKitLab",
            swiftSettings: [.unsafeFlags(["-Ounchecked"])],
            linkerSettings: [.linkedFramework("WebKit")]),
    ]
)
