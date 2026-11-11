#if DEBUG && MEGURI_TEST_HOOKS
@testable import CoreBridge
import Foundation
import Testing

// App-hosted ledger boundaries only. These synthetic DTOs do not supply owned
// progress or replace the runtime's real product/account/Core admission.
@MainActor @Suite(.serialized) struct CloudKitSelectedRestorationTests {
    @Test func restoredAdmissionBeginsAnEmptyLedgerAndDoesNotSupplyOriginalCallbacksOrHeldReview() throws {
        let evidence = CloudKitSmokeEvidence()
        let digest = String(repeating: "0", count: 64)
        let original = AgentRecord(header_sha256: digest, record_seq: 1, command_seq: 1, command_sha256: digest,
            agent_instance: "original", observed_at: "2026-11-10T00:00:00Z", kind: .observation,
            facts: [.init(name: .engineCallback, count: 1, engine: "engine1", operation: "sdk1-state-delivered")])
        try evidence.reserveHistoricalLabels([original])
        try evidence.setGeneration(12, selectedPublication: true)
        let current = RestoredCloudControlObservation(generation: 12, requiredScanComplete: true)
        try evidence.admitRestoredSDK(origin: "fresh-sdk", control: current)
        let facts = evidence.drain()
        #expect(facts.count == 1 && facts[0].operation == "required-scan-before-native-sdk" && facts[0].engine == "engine2")
        #expect(!facts.contains { $0.name == .engineCallback || $0.name == .stateSaved })
        #expect(throws: AgentFailure.execution) { try evidence.joinedOriginalWork() }
        #expect(throws: AgentFailure.execution) { try evidence.joined() }
        #expect(throws: AgentFailure.execution) { try evidence.admitRestoredSDK(origin: "fresh-sdk", control: current) }
    }
    @Test func restoredSDKAdmissionRejectsWrongOpeningAndMissingRequiredScan() throws {
        for control in [RestoredCloudControlObservation(generation: 11, requiredScanComplete: true),
                        RestoredCloudControlObservation(generation: 12, requiredScanComplete: false)] {
            let evidence = CloudKitSmokeEvidence()
            try evidence.setGeneration(12, selectedPublication: true)
            #expect(throws: AgentFailure.execution) { try evidence.admitRestoredSDK(origin: "fresh-sdk", control: control) }
            #expect(evidence.drain().isEmpty)
        }
        #expect(throws: AgentFailure.recovery) { try CloudKitSelectedRestoration.bytes("partial") }
        #expect(try CloudKitSelectedRestoration.bytes(String(repeating: "12", count: 32)) == Data(repeating: 0x12, count: 32))
    }
    @Test func originalMacExchangeRecoveryCannotSupplyCompletionOrCloseFromAccounting() throws {
        #expect(throws: AgentFailure.implementationMissing) {
            try AgentSelectedRestoration.requireExchangeRecovery(.syncNowMac)
        }
        try AgentSelectedRestoration.requireExchangeRecovery(.syncNowPhone)
    }
}
#endif
