#if DEBUG && MEGURI_TEST_HOOKS
import Foundation

// Selected before hook parsing or production collection construction. This
// flag has no effect outside an XCTest app host and grants no agent authority.
enum CloudAgentPreflight {
    static func requested(environment: [String: String] = ProcessInfo.processInfo.environment) -> Bool {
        environment["XCTestConfigurationFilePath"] != nil && environment["MEGURI_CLOUD_AGENT_PATH_PREFLIGHT"] == "1"
    }
    private(set) static var excludedNormalStartup = false
    static func excludeNormalStartup() { excludedNormalStartup = true }
}
#endif

#if DEBUG && MEGURI_TEST_HOOKS && os(macOS)
// Separate from the normal agent: no approved input is consumed or invented.
enum CloudAccountSetupStartup {
    static func requested(environment: [String: String] = ProcessInfo.processInfo.environment) -> Bool {
        environment["XCTestConfigurationFilePath"] != nil &&
        environment["MEGURI_CLOUD_ACCOUNT_SETUP"] == "1" &&
        environment["MEGURI_CLOUD_AGENT_PATH_PREFLIGHT"] == nil &&
        environment["MEGURI_CLOUD_AGENT_RUN_DIRECTORY"] == nil
    }
    private(set) static var excludedNormalStartup = false
    static func excludeNormalStartup() { excludedNormalStartup = true }
}
#endif
