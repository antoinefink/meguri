#if DEBUG
import Foundation
import Instrumentation
internal import MeguriCoreBindings

public struct OriginalCloudRoleFacts: Sendable {
    public let noteDigest: Data
    public let baselineDigest: Data
    public let fieldCount: UInt32
    public let templateCount: UInt32
    public let cardCount: UInt32
    public let deckCount: UInt32
    public let qOrdinal: UInt32
    // Source hashes are private observations, never expected verdict inputs.
    internal let mainSHA256: Data
    internal let walSHA256: Data?
    internal init(_ r: MeguriCoreBindings.CloudOriginalRoleFacts) {
        noteDigest = r.noteDigest; baselineDigest = r.baselineDigest
        fieldCount = r.fieldCount; templateCount = r.templateCount
        cardCount = r.cardCount; deckCount = r.deckCount; qOrdinal = r.qOrdinal
        mainSHA256 = r.sourceMainSha256; walSHA256 = r.sourceWalSha256
    }
}
public struct OriginalCloudOwnedFacts: Sendable {
    public let generation: UInt64
    public let seq: UInt64
    public let noteDigest: Data
    public let qChanged: Bool
}

public struct OriginalCloudEditFacts: Sendable {
    public let generation: UInt64
    public let seq: UInt64
    public let noteDigest: Data
    public let localRevision: UInt64?
    public let source: OpSource
    public let undoName: String?
    public let updatedNotes: UInt32
    internal init(_ r: MeguriCoreBindings.CloudOriginalEditFacts) {
        generation = r.generation; seq = r.seq; noteDigest = r.noteDigest
        localRevision = r.localRevision; source = OpSource(r.source)
        undoName = r.undoName; updatedNotes = r.updatedNotes
    }
}

// Native record correlation stays in process. Only the bounded canonical
// digest and sequence facts may be projected into the agent journal.
public struct OriginalCloudObservation: Sendable {
    public let generation: UInt64
    public let seq: UInt64
    public let qChanged: Bool
    public let entity: SmokeCloudEntity
    public let observedSeq: UInt64
    public let appliedSeq: UInt64?
    internal init(_ r: MeguriCoreBindings.CloudOriginalObservation) {
        generation = r.generation; seq = r.seq; qChanged = r.qChanged
        observedSeq = r.observedSeq; appliedSeq = r.appliedSeq
        let e = r.entity
        entity = .init(label: e.label, recordName: e.recordName, revision: e.revision,
            ackedRevision: e.ackedRevision, digest: e.digest, ackedEnvelopeDigest: e.ackedEnvelopeDigest,
            acknowledged: e.acknowledged, mediaBytes: e.mediaBytes, mediaSHA256: e.mediaSha256,
            blobPresent: e.blobPresent, mediaChangeCounter: e.mediaChangeCounter)
    }
}

// The caller retains verified-source authority through each await. Rust retains
// the full canonical baseline and all native identities behind this handle.
public actor OriginalCloudRoleInspection: CustomStringConvertible, CustomDebugStringConvertible {
    private let raw: MeguriCoreBindings.CloudOriginalRole
    public nonisolated let facts: OriginalCloudRoleFacts
    private init(raw: MeguriCoreBindings.CloudOriginalRole, facts: OriginalCloudRoleFacts) {
        self.raw = raw; self.facts = facts
    }
    public nonisolated var description: String { "OriginalCloudRoleInspection" }
    public nonisolated var debugDescription: String { description }
    public static func inspect(main: Data, wal: Data?) async throws -> OriginalCloudRoleInspection {
        let raw = try await QueryLane.shared.call(.testCloudOriginalRole) {
            try MeguriCoreBindings.testCloudOriginalRole(main: main, wal: wal)
        }
        let facts = try await QueryLane.shared.call(.cloudOriginalRoleFacts) {
            try OriginalCloudRoleFacts(raw.facts())
        }
        return OriginalCloudRoleInspection(raw: raw, facts: facts)
    }
    public func fixedEdit() async throws -> OriginalCloudEditFacts {
        let raw = raw
        try await CoreEventStream.shared.start()
        let started = try await InteractiveLane.shared.call(.cloudOriginalRoleStartFixedEdit) {
            try raw.startFixedEdit()
        }
        let job = try await Jobs.trackDraining(started.handle)
        let outcome = await job.outcome()
        let result = try await InteractiveLane.shared.call(.cloudOriginalEditResult) {
            try started.result.result().map(OriginalCloudEditFacts.init)
        }
        switch outcome {
        case .succeeded:
            guard let result else { throw BridgeError.internal("Original edit completed without its typed result") }
            return result
        case .cancelled: throw BridgeError.cancelled
        case .failed(let error): throw error
        }
    }
    public func validateOwned() async throws -> OriginalCloudOwnedFacts {
        let raw = raw
        return try await QueryLane.shared.call(.cloudOriginalRoleValidateOwned) {
            let r = try raw.validateOwned()
            return OriginalCloudOwnedFacts(generation: r.generation, seq: r.seq, noteDigest: r.noteDigest, qChanged: r.qChanged)
        }
    }
    public func observe() async throws -> OriginalCloudObservation {
        let raw = raw
        return try await QueryLane.shared.call(.cloudOriginalRoleObserve) {
            try OriginalCloudObservation(raw.observe())
        }
    }
}
#endif
