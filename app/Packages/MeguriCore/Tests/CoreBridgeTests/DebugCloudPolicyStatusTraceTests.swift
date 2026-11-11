#if DEBUG
import MeguriCoreBindings
import Testing
@testable import CoreBridge

struct DebugCloudPolicyStatusTraceTests {
    private typealias Trace = DebugCloudDiagnostics.PolicyStatusTrace

    @Test func policyTraceProjectsExactCountersAndEachExchangeOwnTimestamp() {
        let completed = policyRecord(.succeeded(selected: 11, heldBack: 12, newerPending: 13,
            incomingWaiting: 14, media: policyMedia), ended: 101)
        let attempt = policyRecord(.incomplete(outstanding: 21, blocked: 22), ended: 202)
        let status = policyStatus(counts: (1, 2, 3, 4), completed: completed, attempt: attempt)
        #expect(Trace.snapshot(status) == "pending=1 heldBack=2 inFlight=3 incomingWaiting=4 bindingActive=true bindingUsable=false accountMatchesBinding=true lastCompleted={endedMilliseconds=101 outcome=succeeded selected=11 heldBack=12 newerPending=13 incomingWaiting=14} lastAttempt={endedMilliseconds=202 outcome=incomplete outstanding=21 blocked=22}")
    }

    @Test func policyTraceKeepsZeroNilAndUnknownDistinctAndUsesFiniteOutcomeTags() {
        #expect(Trace.snapshot(policyStatus()) == "pending=0 heldBack=0 inFlight=0 incomingWaiting=0 bindingActive=true bindingUsable=false accountMatchesBinding=true lastCompleted=nil lastAttempt=nil")
        #expect(Trace.flag(CoreBridge.CloudPolicyControls.unknown.bindingActive) == "unknown")
        #expect(Trace.flag(CoreBridge.CloudPolicyControls.unknown.bindingUsable) == "unknown")
        #expect(Trace.flag(false) == "false")
        #expect(Trace.flag(true) == "true")
        let cases: [(MeguriCoreBindings.CloudPolicyExchangeOutcome, String)] = [
            (.succeeded(selected: 0, heldBack: 0, newerPending: 0, incomingWaiting: 0, media: policyMedia),
                "succeeded selected=0 heldBack=0 newerPending=0 incomingWaiting=0"),
            (.incomplete(outstanding: 0, blocked: 0), "incomplete outstanding=0 blocked=0"),
            (.waiting(class: .throttled, retryAfterMs: .max), "waiting"),
            (.failed(class: .other), "failed"), (.cancelled, "cancelled")
        ]
        #expect(Trace.exchange(nil) == "nil")
        for (outcome, expected) in cases {
            let record = CoreBridge.CloudPolicyExchangeRecord(policyRecord(outcome, ended: 0))
            #expect(Trace.exchange(record) == "{endedMilliseconds=0 outcome=\(expected)}")
        }
    }

    @Test func policyTraceRetainsInitialAndFirstCompletedAcrossUpdatesAndResetsOnEitherOwnerPart() throws {
        let prior = policyRecord(.incomplete(outstanding: 5, blocked: 6), ended: 101)
        let next = policyRecord(.cancelled, ended: 202)
        let initial = policyStatus(counts: (1, 0, 0, 0), completed: prior)
        let latest = policyStatus(counts: (2, 0, 0, 0), completed: next, attempt: next)
        let trace = Trace()
        #expect(trace.value(generation: 7, hostRevision: 8) == nil)
        trace.retain(initial, generation: 7, hostRevision: 8)
        trace.retain(latest, generation: 7, hostRevision: 8)
        #expect(trace.value(generation: 7, hostRevision: 8) == "policy{latest={\(Trace.snapshot(latest))} initial={\(Trace.snapshot(initial))} firstCompleted={endedMilliseconds=101 outcome=incomplete outstanding=5 blocked=6}}")
        // A later nil cannot erase the retained prior exchange, including after restart's first snapshot.
        let empty = policyStatus()
        trace.retain(empty, generation: 7, hostRevision: 8)
        let retained = try #require(trace.value(generation: 7, hostRevision: 8))
        #expect(retained.contains("firstCompleted={endedMilliseconds=101 outcome=incomplete outstanding=5 blocked=6}"))
        for (generation, revision) in [(UInt64(7), UInt64(9)), (UInt64(8), UInt64(9))] {
            trace.retain(empty, generation: generation, hostRevision: revision)
            #expect(trace.value(generation: generation, hostRevision: revision) == "policy{latest={\(Trace.snapshot(empty))} initial={\(Trace.snapshot(empty))} firstCompleted=nil}")
            trace.retain(latest, generation: generation, hostRevision: revision)
            trace.retain(initial, generation: generation, hostRevision: revision)
            trace.retain(empty, generation: generation, hostRevision: revision)
            #expect(trace.value(generation: generation, hostRevision: revision) == "policy{latest={\(Trace.snapshot(empty))} initial={\(Trace.snapshot(empty))} firstCompleted={endedMilliseconds=202 outcome=cancelled}}")
        }
    }

    @Test func policyTraceSuppressesEitherWrongOwnerPartBeforeAnotherRetainWithoutErasingMatchingEvidence() throws {
        let initial = policyStatus(counts: (1, 0, 0, 0))
        let completed = policyRecord(.cancelled, ended: 202)
        let latest = policyStatus(counts: (2, 0, 0, 0), completed: completed)
        let trace = Trace()
        trace.retain(initial, generation: 7, hostRevision: 8)
        trace.retain(latest, generation: 7, hostRevision: 8)
        let retained = try #require(trace.value(generation: 7, hostRevision: 8))
        #expect(retained == "policy{latest={\(Trace.snapshot(latest))} initial={\(Trace.snapshot(initial))} firstCompleted={endedMilliseconds=202 outcome=cancelled}}")
        // Replacement/reattachment can fail without ever delivering another accepted snapshot.
        for _ in 0..<2 {
            #expect(trace.value(generation: 8, hostRevision: 8) == nil)
            #expect(trace.value(generation: 7, hostRevision: 9) == nil)
            #expect(trace.value(generation: 8, hostRevision: 9) == nil)
            #expect(trace.value(generation: 7, hostRevision: 8) == retained)
        }
    }

