// swift-tools-version: 6.2
// MeguriCore: the Rust core behind CoreBridge (KTD9).
//
// Binaries/MeguriCoreFFI.xcframework and Sources/MeguriCoreBindings/ are
// generated and gitignored: run `tools/mg core-xcframework` (or the Xcode
// scheme pre-action) before building this package.

import PackageDescription

let package = Package(
    name: "MeguriCore",
    platforms: [.macOS("27.0"), .iOS("27.0")],
    products: [
        .library(name: "CoreBridge", targets: ["CoreBridge"]),
    ],
    targets: [
        // The Rust static library, its C header and module map.
        .binaryTarget(name: "MeguriCoreFFI", path: "Binaries/MeguriCoreFFI.xcframework"),
        // UniFFI's generated Swift. Swift 5 mode: the generated code is not
        // Swift 6 clean (bridge-build.md §1). Zero diagnostics are required.
        .target(
            name: "MeguriCoreBindings",
            dependencies: ["MeguriCoreFFI"],
            swiftSettings: [
                .swiftLanguageMode(.v5),
                .treatAllWarnings(as: .error),
            ],
            linkerSettings: [
                // rustls, the platform verifier and DNS (sync, U105).
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedLibrary("resolv"),
            ]
        ),
        // The Swift 6 facade: lanes, cancel tokens, jobs, events. Nonisolated
        // by default; it never re-exports generated types.
        .target(
            name: "CoreBridge",
            dependencies: ["MeguriCoreBindings"],
            swiftSettings: [
                .treatAllWarnings(as: .error),
            ]
        ),
        .testTarget(
            name: "CoreBridgeTests",
            dependencies: ["CoreBridge", "MeguriCoreBindings"]
        ),
    ]
)
