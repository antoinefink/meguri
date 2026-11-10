#if DEBUG && MEGURI_TEST_HOOKS
@testable import CoreBridge
import Foundation
import CryptoKit
import enum MeguriCoreBindings.CloudPolicyExchangeOutcome
import enum MeguriCoreBindings.CloudPolicyOperationError
import enum MeguriCoreBindings.CloudPolicySource
import struct MeguriCoreBindings.CloudPolicyExchangeRecord
import Testing

// Generated observations exercise ledger admission and refusal only. They are never actual
// CloudKit success records, and this suite belongs outside the smoke selection.
@MainActor @Suite(.serialized) struct CloudKitSmokeObservationTests {
    private func entity(_ label: String, revision: UInt64 = 7, acked: UInt64 = 7,
                        digest: UInt8 = 11, acknowledged: Bool = true,
                        blob: Bool = true, bytes: UInt64 = 10_240, ackEnvelopeByte: UInt8 = 1) -> SmokeCloudEntity {
        .init(label: label, recordName: "generated-record-" + label, revision: revision,
              ackedRevision: acked, digest: Data(repeating: digest, count: 32),
              ackedEnvelopeDigest: Data(SHA256.hash(data: Data([ackEnvelopeByte]))), acknowledged: acknowledged,
              mediaBytes: label == "m1" ? bytes : nil,
              mediaSHA256: label == "m1" ? Data(repeating: 13, count: 32) : nil,
              blobPresent: blob, mediaChangeCounter: label == "m1" ? 5 : nil)
    }
    @Test func acknowledgementNeedsEachExactCurrentRevisionActualRecordEventAndBlobProof() throws {
        let labels = ["n1", "c1", "r1", "m1"]
        let current = labels.map { entity($0) }
        let retained = Dictionary(uniqueKeysWithValues: current.map { ($0.label, $0) })
        let events = current.map { CloudWorkflowObservation.savedPendingRemoved(name: $0.recordName, requestID: 17) }
        let valid = try CloudKitSmokeProof.acknowledgements(current: current, retained: retained, observations: events)
        #expect(Set(valid.compactMap(\.operation)) == Set(labels))
        #expect(valid.allSatisfy { $0.name == .stateSaved && $0.count == 7 })
        // A count or matching content cannot ACK an older/newer revision. Also
        // reject a note-only save and a collection success with pending media.
        for invalid in [
            [entity("n1", acked: 6)] + Array(current.dropFirst()),
            [entity("n1", revision: 8, acked: 8)] + Array(current.dropFirst()),
            [entity("n1", digest: 12)] + Array(current.dropFirst()),
            [entity("n1", acknowledged: false)] + Array(current.dropFirst()),
            Array(current.dropLast()) + [entity("m1", blob: false)],
            Array(current.dropLast()) + [entity("m1", bytes: 10_239)],
            Array(current.dropLast()) + [entity("n1")]
        ] {
            #expect(throws: AgentFailure.execution) {
                try CloudKitSmokeProof.acknowledgements(current: invalid, retained: retained, observations: events)
            }
        }
        for incomplete in [Array(events.prefix(1)), Array(events.dropLast()), []] {
            #expect(throws: AgentFailure.execution) {
                try CloudKitSmokeProof.acknowledgements(current: current, retained: retained, observations: incomplete)
            }
        }
    }
    @Test func generatedMediaUsesAll320RunBoundSHA256Blocks() {
        let bytes = CloudKitSmokeGeneratedInput.media(run: "generated-run", device: "generated-mac")
        #expect(bytes.count == 10_240)
        #expect(bytes == CloudKitSmokeGeneratedInput.media(run: "generated-run", device: "generated-mac"))
        #expect(bytes != CloudKitSmokeGeneratedInput.media(run: "other-run", device: "generated-mac"))
        #expect(bytes != CloudKitSmokeGeneratedInput.media(run: "generated-run", device: "other-mac"))
        #expect(Set(stride(from: 0, to: bytes.count, by: 32).map { bytes.subdata(in: $0..<($0 + 32)) }).count == 320)
    }

    private func current(engine: String? = "generated-engine", generation: UInt64 = 11,
                         publicationID: Data? = nil, publicationComplete: Bool = true,
                         bindingUsable: Bool = true) -> SmokeCloudObservation {
        .init(generation: generation, activeSession: nil, seq: 20, publicationID: publicationID, publicationReason: nil,
              publicationCompleting: false, publicationBindingID: nil, bindingID: nil,
              publicationInitial: false, publicationComplete: publicationComplete, bindingUsable: bindingUsable,
              engine: engine, entities: [], cardNoteID: nil, lastReviewID: nil)
    }
    private func ledger(selected: Bool = false) throws -> CloudKitSmokeEvidence {
        let evidence = CloudKitSmokeEvidence()
        try evidence.setGeneration(11, selectedPublication: selected)
        return evidence
    }
    private func stateCallback(_ evidence: CloudKitSmokeEvidence, ordinal: UInt64 = 1) {
        for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
            evidence.sdk(.init(origin: "generated-engine", ordinal: ordinal,
                               event: .stateUpdate(Data([9])), phase: phase), current: current())
        }
    }
    @Test func callbackJournalWritesEachPhaseBeforeReturnAndKeepsItsCommand() throws {
        let evidence = try ledger()
        let journal = CloudKitSmokeJournal(evidence: evidence)
        var commands: [Int] = []
        var records: [AgentFact] = []
        func installWriter(for command: Int) {
            journal.install { { facts in
                commands.append(contentsOf: repeatElement(command, count: facts.count))
                records.append(contentsOf: facts)
            } }
        }
        installWriter(for: 1)
        let event = CloudEngineEvent.stateUpdate(Data([9]))
        journal.record(.init(origin: "generated-engine", ordinal: 1, event: event, phase: .delivered))
        #expect(records.map(\.operation) == ["sdk1-state-delivered"])
        installWriter(for: 2)
        journal.record(.init(origin: "generated-engine", ordinal: 1, event: event, phase: .committed))
        #expect(records.last?.name == .stateSaved && commands == [1, 1])
        journal.record(.init(origin: "generated-engine", ordinal: 1, event: event, phase: .returned))
        #expect(records.last?.operation == "sdk1-state-returned" && commands == [1, 1, 1])
        let batch = CloudEngineBatch(id: 7, saves: [.init(name: "generated-blob", envelope: Data([4]), systemFields: nil)], deletes: [])
        journal.recordBatch(origin: "generated-engine", batch: batch, proof: nil, current: current())
        #expect(records.suffix(2).map(\.operation) == ["batch1-media-handoff", "batch1-save-count"])
        #expect(commands == [1, 1, 1, 2, 2])
        journal.record(.init(origin: "generated-engine", ordinal: 2, event: event, phase: .delivered))
        #expect(commands == [1, 1, 1, 2, 2, 2])
        #expect(evidence.drain().isEmpty)
        try evidence.check()
    }
    @Test func terminalExchangePreservesUnsuccessfulAccountingFromTheOriginalResult() throws {
        let incomplete = CoreBridge.CloudPolicyExchangeRecord(.init(
            exchange: 7, endedMs: 30, source: .command, outcome: .incomplete(outstanding: 3, blocked: 1)))
        let facts = try CloudKitSelectedSyncProof.terminalExchangeFacts(incomplete)
        #expect(facts.map(\.operation) == ["terminal-exchange-incomplete", "terminal-ended-ms",
                                           "terminal-outstanding", "terminal-blocked"])
        #expect(facts.map(\.count) == [7, 30, 3, 1])
        #expect(throws: AgentFailure.execution) { try CloudKitSelectedSyncProof.exchangeFacts(incomplete) }

        let waiting = CoreBridge.CloudPolicyExchangeRecord(.init(
            exchange: 7, endedMs: 30, source: .command, outcome: .waiting(class: .throttled, retryAfterMs: 50)))
        let retry = try CloudKitSelectedSyncProof.terminalExchangeFacts(waiting)
        #expect(retry.map(\.operation) == ["terminal-exchange-waiting", "terminal-ended-ms",
                                          "terminal-error-throttled", "terminal-retry-ms"])
        #expect(retry.map(\.count) == [7, 30, nil, 50])
    }
    @Test func originalOwnAttemptRequiresPrepareEnqueueReplyThenCommit() throws {
        let identity = UUID()
        let metadata = CloudOwnMetadata(operationID: Data([1]), attempt: 4, kind: .createZones)
        let phases: [CloudOwnObservationPhase] = [.prepared, .enqueued(true), .replied(.zonesCreated), .committed]
        let valid = try ledger()
        for phase in phases { valid.own(.init(metadata: metadata, identity: identity, phase: phase)) }
        try valid.check()
        let facts = valid.drain()
        #expect(facts.map(\.count) == [1, 2, 3, 4])
        #expect(facts.map(\.name) == [.applied, .applied, .applied, .stateSaved])
        #expect(facts.map(\.operation) == ["own1-createZones-prepared", "own1-createZones-enqueued", "own1-createZones-replied", "own1-createZones-committed"])
        let invalidPhases: [[CloudOwnObservationPhase]] = [
            [.prepared, .committed], [.prepared, .enqueued(false)],
            [.prepared, .prepared], [.enqueued(true)],
            [.prepared, .enqueued(true), .replied(.notStarted)]
        ]
        for invalid in invalidPhases {
            let evidence = try ledger()
            for phase in invalid { evidence.own(.init(metadata: metadata, identity: identity, phase: phase)) }
            #expect(throws: AgentFailure.execution) { try evidence.check() }
        }
        let wrongOwner = try ledger()
        wrongOwner.own(.init(metadata: metadata, identity: identity, phase: .prepared))
        wrongOwner.own(.init(metadata: metadata, identity: UUID(), phase: .enqueued(true)))
        #expect(throws: AgentFailure.execution) { try wrongOwner.check() }
        let wrongAttempt = try ledger()
        wrongAttempt.own(.init(metadata: metadata, identity: identity, phase: .prepared))
        wrongAttempt.own(.init(metadata: .init(operationID: Data([1]), attempt: 5, kind: .createZones), identity: identity, phase: .enqueued(true)))
        #expect(throws: AgentFailure.execution) { try wrongAttempt.check() }
    }
    // Opaque generated transport payloads; Core's real call, never this ledger,
    // decodes and accepts an MGMediaRoot before emitting the original commit.
    private var controlRecord: CloudControlRecord {
        .init(encrypted: Data([31, 32]), systemFields: Data([41, 42]))
    }
    private func publication(_ evidence: CloudKitSmokeEvidence, mediaResult: CloudPublicationResult,
                             mediaCommit: Bool = true) {
        let operations: [(CloudOwnKind, CloudPublicationResult)] = [
            (.createZones, .zonesCreated), (.saveMediaRoot, mediaResult),
            (.savePublishingEpoch, .saved(controlRecord)), (.saveCompleteEpoch, .saved(controlRecord)),
            (.saveActive, .saved(controlRecord))
        ]
        for (index, operation) in operations.enumerated() {
            let identity = UUID()
            let metadata = CloudOwnMetadata(operationID: Data([1]), attempt: 4, kind: operation.0)
            let phases: [CloudOwnObservationPhase] = [.prepared, .enqueued(true), .replied(operation.1)]
            for phase in phases { evidence.own(.init(metadata: metadata, identity: identity, phase: phase)) }
            if index != 1 || mediaCommit {
                evidence.own(.init(metadata: metadata, identity: identity, phase: .committed))
            }
        }
    }
    @Test func mediaRootTypedConflictUsesOriginalCommitAndSameFixedFactsAsSaved() throws {
        for result in [CloudPublicationResult.saved(controlRecord), .conflict(controlRecord)] {
            let evidence = try ledger()
            publication(evidence, mediaResult: result)
            try evidence.check()
            try evidence.requirePublication(Data([1]), completed: current())
            let facts = evidence.drain()
            #expect(facts.count == 20)
            #expect(facts.map(\.count) == (1...20).map { UInt64($0) })
            let media = Array(facts[4..<8])
            #expect(media.map(\.operation) == ["own2-saveMediaRoot-prepared", "own2-saveMediaRoot-enqueued", "own2-saveMediaRoot-replied", "own2-saveMediaRoot-committed"])
            #expect(media.map(\.name) == [.applied, .applied, .applied, .stateSaved])
            #expect(facts.allSatisfy { $0.digest == nil && $0.engine == nil })
        }
    }
    @Test func mediaRootConflictDoesNotAdmitOtherOperationKindsOrOtherFailureResults() throws {
        let kinds: [CloudOwnKind] = [.createZones, .savePublishingEpoch, .saveCompleteEpoch,
                                    .saveActive, .readOwnEpoch, .readResetMediaRoot]
        for kind in kinds {
            let evidence = try ledger()
            let identity = UUID()
            let metadata = CloudOwnMetadata(operationID: Data([1]), attempt: 4, kind: kind)
            for phase in [CloudOwnObservationPhase.prepared, .enqueued(true), .replied(.conflict(controlRecord))] {
                evidence.own(.init(metadata: metadata, identity: identity, phase: phase))
            }
            #expect(throws: AgentFailure.execution) { try evidence.check() }
        }
        let invalid: [CloudPublicationResult] = [.zonesCreated, .read(.found(controlRecord)), .notStarted,
                                               .failed(retryAfterMilliseconds: nil), .retryAfter(durationMilliseconds: 100)]
        for result in invalid {
            let evidence = try ledger()
            publication(evidence, mediaResult: result)
            #expect(throws: AgentFailure.execution) { try evidence.check() }
            #expect(throws: AgentFailure.execution) { try evidence.requirePublication(Data([1]), completed: current()) }
        }
    }
    @Test func mediaRootConflictCannotReplaceMissingOriginalCoreCommitOrPublicationCompletion() throws {
        let missingCommit = try ledger()
        publication(missingCommit, mediaResult: .conflict(controlRecord), mediaCommit: false)
        try missingCommit.check() // A valid reply can still await its Core commit.
        #expect(throws: AgentFailure.execution) { try missingCommit.requirePublication(Data([1]), completed: current()) }
        #expect(!missingCommit.drain().contains { $0.operation == "own2-saveMediaRoot-committed" })

        let completed = try ledger()
        publication(completed, mediaResult: .conflict(controlRecord))
        for incomplete in [current(publicationComplete: false), current(bindingUsable: false),
                           current(engine: nil), current(publicationID: Data([1])), current(generation: 12)] {
            #expect(throws: AgentFailure.execution) { try completed.requirePublication(Data([1]), completed: incomplete) }
        }
        #expect(throws: AgentFailure.execution) { try completed.requirePublication(Data([2]), completed: current()) }
        try completed.requirePublication(Data([1]), completed: current())
    }
    @Test func mediaRootConflictRetainsOriginalOwnerAttemptAndPhaseOrder() throws {
        let metadata = CloudOwnMetadata(operationID: Data([1]), attempt: 4, kind: .saveMediaRoot)
        for mismatch in 0..<4 {
            let evidence = try ledger()
            let identity = UUID()
            for phase in [CloudOwnObservationPhase.prepared, .enqueued(true), .replied(.conflict(controlRecord))] {
                evidence.own(.init(metadata: metadata, identity: identity, phase: phase))
            }
            let commitMetadata = CloudOwnMetadata(operationID: Data([mismatch == 2 ? 2 : 1]),
                                                   attempt: mismatch == 1 ? 5 : 4, kind: .saveMediaRoot)
            evidence.own(.init(metadata: commitMetadata, identity: mismatch == 0 ? UUID() : identity,
                               phase: mismatch == 3 ? .replied(.conflict(controlRecord)) : .committed))
            #expect(throws: AgentFailure.execution) { try evidence.check() }
            #expect(!evidence.drain().contains { $0.operation == "own1-saveMediaRoot-committed" })
        }
    }
    @Test func selectedControlConflictsRequireTypedOriginAttemptKindAndOriginalCommit() throws {
        for target in [CloudOwnKind.saveCompleteEpoch, .saveActive] {
            let eventKind: SelectedCloudPublicationEvent.Kind = target == .saveCompleteEpoch ? .completeEpoch : .doneActive
            for variant in 0..<6 {
                let evidence = CloudKitSmokeEvidence()
                try evidence.setGeneration(11, selectedPublication: true)
                let kinds: [CloudOwnKind] = [.createZones, .saveMediaRoot, .savePublishingEpoch, .saveCompleteEpoch, .saveActive]
                for kind in kinds {
                    let identity = UUID()
                    let metadata = CloudOwnMetadata(operationID: Data([1]), attempt: 4, kind: kind)
                    let event = kind == target && variant != 5 ? SelectedCloudPublicationEvent(
                        origin: variant == 2 ? .saved : .conflict,
                        attempt: variant == 3 ? 5 : 4,
                        kind: variant == 4 ? (eventKind == .completeEpoch ? .doneActive : .completeEpoch) : eventKind,
                        correlation: Data(repeating: 41, count: 32)) : nil
                    let reply: CloudPublicationResult = kind == target ? .conflict(controlRecord)
                        : (kind == .createZones ? .zonesCreated : .saved(controlRecord))
                    for phase in [CloudOwnObservationPhase.prepared, .enqueued(true), .replied(reply)] {
                        evidence.own(.init(metadata: metadata, identity: identity, phase: phase,
                            publicationEvent: { if case .replied = phase { return event }; return nil }()))
                    }
                    if kind != target || variant != 1 {
                        evidence.own(.init(metadata: metadata, identity: identity, phase: .committed, publicationEvent: event))
                    }
                }
                switch variant {
                case 0:
                    try evidence.check()
                    // Only the phase join is proven here. Runtime still must
                    // correlate both events with actual final Core controls.
                    try evidence.requirePublication(Data([1]), completed: current())
                case 1:
                    try evidence.check()
                    #expect(throws: AgentFailure.execution) { try evidence.requirePublication(Data([1]), completed: current()) }
                    #expect(!evidence.drain().contains { $0.operation?.contains(target == .saveCompleteEpoch ? "saveCompleteEpoch-committed" : "saveActive-committed") == true })
                default:
                    #expect(throws: AgentFailure.execution) { try evidence.check() }
                }
            }
        }
    }

    @Test func sdkStateSaveRequiresOriginalDeliveryCommitAndReturnWithoutOpaqueFacts() throws {
        let valid = try ledger()
        stateCallback(valid)
        try valid.check()
        let facts = valid.drain()
        #expect(facts.map(\.count) == [1, 2, 3])
        #expect(facts.map(\.name) == [.engineCallback, .stateSaved, .engineCallback])
        #expect(facts.allSatisfy { $0.engine == "engine1" && $0.digest == nil })
        let invalidPhases: [[CloudSDKObservationPhase]] = [
            [.committed], [.delivered, .returned], [.delivered, .delivered],
            [.delivered, .committed, .committed]
        ]
        for phases in invalidPhases {
            let invalid = try ledger()
            for phase in phases {
                invalid.sdk(.init(origin: "generated-engine", ordinal: 1, event: .stateUpdate(Data([9])), phase: phase), current: current())
            }
            #expect(throws: AgentFailure.execution) { try invalid.check() }
        }
        for wrong in [current(engine: "replacement"), current(generation: 12)] {
            let invalid = try ledger()
            invalid.sdk(.init(origin: "generated-engine", ordinal: 1, event: .stateUpdate(Data([9])), phase: .delivered), current: wrong)
            #expect(throws: AgentFailure.execution) { try invalid.check() }
        }
    }
    @Test func commonOriginalJoinUsesCompletedPublicationBatchAndStateWithoutFictionalReview() throws {
        let evidence = try ledger(selected: true)
        publication(evidence, mediaResult: .saved(controlRecord))
        let record = CloudRecord(name: entity("n1").recordName, envelope: Data([1]), systemFields: Data([2]))
        evidence.batch(origin: "generated-engine", batch: .init(id: 9, saves: [record], deletes: []), proof: nil, current: current())
        let sent = CloudEngineEvent.sent(batchID: 9, saved: [record], failed: [], deleted: [])
        for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
            evidence.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: phase), current: current())
        }
        // The earlier source ACK and the common join both need the original
        // saved envelope and a later committed AND returned state callback.
        #expect(throws: AgentFailure.execution) { try evidence.joinedOriginalWork() }
        stateCallback(evidence, ordinal: 2)
        _ = try evidence.requireSelectedAcknowledgement(entity("n1"),
            observations: [.savedPendingRemoved(name: record.name, requestID: 9)],
            attemptBoundary: evidence.acknowledgementBoundary())
        #expect(throws: AgentFailure.execution) { try evidence.joined() } // Smoke still needs held evidence.
        let joined = try evidence.joinedOriginalWork()
        #expect(joined.map(\.operation) == ["own-joined", "sdk-joined"])
        #expect(joined.map(\.count) == [5, 2])
        #expect(!joined.contains { $0.operation?.contains("held") == true })
    }
    @Test func replacementEngineKeepsOriginalBatchAndStateSeparate() throws {
        let evidence = try ledger()
        let original = "generated-engine"
        let replacement = "replacement-engine"
        let record = CloudRecord(name: entity("n1").recordName, envelope: Data([1]), systemFields: Data([2]))
        try evidence.admitSmokeSDK(origin: original)
        evidence.batch(origin: original, batch: .init(id: 9, saves: [record], deletes: []), proof: nil, current: current())
        let sent = CloudEngineEvent.sent(batchID: 9, saved: [record], failed: [], deleted: [])
        for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
            evidence.sdk(.init(origin: original, ordinal: 1, event: sent, phase: phase), current: current())
        }

        try evidence.admitSmokeSDK(origin: replacement)
        let replaced = current(engine: replacement)
        evidence.batch(origin: replacement, batch: .init(id: 9, saves: [], deletes: []), proof: nil, current: replaced)
        for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
            evidence.sdk(.init(origin: replacement, ordinal: 1, event: .stateUpdate(Data([3])), phase: phase), current: replaced)
        }
        try evidence.check()
        let observations: [CloudWorkflowObservation] = [.savedPendingRemoved(name: record.name, requestID: 9)]
        #expect(throws: AgentFailure.execution) {
            try evidence.requireAcknowledgements([entity("n1")], observations: observations)
        }
        #expect(throws: AgentFailure.execution) { try evidence.joinedOriginalWork() }

        stateCallback(evidence, ordinal: 2)
        let ack = try evidence.requireAcknowledgements([entity("n1")], observations: observations)
        #expect(ack.first?.operation == "n1-sdk1-batch1-ack" && ack.first?.engine == "engine1")
        let facts = evidence.drain()
        #expect(facts.first { $0.operation == "batch2-media-handoff" }?.engine == "engine2")
        let joined = try evidence.joinedOriginalWork()
        #expect(joined.last?.operation == "sdk-joined" && joined.last?.count == 3 && joined.last?.engine == nil)
    }
    @Test func commonOriginalJoinWaitsForOriginalOwnCommitAndSDKReturn() throws {
        let evidence = try ledger(selected: true)
        let identity = UUID()
        let metadata = CloudOwnMetadata(operationID: Data([1]), attempt: 4, kind: .createZones)
        for phase in [CloudOwnObservationPhase.prepared, .enqueued(true), .replied(.zonesCreated)] {
            evidence.own(.init(metadata: metadata, identity: identity, phase: phase))
        }
        let event = CloudEngineEvent.stateUpdate(Data([9]))
        for phase in [CloudSDKObservationPhase.delivered, .committed] {
            evidence.sdk(.init(origin: "generated-engine", ordinal: 1, event: event, phase: phase), current: current())
        }
        #expect(throws: AgentFailure.execution) { try evidence.joinedOriginalWork() }
        evidence.own(.init(metadata: metadata, identity: identity, phase: .committed))
        #expect(throws: AgentFailure.execution) { try evidence.joinedOriginalWork() }
        evidence.sdk(.init(origin: "generated-engine", ordinal: 1, event: event, phase: .returned), current: current())
        #expect(try evidence.joinedOriginalWork().map(\.count) == [1, 1])
    }
    @Test func commonOriginalPeerJoinRetainsOriginalFetchWithoutHeldReview() throws {
        let evidence = try ledger(selected: true)
        let record = CloudRecord(name: entity("n1").recordName, envelope: Data([1]), systemFields: Data([2]))
        let events: [CloudEngineEvent] = [.willFetch(groupID: "generated-fetch", reason: .manual),
            .willFetchZone(zone: "generated-zone"), .fetched(zone: "generated-zone", records: [record], deleted: []),
            .didFetchZone(zone: "generated-zone", error: nil), .didFetch(groupID: "generated-fetch", reason: .manual)]
        for (i, event) in events.enumerated() {
            for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
                evidence.sdk(.init(origin: "generated-engine", ordinal: UInt64(i + 1), event: event, phase: phase), current: current())
            }
        }
        let fetched = try evidence.requireSelectedFetch(entity("n1"))
        #expect(fetched.first?.operation == "n1-original-fetched-returned")
        // This callback proof cannot replace runtime's earlier durable peer
        // observation. The common join proves only the retained original work.
        #expect(!fetched.contains { $0.name == .localDurable })
        #expect(throws: AgentFailure.execution) { try evidence.joined() }
        #expect(try evidence.joinedOriginalWork().map(\.count) == [0, 5])
    }
    @Test func commonOriginalJoinCannotUseEmptyMapsCurrentStateOrRefusedOwnWork() throws {
        let empty = try ledger(selected: true)
        empty.validate(origin: "generated-engine", current: current())
        #expect(throws: AgentFailure.execution) { try empty.joinedOriginalWork() }
        let refused = try ledger(selected: true)
        stateCallback(refused)
        let identity = UUID()
        let metadata = CloudOwnMetadata(operationID: Data([1]), attempt: 4, kind: .createZones)
        refused.own(.init(metadata: metadata, identity: identity, phase: .prepared))
        refused.own(.init(metadata: metadata, identity: identity, phase: .enqueued(false)))
        #expect(throws: AgentFailure.execution) { try refused.joinedOriginalWork() }
        let failed = try ledger(selected: true)
        stateCallback(failed)
        failed.fail()
        #expect(throws: AgentFailure.execution) { try failed.joinedOriginalWork() }
    }
    @Test func commonOriginalJoinRejectsWrongCallbackIdentityOrderAndChangedPayload() throws {
        for variant in 0..<7 {
            let evidence = try ledger(selected: true)
            let record = CloudRecord(name: "generated-record-n1", envelope: Data([1]), systemFields: nil)
            evidence.batch(origin: "generated-engine", batch: .init(id: 9, saves: [record], deletes: []), proof: nil, current: current())
            let sent = CloudEngineEvent.sent(batchID: 9, saved: [record], failed: [], deleted: [])
            evidence.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: .delivered), current: current())
            evidence.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: .committed), current: current())
            let changed = CloudRecord(name: record.name, envelope: Data([2]), systemFields: nil)
            let returned: CloudEngineEvent = variant == 3 ? .sent(batchID: 9, saved: [changed], failed: [], deleted: [])
                : variant == 4 ? .sent(batchID: 10, saved: [record], failed: [], deleted: [])
                : variant == 5 ? .sent(batchID: 9, saved: [], failed: [], deleted: []) : sent
            evidence.sdk(.init(origin: variant == 0 ? "other-engine" : "generated-engine",
                ordinal: variant == 1 ? 2 : 1, event: returned,
                phase: variant == 6 ? .committed : .returned), current: current(generation: variant == 2 ? 12 : 11))
            stateCallback(evidence, ordinal: 3)
            #expect(throws: AgentFailure.execution) { try evidence.joinedOriginalWork() }
        }
        let alteredState = try ledger(selected: true)
        alteredState.sdk(.init(origin: "generated-engine", ordinal: 1, event: .stateUpdate(Data([9])), phase: .delivered), current: current())
        alteredState.sdk(.init(origin: "generated-engine", ordinal: 1, event: .stateUpdate(Data([10])), phase: .committed), current: current())
        #expect(throws: AgentFailure.execution) { try alteredState.joinedOriginalWork() }
    }
    @Test func commonOriginalJoinRefusesLateCallbacksAndOwnWorkAfterTerminalProof() throws {
        for lateOwn in [false, true] {
            let evidence = try ledger(selected: true)
            publication(evidence, mediaResult: .saved(controlRecord))
            stateCallback(evidence)
            _ = try evidence.joinedOriginalWork()
            if lateOwn {
                evidence.own(.init(metadata: .init(operationID: Data([2]), attempt: 5, kind: .createZones),
                    identity: UUID(), phase: .prepared))
            } else { stateCallback(evidence, ordinal: 2) }
            #expect(throws: AgentFailure.execution) { try evidence.check() }
            #expect(throws: AgentFailure.execution) { try evidence.joinedOriginalWork() }
        }
    }
    private func heldProjection(cardDigest: UInt8 = 11, reviewDigest: Data? = nil,
                                session: UInt64 = 99, generation: UInt64 = 11) -> SmokeBatchProof {
        .init(generation: generation, seq: 20, session: session, saves: 0, images: [
            .init(name: "generated-record-c1", envelopeDigest: nil, digest: Data(repeating: cardDigest, count: 32), imageSeq: 19, held: true),
            .init(name: "generated-record-r1", envelopeDigest: nil, digest: reviewDigest, imageSeq: 19, held: true)
        ])
    }
    private func retainHeld(_ evidence: CloudKitSmokeEvidence, _ projection: SmokeBatchProof) throws -> [AgentFact] {
        try evidence.held(projection, session: 99, answerSeq: 20,
                          card: entity("c1", revision: 8, digest: 12), review: entity("r1"), baseline: entity("c1"))
    }
    @Test func zeroActualBatchesStillNeedsSameHeldProjectionAndCannotClaimAnUpload() throws {
        let valid = try ledger()
        let facts = try retainHeld(valid, heldProjection())
        #expect(facts.map(\.operation) == ["c1-held-projection", "r1-held-absent"])
        stateCallback(valid)
        let joined = try valid.joined()
        #expect(joined.first { $0.operation == "held-collection-batches" }?.count == 0)
        #expect(joined.first { $0.operation == "c1-held-handed" }?.count == 0)
        for invalid in [heldProjection(cardDigest: 12), heldProjection(reviewDigest: Data(repeating: 11, count: 32)),
                        heldProjection(session: 100), heldProjection(generation: 12)] {
            #expect(throws: AgentFailure.execution) { try retainHeld(ledger(), invalid) }
        }
        #expect(throws: AgentFailure.execution) { try retainHeld(valid, heldProjection()) }
    }
    @Test func heldHandoffRejectsPrematureAnswerImageAndDuplicateOrAlteredBatch() throws {
        let record = CloudRecord(name: "generated-record-c1", envelope: Data([1]), systemFields: nil)
        let envelopeDigest = Data(SHA256.hash(data: record.envelope))
        for digest in [UInt8(11), UInt8(12)] {
            let evidence = try ledger()
            _ = try retainHeld(evidence, heldProjection())
            let proof = SmokeBatchProof(generation: 11, seq: 20, session: 99, saves: 1, images: [
                .init(name: record.name, envelopeDigest: envelopeDigest, digest: Data(repeating: digest, count: 32), imageSeq: 19, held: true)
            ])
            evidence.batch(origin: "generated-engine", batch: .init(id: 5, saves: [record], deletes: []), proof: proof, current: current())
            stateCallback(evidence)
            if digest == 11 {
                #expect(try evidence.joined().first { $0.operation == "c1-held-handed" }?.count == 1)
                evidence.batch(origin: "generated-engine", batch: .init(id: 5, saves: [record], deletes: []), proof: proof, current: current())
                #expect(throws: AgentFailure.execution) { try evidence.check() }
            } else {
                #expect(throws: AgentFailure.execution) { try evidence.joined() }
            }
        }
        let invalidEnvelope = try ledger()
        let proof = SmokeBatchProof(generation: 11, seq: 20, session: 99, saves: 1, images: [
            .init(name: record.name, envelopeDigest: Data(repeating: 0, count: 32), digest: Data(repeating: 11, count: 32), imageSeq: 19, held: true)
        ])
        invalidEnvelope.batch(origin: "generated-engine", batch: .init(id: 5, saves: [record], deletes: []), proof: proof, current: current())
        #expect(throws: AgentFailure.execution) { try invalidEnvelope.check() }
    }
    @Test func acknowledgementJoinsOriginalBatchSentReturnRemovalAndLaterStateCommit() throws {
        let record = CloudRecord(name: "generated-record-n1", envelope: Data([1]), systemFields: Data([2]))
        let evidence = try ledger()
        evidence.batch(origin: "generated-engine", batch: .init(id: 9, saves: [record], deletes: []), proof: nil, current: current())
        let sent = CloudEngineEvent.sent(batchID: 9, saved: [record], failed: [], deleted: [])
        for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
            evidence.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: phase), current: current())
        }
        let removed = [CloudWorkflowObservation.savedPendingRemoved(name: record.name, requestID: 9)]
        #expect(throws: AgentFailure.execution) {
            try evidence.requireAcknowledgements([entity("n1")], observations: removed)
        }
        stateCallback(evidence, ordinal: 2)
        let facts = try evidence.requireAcknowledgements([entity("n1")], observations: removed)
        #expect(facts.first?.operation == "n1-sdk1-batch1-ack")
        #expect(throws: AgentFailure.execution) {
            try evidence.requireAcknowledgements([entity("n1", ackEnvelopeByte: 2)], observations: removed)
        }
        for wrong in [[], [.savedPendingRemoved(name: record.name, requestID: 8)]] as [[CloudWorkflowObservation]] {
            #expect(throws: AgentFailure.execution) {
                try evidence.requireAcknowledgements([entity("n1")], observations: wrong)
            }
        }
        let unowned = try ledger()
        unowned.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: .delivered), current: current())
        unowned.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: .committed), current: current())
        #expect(throws: AgentFailure.execution) { try unowned.check() }
    }
    @Test func selectedAckKeepsOriginalAttributionAndRejectsHeldFailedAndDifferentEnvelope() throws {
        for variant in 0..<6 {
            let evidence = try ledger()
            let record = CloudRecord(name: "generated-record-n1", envelope: Data([1]), systemFields: Data([2]))
            let boundaryBefore = try evidence.acknowledgementBoundary()
            evidence.batch(origin: "generated-engine", batch: .init(id: 9, saves: [record], deletes: []), proof: nil, current: current())
            let sent = CloudEngineEvent.sent(batchID: 9, saved: [record], failed: [], deleted: [])
            for phase in variant == 2 ? [CloudSDKObservationPhase.delivered, .committed] : [.delivered, .committed, .returned] {
                evidence.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: phase), current: current())
            }
            if variant != 2 && variant != 3 { stateCallback(evidence, ordinal: 2) }
            let boundary = variant == 1 ? boundaryBefore : try evidence.acknowledgementBoundary()
            let removed: [CloudWorkflowObservation] = variant == 4 ? [] : [.savedPendingRemoved(name: record.name, requestID: 9)]
            if variant < 2 {
                let facts = try evidence.requireSelectedAcknowledgement(entity("n1"), observations: removed, attemptBoundary: boundary)
                #expect(facts.last?.operation == (variant == 0 ? "n1-earlier-original-ack" : "n1-original-ack-after-attempt-boundary-timing-unresolved"))
                #expect(!facts.contains { $0.operation?.contains("manual-exchange-ack") == true })
            } else {
                #expect(throws: AgentFailure.execution) {
                    try evidence.requireSelectedAcknowledgement(entity("n1", ackEnvelopeByte: variant == 5 ? 2 : 1), observations: removed, attemptBoundary: boundary)
                }
            }
        }
    }
    @Test func selectedFetchedNeedsEligibleOriginalCallbacksCommitReturnAndExactEnvelope() throws {
        for variant in 0..<8 {
            let evidence = CloudKitSmokeEvidence()
            try evidence.setGeneration(11, selectedPublication: true)
            let record = CloudRecord(name: "generated-record-n1", envelope: Data([1]), systemFields: Data([2]))
            var events: [CloudEngineEvent] = [.willFetch(groupID: "generated-fetch", reason: .manual),
                .willFetchZone(zone: "generated-zone"), .fetched(zone: "generated-zone", records: [record], deleted: []),
                .didFetchZone(zone: "generated-zone", error: nil), .didFetch(groupID: "generated-fetch", reason: .manual)]
            if variant == 1 { events.removeFirst() }
            if variant == 2 { events.removeLast() }
            if variant == 3 { events[4] = .didFetch(groupID: "other-fetch", reason: .manual) }
            if variant == 4 { events[3] = .didFetchZone(zone: "other-zone", error: nil) }
            if variant == 5 { events[3] = .didFetchZone(zone: "generated-zone", error: .networkFailure) }
            if variant == 6 { events = Array(events.prefix(3)) }
            for (i, event) in events.enumerated() {
                let phases: [CloudSDKObservationPhase] = variant == 6 && i == 2 ? [.delivered, .committed] : [.delivered, .committed, .returned]
                for phase in phases { evidence.sdk(.init(origin: "generated-engine", ordinal: UInt64(i + 1), event: event, phase: phase), current: current()) }
            }
            if variant == 0 {
                let facts = try evidence.requireSelectedFetch(entity("n1"))
                #expect(facts.map(\.operation) == ["n1-original-fetched-returned"])
                #expect(!facts.contains { $0.name == .localDurable }) // Callback is only one side of the apply join.
            } else {
                #expect(throws: (any Error).self) {
                    try evidence.requireSelectedFetch(entity("n1", ackEnvelopeByte: variant == 7 ? 2 : 1))
                }
            }
        }
    }
    @Test func selectedAckBetweenAttemptSampleAndRequestHasUnresolvedManualTiming() async throws {
        let evidence = try ledger()
        let record = CloudRecord(name: "generated-record-n1", envelope: Data([1]), systemFields: Data([2]))
        let observed = current()
        evidence.batch(origin: "generated-engine", batch: .init(id: 9, saves: [record], deletes: []), proof: nil, current: observed)
        let boundary = try evidence.acknowledgementBoundary()
        let startCallback = DispatchSemaphore(value: 0), callbackFinished = DispatchSemaphore(value: 0)
        // Hold synchronous MainActor prepare while a GCD worker finishes the
        // original flight. No suspension is invented inside prepare/request.
        let (callback, callbackCompletion) = AsyncStream<Bool>.makeStream()
        DispatchQueue.global().async {
            var completed = false
            defer {
                callbackCompletion.yield(completed)
                callbackCompletion.finish()
            }
            guard startCallback.wait(timeout: .now() + 5) == .success else { return }
            for (ordinal, event) in [CloudEngineEvent.sent(batchID: 9, saved: [record], failed: [], deleted: []),
                                     .stateUpdate(Data([9]))].enumerated() {
                for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
                    evidence.sdk(.init(origin: "generated-engine", ordinal: UInt64(ordinal + 1), event: event, phase: phase), current: observed)
                }
            }
            callbackFinished.signal()
            completed = true
        }
        var requests = 0
        do {
            let facts = try await AgentManualExchange.run(prepare: {
                startCallback.signal()
                guard callbackFinished.wait(timeout: .now() + 5) == .success else { throw AgentFailure.execution }
            }, request: { requests += 1; return true }, afterRequest: {}, exactResult: {
                try evidence.requireSelectedAcknowledgement(entity("n1"),
                    observations: [.savedPendingRemoved(name: record.name, requestID: 9)], attemptBoundary: boundary)
            }, afterResult: {})
            var callbackCompleted = false
            for await completed in callback { callbackCompleted = completed }
            #expect(callbackCompleted && requests == 1)
            #expect(facts.last?.operation == "n1-original-ack-after-attempt-boundary-timing-unresolved")
            let requestBoundary = try evidence.acknowledgementBoundary()
            let earlier = try evidence.requireSelectedAcknowledgement(entity("n1"),
                observations: [.savedPendingRemoved(name: record.name, requestID: 9)], attemptBoundary: requestBoundary)
            #expect(earlier.last?.operation == "n1-earlier-original-ack")
        } catch {
            // Join the bounded worker even when prepare refuses or a later step throws.
            for await _ in callback {}
            throw error
        }
    }
}
#endif
