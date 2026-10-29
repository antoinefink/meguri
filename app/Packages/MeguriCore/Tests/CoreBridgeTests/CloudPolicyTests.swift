import Dispatch
import Foundation
import Instrumentation
import MeguriCoreBindings
import Testing
@testable import CoreBridge

extension EventTests {
    @Test func policyUsesActualRetainedHostAndSamplesOnlyItsPlatformProvider() async throws {
        try await withCloudConnection { fixture in
            let policy = try await fixture.workflow.policy()
            _ = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
            _ = try await policy.refreshAccount()
            _ = try await policy.input(.network(reachable: true))
            let status = try await policy.status()
            #expect(status.accountStatus == .available)
            #expect(status.syncNow == .unavailable(.notConnected))
            #expect(status.asOfMilliseconds == nil && status.lastCompleted == nil)
            #expect(status.media.uploadFiles == nil && status.media.downloadFiles == nil)
            // Negative control: no binding or collection authority is created
            // by an account read, settings, reachability or Sync Now input.
            if case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) {
                #expect(!commands.contains { if case .runExchange = $0.action { true } else { false } })
            } else { Issue.record("unexpected policy failure") }
            try await fixture.collection.close()
            do { _ = try await policy.input(.foreground); Issue.record("closed policy followed successor engine") }
            catch { #expect(error as? BridgeError == .cloudPolicy(.closed)) }
        }
    }

    @Test func policyHandleIsUsableThroughActualUniFFIMethodsAndRejectsAfterClose() async throws {
        try await withCloudConnection { fixture in
            let raw = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
            let policy = try await MaintenanceLane.shared.call(.cloudWorkflowPolicy) { [raw] in try raw.policy() }
            _ = try await MaintenanceLane.shared.call(.cloudPolicyRefreshAccount) { [policy] in try policy.refreshAccount() }
            let status = try await MaintenanceLane.shared.call(.cloudPolicyStatus) { [policy] in try policy.status() }
            #expect(status.accountStatus == .available)
            try await fixture.collection.close()
            do {
                _ = try await MaintenanceLane.shared.call(.cloudPolicyStatus) { [policy] in try policy.status() }
                Issue.record("retained raw policy accepted a closed Host")
            } catch { #expect(error as? BridgeError == .cloudPolicy(.closed)) }
        }
    }
}

@Suite struct CloudPolicyMappingTests {
    @Test @MainActor func policyCallSelectorRunsOnInstrumentedMaintenanceLane() async throws {
        let ran = try await CoreCloudPolicyService.call(.cloudPolicyStatus, probe)
        #expect(ran.queueLabel == Lane.maintenance.queueLabel && !ran.isMainThread)
        #expect(ran.version == (try VersionTests.workspaceVersion()))
    }
    @Test func typedDeferredAndFailureFollowupsRemainSeparate() {
        let noOperation = CoreBridge.CloudPolicyAdmission(MeguriCoreBindings.CloudPolicyAdmission.noOperation)
        guard case .noOperation = noOperation else { Issue.record("no-operation became queue admission"); return }
        let rejected = CoreBridge.CloudPolicyAdmission(MeguriCoreBindings.CloudPolicyAdmission.admitted(accepted: false))
        guard case .admitted(let accepted) = rejected else { Issue.record("queue rejection became no-operation"); return }
        #expect(!accepted)
        let deferred = CoreBridge.CloudPolicyOutput(MeguriCoreBindings.CloudPolicyOutput.applyDeferred(error: .Cancelled))
        guard case .applyDeferred(let error) = deferred else { Issue.record("deferred lost"); return }
        #expect(error == .cancelled)
        let failed = CoreBridge.CloudPolicyOutput(MeguriCoreBindings.CloudPolicyOutput.coreActionFailed(error: .CloudPolicy(failure: .staleCommand), commands: []))
        guard case .coreActionFailed(let failure, let followups) = failed else { Issue.record("failure collapsed"); return }
        #expect(failure == .cloudPolicy(.staleCommand) && followups.isEmpty)
    }
    @Test func awaitingEvidenceAndUnknownMediaSurviveBindingsMapping() {
        #expect(CoreBridge.CloudPolicyStatusState(.awaitingEvidence) == .awaitingEvidence)
        #expect(CoreBridge.CloudPolicySyncNowAvailability(.unavailable(reason: .reviewActive)) == .unavailable(.reviewActive))
        let raw = MeguriCoreBindings.CloudPolicyMediaFacts(uploadFiles: 7, uploadBytes: nil, downloadFiles: nil,
            downloadBytes: 11, unknownBlobs: nil, missingNames: 13, uploadsRunning: nil, downloadsRunning: nil,
            remainingDownloadMs: nil, rootAcked: nil, zoneMissing: nil, noSpace: nil)
        let media = CoreBridge.CloudPolicyMediaFacts(raw)
        #expect(media.uploadFiles == 7 && media.downloadBytes == 11 && media.missingNames == 13)
        #expect(media.downloadFiles == nil && media.rootAcked == nil && media.remainingDownloadMs == nil)
    }
}
