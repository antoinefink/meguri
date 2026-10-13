import Foundation
import Testing

@testable import CoreBridge

@Suite struct VersionTests {
    /// The `[workspace.package]` version in core/Cargo.toml, read from the
    /// checkout rather than from the core.
    static func workspaceVersion() throws -> String {
        let manifest = URL(filePath: #filePath)
            .deletingLastPathComponent()  // CoreBridgeTests
            .appending(path: "../../../../../core/Cargo.toml")
            .standardized
        let text = try String(contentsOf: manifest, encoding: .utf8)
        let section = try #require(text.components(separatedBy: "[workspace.package]").dropFirst().first)
        let line = try #require(section.split(separator: "\n").first { $0.hasPrefix("version = ") })
        return line.dropFirst("version = ".count).trimmingCharacters(in: CharacterSet(charactersIn: "\" "))
    }

    @Test func versionIsTheWorkspaceVersion() async throws {
        let expected = try Self.workspaceVersion()
        #expect(try await Core.version() == expected)
    }

    @Test func swiftTestRunsAgainstTheDebugFeatureSet() async throws {
        // `swift test` uses the Debug XCFramework: test hooks and AnkiWeb on
        // (KTD18, KTD21), debug assertions on, Mac slice.
        let info = try await Core.buildInfo()
        #expect(info.version == (try Self.workspaceVersion()))
        #expect(info.features == ["ankiweb", "test-hooks"])
        #expect(info.debugAssertions)
        #expect(info.target == "aarch64-apple-darwin")
    }
}
