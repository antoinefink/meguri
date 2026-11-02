#if DEBUG && MEGURI_TEST_HOOKS && os(macOS)
@testable import Meguri
import Testing

// Generated startup predicates only; never runs the setup entry or native SDK.
@MainActor struct CloudKitAccountSetupStartupTests {
    @Test func explicitSeparateTestHostSelectionIsRequired() {
        let host = ["XCTestConfigurationFilePath": "generated", "MEGURI_CLOUD_ACCOUNT_SETUP": "1"]
        #expect(CloudAccountSetupStartup.requested(environment: host))
        #expect(!CloudAccountSetupStartup.requested(environment: [:]))
        #expect(!CloudAccountSetupStartup.requested(environment: ["MEGURI_CLOUD_ACCOUNT_SETUP": "1"]))
        #expect(!CloudAccountSetupStartup.requested(environment: ["XCTestConfigurationFilePath": "generated"]))
        #expect(!CloudAccountSetupStartup.requested(environment: host.merging(["MEGURI_CLOUD_AGENT_RUN_DIRECTORY": "/generated"]) { _, new in new }))
        #expect(!CloudAccountSetupStartup.requested(environment: host.merging(["MEGURI_CLOUD_AGENT_PATH_PREFLIGHT": "1"]) { _, new in new }))
        #expect(!CloudAccountSetupStartup.requested(environment: host.merging(["MEGURI_CLOUD_ACCOUNT_SETUP": "YES"]) { _, new in new }))
    }
}
#endif
