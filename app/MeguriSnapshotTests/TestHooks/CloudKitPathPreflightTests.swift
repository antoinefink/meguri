#if DEBUG && MEGURI_TEST_HOOKS
@testable import Meguri
import AppModel
import Foundation
import XCTest

// Deliberately XCTest: its single, private attachment travels through MG's
// checked result-bundle transport. There is no caller-selected output pathname.
@MainActor final class CloudKitPathPreflightTests: XCTestCase {
    func testObserveOSRootAndLoadedImages() throws {
        guard CloudAgentPreflight.requested() else { throw XCTSkip("Path preparation is separately selected") }
        guard CloudAgentPreflight.excludedNormalStartup else { throw AgentFailure.binding }
        let observation = try AgentRunningProductInspector.observe()
        guard case .mac(let os) = observation.os else { throw AgentFailure.binding }
        try AgentRunDirectory.withOSApplicationSupportRoot(create: true) { root, rootChain in
            func bundle(_ value: AgentRunningProductObservation.BundleIdentity) -> [String: String] {
                ["root": value.root, "canonical_root": value.canonicalRoot,
                 "bundle_id": value.bundleID, "executable": value.executable]
            }
            let images: [[String: Any]] = observation.images.filter {
                $0.path.hasPrefix(observation.host.root + "/") || $0.path.hasPrefix(observation.host.canonicalRoot + "/")
            }.map {
                ["path": $0.path, "cpu": $0.identity.cpu, "subtype": $0.identity.subtype,
                 "uuid": $0.identity.uuid.map { String(format: "%02x", $0) }.joined()]
            }
            guard !images.isEmpty, images.count <= 128, let main = observation.images.first else { throw AgentFailure.binding }
            let value: [String: Any] = ["protocol_version": 1, "scope": "PATH-PREPARATION-ONLY",
                "application_support": root.path, "application_support_chain": rootChain, "host": bundle(observation.host), "agent": bundle(observation.agent),
                "app_build": observation.appBuild, "os_build": os, "executable_path": observation.executablePath,
                "anchor_image_path": observation.anchorImagePath, "main_image_path": main.path, "images": images]
            let bytes = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
            guard bytes.count <= 128 * 1024, try AgentRunDirectory.osApplicationSupportRoot().path == root.path,
                  try AgentRunningProductInspector.observe() == observation else { throw AgentFailure.binding }
            let attachment = XCTAttachment(data: bytes, uniformTypeIdentifier: "public.json")
            attachment.name = "cloud-agent-path-preflight.json"
            attachment.lifetime = .keepAlways
            add(attachment)
        }
    }

    func testSetupSelectionDoesNotConsumeFixtureOptions() throws {
        // Generated startup boundary only. The actual setup invocation selects
        // just testObserveOSRootAndLoadedImages, never the smoke or this test.
        XCTAssertFalse(CloudAgentPreflight.requested(environment: ["MEGURI_CLOUD_AGENT_PATH_PREFLIGHT": "1"]))
        XCTAssertTrue(CloudAgentPreflight.requested(environment: ["XCTestConfigurationFilePath": "generated", "MEGURI_CLOUD_AGENT_PATH_PREFLIGHT": "1"]))
        if CloudAgentPreflight.requested() {
            var configuration = LaunchConfiguration()
            let session = TestHooks.prepare(&configuration, arguments: ["generated", "-MeguriFixture", "/must-not-open", "-MeguriRoute", "invalid"])
            XCTAssertTrue(session.setupOnly)
            XCTAssertNil(session.shell)
            XCTAssertNil(session.error)
            XCTAssertEqual(session.options, LaunchOptions())
        }
    }
}
#endif
