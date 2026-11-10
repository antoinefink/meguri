#if DEBUG && MEGURI_TEST_HOOKS && os(macOS)
@testable import Meguri
import AppModel
import CloudKit
import CoreBridge
import Darwin
import Foundation
import XCTest

// Explicit owner selection only. Never include this class in a normal plan's
// selectedTests; Snapshot excludes it. Failure messages are fixed and no test
// attachment, account value, SDK description or private path is recorded.
@MainActor final class CloudKitAccountSetupTests: XCTestCase {
    func testOwnerSetup() async {
        guard CloudAccountSetupStartup.requested(), CloudAccountSetupStartup.excludedNormalStartup else {
            // An accidental normal selection cannot perform any account lookup.
            XCTFail("Owner setup requires its separate startup selection")
            return
        }
        var phase = OwnerSetupPhase.configuration
        var originalChannel: OwnerSetupChannel?
        do {
            guard let configuration = CloudBuildConfiguration.fromBundle,
                  case .development = configuration.environment, configuration.schemaConfigured,
                  let path = ProcessInfo.processInfo.environment["MEGURI_CLOUD_ACCOUNT_DIRECTORY"] else {
                throw AgentFailure.binding
            }
            phase = .product
            let original = try AgentRunningProductInspector.observe()
            phase = .channel
            let channel = try OwnerSetupChannel(path: path)
            originalChannel = channel
            defer { channel.close() }
            func observation() throws -> [String: Any] {
                phase = .observationChannel
                try channel.check()
                phase = .observationProduct
                let actual = try AgentRunningProductInspector.observe()
                phase = .observationMatch
                try actual.requireSameOwnedProduct(as: original)
                phase = .observationConfiguration
                guard let current = CloudBuildConfiguration.fromBundle,
                      current.container == configuration.container, current.environment == configuration.environment,
                      current.schemaConfigured == configuration.schemaConfigured else { throw AgentFailure.binding }
                phase = .observationEncode
                return try OwnerSetupObservation.encode(actual)
            }
            let initialObservation = try observation()
            phase = .helloSend
            try channel.send(["pid": getpid(), "container": configuration.container, "observation": initialObservation])
            // Only the original parent, after actual product/signature/profile,
            // destination and loaded-image admission, sends this first command.
            phase = .captureCommand
            guard try channel.action() == "capture" else { throw AgentFailure.binding }
            let container = CKContainer(identifier: configuration.container)
            func read() async throws -> [String: Any] {
                _ = try observation()
                var status = "failed", identity: [UInt8] = []
                do {
                    phase = .accountStatus
                    switch try await container.accountStatus() {
                    case .available:
                        phase = .accountIdentity
                        let record = try await container.userRecordID()
                        identity = Array(record.recordName.utf8) // Exact V9.1 encoding.
                        guard !identity.isEmpty else { throw AgentFailure.binding }
                        status = "available"
                    case .noAccount: status = "no-account"
                    default: status = "unavailable"
                    }
                } catch {
                    // SDK errors and any private markers never reach XCTest.
                    status = "failed"; identity = []
                }
                try Task.checkCancellation()
                return ["status": status, "identity": identity, "observation": try observation()]
            }
            let captured = try await read()
            phase = .captureSend
            try channel.send(captured)
            phase = .freshCommand
            guard try channel.action() == "fresh" else { throw AgentFailure.binding }
            let fresh = try await read()
            phase = .freshSend
            try channel.send(fresh)
            while true {
                phase = .nextCommand
                switch try channel.action() {
                case "hold":
                    let current = try observation()
                    phase = .holdSend
                    try channel.send(["observation": current])
                case "finish":
                    _ = try observation()
                    phase = .finishSend
                    try channel.send(["finished": true])
                    return
                default: throw AgentFailure.binding
                }
            }
        } catch {
            let checkpoint = originalChannel?.checkpoint.rawValue ?? "none"
            let marker = "MG_OWNER_APP_DIAGNOSTIC {\"phase\":\"\(phase.rawValue)\",\"channel\":\"\(checkpoint)\"}\n"
            try? FileHandle.standardError.write(contentsOf: Data(marker.utf8))
            XCTFail("Owner setup refused; no signed execution evidence")
        }
    }
}

private enum OwnerSetupPhase: String {
    case configuration, product, channel, helloSend, captureCommand
    case observationChannel, observationProduct, observationMatch, observationConfiguration, observationEncode
    case accountStatus, accountIdentity, captureSend, freshCommand, freshSend, nextCommand, holdSend, finishSend
}

// Same actual loaded-product observer as preflight/normal CloudAgent, with the
// same bounded private observation consumed by admit_preflight on the parent.
@MainActor private enum OwnerSetupObservation {
    static func encode(_ observation: AgentRunningProductObservation) throws -> [String: Any] {
        guard case .mac(let os) = observation.os else { throw AgentFailure.binding }
        let main = try observation.executableImage()
        return try AgentRunDirectory.withOSApplicationSupportRoot(create: true) { root, chain in
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
            guard !images.isEmpty, images.count <= 128 else { throw AgentFailure.binding }
            return ["protocol_version": 1, "scope": "PATH-PREPARATION-ONLY",
                "application_support": root.path, "application_support_chain": chain,
                "host": bundle(observation.host), "agent": bundle(observation.agent),
                "app_build": observation.appBuild, "os_build": os, "executable_path": observation.executablePath,
                "anchor_image_path": observation.anchorImagePath, "main_image_path": main.path, "images": images]
        }
    }
}

#endif
