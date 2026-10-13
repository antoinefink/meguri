internal import MeguriCoreBindings

/// Process-level core calls. Area services (U37 and the area units) live
/// beside this, each on its KTD9 lane.
public enum Core {
    /// The Rust workspace version.
    public static func version() async throws(BridgeError) -> String {
        try await ControlLane.shared.call("core_version") { try coreVersion() }
    }

    public static func buildInfo() async throws(BridgeError) -> CoreBuildInfo {
        try await ControlLane.shared.call("core_build_info") { CoreBuildInfo(try coreBuildInfo()) }
    }
}

/// What the linked core is: version, Rust target and compiled features.
public struct CoreBuildInfo: Sendable, Hashable {
    public let version: String
    /// For example `aarch64-apple-ios-sim`.
    public let target: String
    public let debugAssertions: Bool
    /// Sorted: `ankiweb`, `test-hooks` when compiled in (KTD18, KTD21).
    public let features: [String]

    init(_ info: MeguriCoreBindings.CoreBuildInfo) {
        version = info.version
        target = info.target
        debugAssertions = info.debugAssertions
        features = info.features
    }
}
