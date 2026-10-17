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
                .product(name: "CoreBridge", package: "MeguriCore"),
                .product(name: "Instrumentation", package: "MeguriCore"),
            ],
            swiftSettings: settings
        ),
        .target(name: "CardRendering", dependencies: ["DesignSystem"], swiftSettings: settings),
        .target(name: "HeatmapKit", swiftSettings: settings),
        .testTarget(
            name: "AppModelTests",
            dependencies: ["AppModel", .product(name: "Instrumentation", package: "MeguriCore")],
            swiftSettings: settings
        ),
        .testTarget(name: "DesignSystemTests", dependencies: ["DesignSystem"], swiftSettings: settings),
        .testTarget(
            name: "CardRenderingTests",
            dependencies: ["CardRendering", .product(name: "Instrumentation", package: "MeguriCore")],
            exclude: ["AGENTS.md"],
            swiftSettings: settings
        ),
    ] + features.map { name in
        .target(
            name: name,
            dependencies: ["AppModel", "DesignSystem"],
            path: "Sources/Features/\(name)",
            swiftSettings: settings
        )
    }
)
