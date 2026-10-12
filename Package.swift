// swift-tools-version:5.5

import PackageDescription

let localCheckout = !#filePath.contains("/checkouts/")
let optimizedDebugC: [CSetting] = localCheckout ? [.unsafeFlags(["-O2"], .when(configuration: .debug))] : []

let package = Package(
    name: "twidget",
    platforms: [.macOS(.v10_13)],
    products: [
        .library(name: "LiteHTML", targets: ["LiteHTML"]),
        .library(name: "TwidgetAssets", targets: ["TwidgetAssets"]),
        .library(name: "HTMLScript", targets: ["HTMLScript"]),
        .library(name: "HTMLSandbox", targets: ["HTMLSandbox"]),
        .library(name: "HTML3D", targets: ["HTML3D"]),
        .library(name: "Twidget3D", targets: ["Twidget3D"]),
        .library(name: "TwidgetRenderer", targets: ["TwidgetRenderer"]),
        .library(name: "CQuickJSNG", targets: ["CQuickJSNG"]),
        .library(name: "TwidgetProtocol", targets: ["TwidgetProtocol"]),
    ],
    targets: [
        .target(
            name: "TwidgetAssets",
            path: "Sources/TwidgetAssets",
            exclude: ["icons", "icons.json", "twa_icon_data.inc"],
            sources: ["twa_icons.cpp"],
            publicHeadersPath: "include"),
        .target(
            name: "LiteHTML",
            dependencies: ["TwidgetAssets"],
            path: "Sources/LiteHTML",
            exclude: [
                "litehtml/LICENSE",
                "litehtml/src/gumbo/LICENSE",
            ],
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("litehtml/include"),
                .headerSearchPath("litehtml/include/litehtml"),
                .headerSearchPath("litehtml/src"),
                .headerSearchPath("litehtml/gumbo-include"),
                .headerSearchPath("core"),
            ],
            cxxSettings: [
                .headerSearchPath("litehtml/include"),
                .headerSearchPath("litehtml/include/litehtml"),
                .headerSearchPath("litehtml/src"),
                .headerSearchPath("litehtml/gumbo-include"),
                .headerSearchPath("core"),
            ],
            linkerSettings: [
                .linkedFramework("Cocoa"),
                .linkedFramework("CoreText"),
                .linkedFramework("ImageIO"),
                .linkedFramework("Metal"),
                .linkedFramework("IOSurface"),
                .linkedFramework("CoreImage"),
            ]),
        .target(
            name: "CQuickJSNG",
            path: "Sources/CQuickJSNG",
            exclude: ["quickjs/LICENSE", "quickjs/docs", "quickjs/examples", "quickjs/tests", "quickjs/test262", "quickjs/gen"],
            sources: ["quickjs/quickjs.c", "quickjs/dtoa.c", "quickjs/libregexp.c", "quickjs/libunicode.c", "shim/qjs_shim.c"],
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("include"),
                .headerSearchPath("quickjs"),
                .define("_GNU_SOURCE"),
                .define("QUICKJS_NG_BUILD"),
                .define("QJS_DISABLE_DUMPS"),
            ] + optimizedDebugC),
        .target(
            name: "HTMLSandbox",
            path: "Sources/HTMLSandbox"),
        .target(
            name: "HTMLScript",
            dependencies: ["HTMLSandbox", "CQuickJSNG"],
            path: "Sources/HTMLScript",
            linkerSettings: [.linkedFramework("JavaScriptCore"), .linkedFramework("QuartzCore")]),
        .target(
            name: "Twidget3D",
            path: "Sources/Twidget3D",
            resources: [.copy("Resources/three.js"), .copy("Resources/twidget3d.js")],
            linkerSettings: [.linkedFramework("Metal"), .linkedFramework("QuartzCore")]),
        .target(
            name: "HTML3D",
            dependencies: ["HTMLScript", "Twidget3D"],
            path: "Sources/HTML3D",
            linkerSettings: [.linkedFramework("JavaScriptCore")]),
        .target(
            name: "TwidgetProtocol",
            path: "Sources/TwidgetProtocol"),
        .target(
            name: "TwidgetRenderer",
            dependencies: ["LiteHTML", "TwidgetProtocol"],
            path: "Sources/TwidgetRenderer",
            linkerSettings: [.linkedFramework("CoreVideo")]),
        .testTarget(
            name: "TwidgetRendererTests",
            dependencies: ["TwidgetRenderer", "TwidgetProtocol", "HTMLScript", "HTMLSandbox", "LiteHTML", "CQuickJSNG"],
            path: "Tests/TwidgetRendererTests"),
        .testTarget(
            name: "Twidget3DTests",
            dependencies: ["Twidget3D"],
            path: "Tests/Twidget3DTests"),
        .testTarget(
            name: "LiteHTMLTests",
            dependencies: ["LiteHTML", "TwidgetAssets"],
            path: "Tests/LiteHTMLTests",
            cxxSettings: [
                .headerSearchPath("../../Sources/LiteHTML/litehtml/include"),
                .headerSearchPath("../../Sources/LiteHTML/litehtml/include/litehtml"),
                .headerSearchPath("../../Sources/LiteHTML/core"),
                .headerSearchPath("../../Sources/LiteHTML/include"),
            ],
            linkerSettings: [.linkedFramework("XCTest")]),
    ],
    cxxLanguageStandard: .gnucxx17
)
