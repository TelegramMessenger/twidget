// swift-tools-version:5.5

import PackageDescription

let package = Package(
    name: "LiteHTMLKit",
    platforms: [.macOS(.v10_13)],
    products: [
        .library(name: "LiteHTML", targets: ["LiteHTML"]),
        .library(name: "HTMLScript", targets: ["HTMLScript"]),
        .library(name: "HTMLSandbox", targets: ["HTMLSandbox"]),
    ],
    targets: [
        .target(
            name: "LiteHTML",
            dependencies: [],
            path: "Sources/LiteHTML",
            exclude: [
                "litehtml/LICENSE",
                "litehtml/src/gumbo/LICENSE",
            ],
            publicHeadersPath: "include",
            cSettings: [
                .unsafeFlags(["-O2"]),
                .headerSearchPath("litehtml/include"),
                .headerSearchPath("litehtml/include/litehtml"),
                .headerSearchPath("litehtml/src"),
                .headerSearchPath("litehtml/gumbo-include"),
                .headerSearchPath("core"),
            ],
            cxxSettings: [
                .unsafeFlags(["-O2"]),
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
            ]),
        .target(
            name: "CQuickJS",
            path: "Sources/CQuickJS",
            exclude: ["quickjs/LICENSE"],
            sources: ["quickjs/quickjs.c", "quickjs/dtoa.c", "quickjs/libregexp.c", "quickjs/libunicode.c", "shim/qjs_shim.c"],
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("include"),
                .headerSearchPath("quickjs"),
                .define("_GNU_SOURCE"),
                .define("QUICKJS_NG_BUILD"),
            ]),
        .target(
            name: "HTMLSandbox",
            dependencies: ["CQuickJS"],
            path: "Sources/HTMLSandbox"),
        .target(
            name: "HTMLScript",
            dependencies: ["HTMLSandbox"],
            path: "Sources/HTMLScript",
            linkerSettings: [.linkedFramework("JavaScriptCore"), .linkedFramework("Metal"), .linkedFramework("QuartzCore")]),
        .testTarget(
            name: "LiteHTMLTests",
            dependencies: ["LiteHTML"],
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
