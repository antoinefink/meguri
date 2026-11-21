import Foundation
import XCTest

/// The app build these tests run against, read from this bundle's Info.plist
/// (MeguriUITests/Info.plist) and from the built app bytes. Physical iOS
/// runners carry those bytes as test resources; the build Mac is unreachable.
struct BuildUnderTest {
    enum Configuration: String, CaseIterable {
        case debug = "Debug", profile = "Profile", release = "Release", appStore = "AppStore"
    }

    let configuration: Configuration
    let appBundle: URL
    let repositoryRoot: URL

    static func current() throws -> BuildUnderTest {
        let bundle = Bundle(for: Marker.self)
        let info = bundle.infoDictionary ?? [:]
        let name = try XCTUnwrap(info["MeguriBuildConfiguration"] as? String, "MeguriBuildConfiguration missing")
        let configuration = try XCTUnwrap(Configuration(rawValue: name), "unknown configuration \(name)")
        #if os(iOS) && !targetEnvironment(simulator)
        let appBundle = try XCTUnwrap(bundle.resourceURL, "UI-test resources missing")
            .appending(path: "MeguriBuildUnderTest", directoryHint: .isDirectory)
        #else
        let app = try XCTUnwrap(info["MeguriAppBundlePath"] as? String, "MeguriAppBundlePath missing")
        let appBundle = URL(filePath: app, directoryHint: .isDirectory)
        #endif
        let root = try XCTUnwrap(info["MeguriRepositoryRoot"] as? String, "MeguriRepositoryRoot missing")
        let build = BuildUnderTest(
            configuration: configuration,
            appBundle: appBundle,
            repositoryRoot: URL(filePath: root, directoryHint: .isDirectory)
        )
        #if os(iOS) && !targetEnvironment(simulator)
        let metadata = try build.appInfo
        let executable = try XCTUnwrap(metadata["CFBundleExecutable"] as? String, "staged executable identifier missing")
        let validMetadata = build.hasHooks
            && (metadata["CFBundleIdentifier"] as? String) == build.expectedBundleIdentifier
            && !executable.isEmpty && !executable.hasPrefix(".") && !executable.contains("/")
            && executable != "Info.plist"
        _ = try XCTUnwrap(validMetadata ? metadata : nil, "invalid staged development app metadata")
        // Fail closed even if a dylib exists but the executable was lost. The
        // unchanged machOFiles scan below still checks all copied code bytes.
        _ = try Data(contentsOf: appBundle.appending(path: executable), options: .alwaysMapped)
        #endif
        return build
    }

    // MARK: What each configuration must be (U7, Assumptions)

    /// Debug and Profile carry the test hooks (KTD18).
    var hasHooks: Bool { configuration == .debug || configuration == .profile }

    var expectedBundleIdentifier: String {
        hasHooks ? "com.antoinefink.meguri.dev" : "com.antoinefink.meguri"
    }

    /// The name the system shows for the development identity
    /// (CFBundleDisplayName; also the app's Files folder on iOS), set by the
    /// bundle id in app/Config/Base.xcconfig.
    static let developmentDisplayName = "Meguri Dev"

    var expectedDisplayName: String {
        hasHooks ? Self.developmentDisplayName : "Meguri"
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
