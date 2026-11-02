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
