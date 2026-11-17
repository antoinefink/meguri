import Testing
import MeguriCoreBindings
@testable import CoreBridge

extension EventTests {
    @Test func actualStatusQueryCarriesUnboundControlFactsWithoutMakingUpMetadata() async throws {
        try await withCloudConnection { fixture in
            let policy = try await fixture.workflow.policy()
            try await fixture.beginDiscovery()
            defer { fixture.endDiscovery() }
            _ = try await policy.refreshAccount()
            let status = try await policy.status()
            #expect(status.controls.enable == .connect)
            #expect(status.controls.bindingUsable == false)
            #expect(status.controls.bindingActive == false)
            #expect(status.controls.bindingStopped == false)
            #expect(status.controls.resumeBindingOffered == nil)
            #expect(status.controls.automaticEligible == false)
            #expect(status.asOfMilliseconds == nil && status.lastCompleted == nil)
            #expect(status.media.uploadFiles == nil && status.media.downloadFiles == nil)
            _ = try await policy.input(.settings(.init(enabled: false, paused: true, automatic: true, media: true)))
            let paused = try await policy.status()
            #expect(paused.controls.enable == .resumePause && paused.controls.userPaused == true)
            #expect(paused.controls.bindingUsable == false)
        }
    }
}

@Suite struct CloudPolicyControlsMappingTests {
    @Test func readonlyFactsMapWithoutUsingDisplayStateOrSupplyingResumeAuthority() {
        let raw = MeguriCoreBindings.CloudPolicyControls(enable: .pause, bindingUsable: true, bindingActive: true,
            bindingStopped: false, decisionRequired: false, userPaused: false, resumeBindingOffered: nil,
            automaticEligible: true)
        let value = CoreBridge.CloudPolicyControls(raw)
        #expect(value.enable == .pause)
        #expect(value.bindingUsable == true && value.bindingActive == true)
        #expect(value.bindingStopped == false && value.decisionRequired == false && value.userPaused == false)
        #expect(value.automaticEligible == true && value.resumeBindingOffered == nil)
        #expect(CoreBridge.CloudPolicyEnableControl(.awaitingResumeOffer) == .awaitingResumeOffer)
        #expect(CoreBridge.CloudPolicyEnableControl(.decisionRequired) == .decisionRequired)
        #expect(CoreBridge.CloudPolicyEnableControl(.resumePause) == .resumePause)
        #expect(CoreBridge.CloudPolicyEnableControl(.connect) == .connect)
        #expect(CoreBridge.CloudPolicyControls.unknown.enable == nil)
        #expect(CoreBridge.CloudPolicyControls.unknown.bindingUsable == nil)
        #expect(CoreBridge.CloudPolicyControls.unknown.automaticEligible == nil)
    }
}
