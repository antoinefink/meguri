import Foundation
import XCTest

/// The app build these tests run against, read from this bundle's Info.plist
/// (MeguriUITests/Info.plist) and from the built app on disk.
struct BuildUnderTest {
    enum Configuration: String, CaseIterable {
        case debug = "Debug", profile = "Profile", release = "Release", appStore = "AppStore"
    }

    let configuration: Configuration
    let appBundle: URL
    let repositoryRoot: URL

    static func current() throws -> BuildUnderTest {
        let info = Bundle(for: Marker.self).infoDictionary ?? [:]
        let name = try XCTUnwrap(info["MeguriBuildConfiguration"] as? String, "MeguriBuildConfiguration missing")
        let configuration = try XCTUnwrap(Configuration(rawValue: name), "unknown configuration \(name)")
        let app = try XCTUnwrap(info["MeguriAppBundlePath"] as? String, "MeguriAppBundlePath missing")
        let root = try XCTUnwrap(info["MeguriRepositoryRoot"] as? String, "MeguriRepositoryRoot missing")
        return BuildUnderTest(
            configuration: configuration,
            appBundle: URL(filePath: app, directoryHint: .isDirectory),
            repositoryRoot: URL(filePath: root, directoryHint: .isDirectory)
        )
    }

    // MARK: What each configuration must be (U7, Assumptions)

    /// Debug and Profile carry the test hooks (KTD18).
    var hasHooks: Bool { configuration == .debug || configuration == .profile }

    var expectedBundleIdentifier: String {
        hasHooks ? "com.antoinefink.meguri.dev" : "com.antoinefink.meguri"
    }

    var expectedAppGroup: String {
        #if os(macOS)
        "DNQ34W43XX." + expectedBundleIdentifier
        #else
        "group." + expectedBundleIdentifier
        #endif
    }

    /// The Rust features of the linked core (KTD18, KTD21).
    var expectedCoreFeatures: String {
        switch configuration {
        case .debug, .profile: "ankiweb,test-hooks"
        case .release: "ankiweb"
        case .appStore: ""
        }
    }

    // MARK: The built app

    var appInfo: [String: Any] {
        get throws {
            #if os(macOS)
            let url = appBundle.appending(path: "Contents/Info.plist")
            #else
            let url = appBundle.appending(path: "Info.plist")
            #endif
            let data = try Data(contentsOf: url)
            return try XCTUnwrap(PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any])
        }
    }

    var bundleIdentifier: String {
        get throws { try XCTUnwrap(appInfo["CFBundleIdentifier"] as? String) }
    }

    /// The app's executable and the dylibs beside it (Debug builds keep the
    /// code in `Meguri.debug.dylib`).
    var machOFiles: [URL] {
        get throws {
            #if os(macOS)
            let directory = appBundle.appending(path: "Contents/MacOS", directoryHint: .isDirectory)
            #else
            let directory = appBundle
            #endif
            let executable = try XCTUnwrap(appInfo["CFBundleExecutable"] as? String)
            let names = try FileManager.default.contentsOfDirectory(atPath: directory.path(percentEncoded: false))
            return names.filter { $0 == executable || $0.hasSuffix(".dylib") }.sorted().map { directory.appending(path: $0) }
        }
    }

    /// A committed fixture under app/Fixtures/TestHooks. Fixtures live in the
    /// checkout: launched outside XCTest, a sandboxed Mac hook build may read
    /// only there (Meguri-macOS-Dev*.entitlements), and the Mac test runner,
    /// sandboxed too, cannot write fixtures anywhere the app can read.
    func hookFixture(_ name: String) -> URL {
        repositoryRoot.appending(path: "app/Fixtures/TestHooks/\(name)")
    }

    private final class Marker {}
}