    @Test func processFirstAccessibilityValueKeepsRealSizeLatestFrameWithinInspectionLimit() throws {
        let completed = policyRecord(.succeeded(selected: 1842, heldBack: 12, newerPending: 13,
            incomingWaiting: 14, media: policyMedia), ended: 1794398400000)
        let attempt = policyRecord(.incomplete(outstanding: 21, blocked: 22), ended: 1794398460000)
        let trace = Trace()
        trace.retain(policyStatus(), generation: 7, hostRevision: 8)
        trace.retain(policyStatus(counts: (12, 3, 4, 5), completed: completed, attempt: attempt),
            generation: 7, hostRevision: 8)
        let policyValue = try #require(trace.value(generation: 7, hostRevision: 8))
        let failureValue = DebugCloudDiagnostics.Value(.policyExchange,
            error: BridgeError.cloudPolicy(.unqualifiedCompletion)).formatted
        let process = DebugCloudDiagnostics.FetchRecorder()
        process.record(.will, count: 12)
        process.record(.zoneC, count: 11)
        process.record(.modC, count: 1842)
        process.record(.modR, count: 2)
        process.record(.coreC, count: 1840)
        process.record(.refused)
        process.record(.zoneFail, count: 3)
        // Match SyncStatusRows: all-process counts, current-owner policy, failure.
        let values: [String?] = [process.formatted, policyValue, failureValue]
        let accessibilityValue = values.compactMap { $0 }.joined(separator: " ")
        let latestFrame = "policy{latest={pending=12 heldBack=3 inFlight=4 incomingWaiting=5 bindingActive=true bindingUsable=false accountMatchesBinding=true lastCompleted={endedMilliseconds=1794398400000 outcome=succeeded selected=1842 heldBack=12 newerPending=13 incomingWaiting=14} lastAttempt={endedMilliseconds=1794398460000 outcome=incomplete outstanding=21 blocked=22}}"
        #expect(accessibilityValue.count > 512)
        #expect(String(accessibilityValue.prefix(512)).hasPrefix("\(process.formatted) \(latestFrame)"))
        #expect(accessibilityValue.hasSuffix(failureValue))
    }

    @Test func policyTraceStaysBoundedAndDiscardsUnrequestedAssociatedPayloads() throws {
        let outcome = MeguriCoreBindings.CloudPolicyExchangeOutcome.succeeded(selected: .max, heldBack: .max,
            newerPending: .max, incomingWaiting: .max, media: policyMedia)
        let record = policyRecord(outcome, ended: .min)
        let status = policyStatus(counts: (.max, .max, .max, .max), completed: record, attempt: record)
        let trace = Trace()
        trace.retain(status, generation: 777777, hostRevision: 888888)
        let original = try #require(trace.value(generation: 777777, hostRevision: 888888))
        for _ in 0..<1_000 { trace.retain(status, generation: 777777, hostRevision: 888888) }
        let value = try #require(trace.value(generation: 777777, hostRevision: 888888))
        #expect(value == original && value.utf8.count < 3_000)
        for excluded in ["exchange=", "source=", "ankiConnect", "media", "missingNames", "999999", "777777", "888888"] {
            #expect(!value.contains(excluded))
        }
        let waiting = CoreBridge.CloudPolicyExchangeRecord(policyRecord(.waiting(class: .throttled, retryAfterMs: 999999), ended: 0))
        #expect(Trace.exchange(waiting) == "{endedMilliseconds=0 outcome=waiting}")
    }

    private var policyMedia: MeguriCoreBindings.CloudPolicyMediaFacts {
        .init(uploadFiles: nil, uploadBytes: nil, downloadFiles: nil, downloadBytes: nil,
            unknownBlobs: nil, missingNames: 999999, uploadsRunning: nil, downloadsRunning: nil,
            remainingDownloadMs: nil, rootAcked: nil, zoneMissing: nil, noSpace: nil)
    }

    private func policyRecord(_ outcome: MeguriCoreBindings.CloudPolicyExchangeOutcome, ended: Int64) -> MeguriCoreBindings.CloudPolicyExchangeRecord {
        .init(exchange: 999999, endedMs: ended, source: .ankiConnect, outcome: outcome)
    }

    private func policyStatus(counts: (UInt64, UInt64, UInt64, UInt64) = (0, 0, 0, 0),
                              completed: MeguriCoreBindings.CloudPolicyExchangeRecord? = nil,
                              attempt: MeguriCoreBindings.CloudPolicyExchangeRecord? = nil) -> CoreBridge.CloudPolicyStatus {
        .init(MeguriCoreBindings.CloudPolicyStatus(state: .awaitingEvidence,
            pending: counts.0, heldBack: counts.1, inFlight: counts.2, incomingWaiting: counts.3,
            attention: [], media: policyMedia, mediaPaused: false, error: nil, accountStatus: .available,
            accountMatchesBinding: true, asOfMs: nil, lastCompleted: completed, lastAttempt: attempt,
            syncNow: .available, controls: .init(enable: .pause, bindingUsable: false, bindingActive: true,
                bindingStopped: false, decisionRequired: false, userPaused: false,
                resumeBindingOffered: nil, automaticEligible: false)))
    }

}
#endif
