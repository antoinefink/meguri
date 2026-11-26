// swift-tools-version: 6.2
// MeguriKit: the app's Swift modules (KTD10). MainActor default isolation;
// the Rust core is reached only through MeguriCore's CoreBridge (KTD9).
//
// Packages build Release and AppStore alike and never see the app's
// compilation conditions (MEGURI_TEST_HOOKS, ANKIWEB_ENABLED): those stay in
// the app target, which injects into the seams here (KTD18, KTD21).

import PackageDescription

let settings: [SwiftSetting] = [
    .defaultIsolation(MainActor.self),
    .treatAllWarnings(as: .error),
]

/// Feature modules, one per `Sources/Features/<Name>/`.
let features = [
    "Home", "Review", "Browse", "CardInfo", "Editor", "NoteTypes",
    "ImageOcclusion", "DeckOptions", "Stats", "Settings", "Onboarding",
    "Integrations",
]

let package = Package(
    name: "MeguriKit",
    defaultLocalization: "en",
    platforms: [.macOS("27.0"), .iOS("27.0")],
    products: [
        // Everything the app links. A new target joins this product, so the
        // Xcode project needs no edit.
        .library(
            name: "MeguriKit",
            targets: ["DesignSystem", "AppModel", "CardRendering", "HeatmapKit"] + features
        ),
        // The widget's only module: core-free (KTD16).
        .library(name: "HeatmapKit", targets: ["HeatmapKit"]),
    ],
    dependencies: [
        .package(path: "../MeguriCore"),
    ],
    targets: [
        .target(name: "DesignSystem", resources: [.process("Resources")], swiftSettings: settings),
        .target(
            name: "AppModel",
            dependencies: [
                "CardRendering",
                .product(name: "CoreBridge", package: "MeguriCore"),
                .product(name: "Instrumentation", package: "MeguriCore"),
            ],
            exclude: ["Review/AGENTS.md"],
            resources: [.process("Sync/Resources")],
            swiftSettings: settings
        ),
        .target(
            name: "CardRendering",
            dependencies: [
                "DesignSystem",
                .product(name: "CoreBridge", package: "MeguriCore"),
                .product(name: "Instrumentation", package: "MeguriCore"),
            ],
            exclude: ["AGENTS.md", "Resources/CardRuntime/AGENTS.md"],
            // Copied flat into the bundle; CardRuntime serves them under
            // /_anki/ (specs/render/card-runtime.md).
            resources: [
                .process("Resources/CardBehavior.xcstrings"),
                .copy("Resources/CardRuntime/page.html"),
                .copy("Resources/CardRuntime/base.css"),
                .copy("Resources/CardRuntime/runtime.js"),
                .copy("Resources/CardRuntime/image-occlusion.js"),
                .copy("Resources/CardRuntime/interaction.js"),
                .copy("Resources/CardRuntime/review-host.js"),
                .copy("Resources/CardRuntime/vendor/jquery-3.7.1.min.js"),
            ],
            swiftSettings: settings
        ),
        .target(name: "HeatmapKit", swiftSettings: settings),
        .testTarget(
            name: "AppModelTests",
            dependencies: ["AppModel", "CardRendering", "Editor", "ImageOcclusion", "HeatmapKit", .product(name: "CoreBridge", package: "MeguriCore"), .product(name: "Instrumentation", package: "MeguriCore")],
            exclude: ["AGENTS.md"],
            resources: [.copy("Fixtures/schema-18.anki2"), .copy("Fixtures/review.colpkg"), .copy("Fixtures/deck-tree.apkg")],
            swiftSettings: settings
        ),
        .testTarget(name: "DesignSystemTests", dependencies: ["DesignSystem"], swiftSettings: settings),
        .testTarget(name: "ImageOcclusionTests", dependencies: ["ImageOcclusion", .product(name: "CoreBridge", package: "MeguriCore")], swiftSettings: settings),
        .testTarget(name: "HomeTests", dependencies: ["Home"], swiftSettings: settings),
        .testTarget(name: "DeckOptionsTests", dependencies: ["DeckOptions", "AppModel"], swiftSettings: settings),
        .testTarget(name: "BrowseTests", dependencies: ["Browse", "AppModel", .product(name: "CoreBridge", package: "MeguriCore")], swiftSettings: settings),
        .testTarget(name: "CardInfoTests", dependencies: ["CardInfo", "AppModel", .product(name: "CoreBridge", package: "MeguriCore")], swiftSettings: settings),
        .testTarget(name: "NoteTypesTests", dependencies: ["NoteTypes", "AppModel", .product(name: "CoreBridge", package: "MeguriCore")], swiftSettings: settings),
        .testTarget(name: "HeatmapKitTests", dependencies: ["HeatmapKit"], swiftSettings: settings),
        .testTarget(name: "StatsTests", dependencies: ["Stats", "AppModel", "HeatmapKit", .product(name: "CoreBridge", package: "MeguriCore")], swiftSettings: settings),
        .testTarget(name: "ReviewTests", dependencies: ["Review", "AppModel", "DesignSystem", "Settings"], swiftSettings: settings),
        .testTarget(
            name: "CardRenderingTests",
            dependencies: [
                "AppModel",
                "CardRendering",
                .product(name: "CoreBridge", package: "MeguriCore"),
                .product(name: "Instrumentation", package: "MeguriCore"),
            ],
            exclude: ["AGENTS.md"],
            resources: [.copy("Fixtures")],
            swiftSettings: settings
        ),
    ] + features.map { name in
        .target(
            name: name,
            dependencies: ["AppModel", "DesignSystem"] + (["Review", "Browse"].contains(name) ? ["CardRendering"] : [])
                // Browse's inspector shows the note in the round 5 editor (U273) and its Card Info (U274).
                + (name == "Browse" ? ["Editor", "CardInfo"] : []) + (name == "Editor" ? ["ImageOcclusion"] : []) + (["Home", "Stats"].contains(name) ? ["HeatmapKit"] : []) + (["Home", "Review", "Settings", "Editor", "DeckOptions", "NoteTypes", "Browse", "Stats", "CardInfo", "ImageOcclusion"].contains(name) ? [.product(name: "CoreBridge", package: "MeguriCore")] : []) + (["Editor", "Stats"].contains(name) ? [.product(name: "Instrumentation", package: "MeguriCore")] : []),
            path: "Sources/Features/\(name)",
            exclude: ["ImageOcclusion", "Editor"].contains(name) ? ["AGENTS.md"] : [],
            resources: ["Home", "Review", "Settings", "Editor", "DeckOptions", "NoteTypes", "Browse", "Stats", "CardInfo", "ImageOcclusion"].contains(name) ? [.process("Resources")] : [],
            swiftSettings: settings
        )
    }
)
