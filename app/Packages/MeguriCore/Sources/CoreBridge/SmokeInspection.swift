#if DEBUG
import Foundation
import Instrumentation
internal import MeguriCoreBindings

// Debug/test-hooks only. Fixed observations over the currently owned Host;
// neither arbitrary SQL nor a path nor an expected verdict crosses the bridge.
public struct SmokeLocalIntegrity: Sendable {
    public let generation: UInt64
    public let seq: UInt64
    public let integrityOK: Bool
    public let schemaOK: Bool
    public let referenceFailures: UInt64
}
public struct SmokeCloudEntity: Sendable, Equatable {
    public let label: String
    public let recordName: String
    public let revision: UInt64
    public let ackedRevision: UInt64
    public let digest: Data
    public let acknowledged: Bool
    public let mediaBytes: UInt64?
    public let mediaSHA256: Data?
    public let blobPresent: Bool
    public let mediaChangeCounter: UInt64?
}
public struct SmokeCloudObservation: Sendable {
    public let generation: UInt64
    public let seq: UInt64
    public let publicationID: Data?
    public let publicationReason: UInt8?
    public let publicationCompleting: Bool
    public let publicationBindingID: Data?
    public let bindingID: Data?
    public let publicationInitial: Bool
    public let publicationComplete: Bool
    public let bindingUsable: Bool
    public let engine: String?
    public let entities: [SmokeCloudEntity]
    public let cardNoteID: Int64?
    public let lastReviewID: Int64?
}
public actor CoreSmokeInspection {
    public init() {}
    public nonisolated static func samePreview(_ lhs: CloudConnectionPreview, _ rhs: CloudConnectionPreview) -> Bool {
        lhs.raw === rhs.raw
    }
    public func integrity() async throws -> SmokeLocalIntegrity {
        try await QueryLane.shared.call(.testSmokeIntegrity) {
            let r = try MeguriCoreBindings.testSmokeIntegrity()
            return .init(generation: r.generation, seq: r.seq, integrityOK: r.integrityOk,
                         schemaOK: r.schemaOk, referenceFailures: r.referenceFailures)
        }
    }
    public func cloud(noteID: Int64? = nil, cardID: Int64? = nil, revlogID: Int64? = nil,
                      mediaName: String? = nil) async throws -> SmokeCloudObservation {
        try await QueryLane.shared.call(.testCloudSmokeObservation) {
            let r = try MeguriCoreBindings.testCloudSmokeObservation(noteId: noteID, cardId: cardID,
                                                                   revlogId: revlogID, mediaName: mediaName)
            return .init(generation: r.generation, seq: r.seq, publicationID: r.publicationId,
                publicationReason: r.publicationReason, publicationCompleting: r.publicationCompleting,
                publicationBindingID: r.publicationBindingId, bindingID: r.bindingId, publicationInitial: r.publicationInitial,
                publicationComplete: r.publicationComplete, bindingUsable: r.bindingUsable, engine: r.engine,
                entities: r.entities.map { .init(label: $0.label, recordName: $0.recordName,
                    revision: $0.revision, ackedRevision: $0.ackedRevision, digest: $0.digest,
                    acknowledged: $0.acknowledged, mediaBytes: $0.mediaBytes,
                    mediaSHA256: $0.mediaSha256, blobPresent: $0.blobPresent, mediaChangeCounter: $0.mediaChangeCounter) },
                cardNoteID: r.cardNoteId, lastReviewID: r.lastReviewId)
        }
    }
}
#endif
