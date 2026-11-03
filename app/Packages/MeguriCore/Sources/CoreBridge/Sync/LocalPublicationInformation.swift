import Foundation
internal import MeguriCoreBindings

public enum LocalPublicationCondition: Sendable, Equatable { case pending, clear, localOnly }
public enum LocalPendingDecisionReason: Sendable, Equatable { case restore, colpkgImport, manualImport }
public enum IsolatedLocalContent: Sendable, Equatable { case replacedLocally, fileMismatch, unverified }
public enum LocalBackupKnowledge: Sendable, Equatable { case unknown, notTakenForExternalReplacement }
public struct IsolatedLocalInformation: Sendable, Equatable {
    public let pendingCounts: [CloudCountKind: UInt64]
    public let content: IsolatedLocalContent
    public let backup: LocalBackupKnowledge
}
public struct LocalTrackingInformation: Sendable, Equatable {
    /// Ordinary bindings in this build's container/environment only. No
    /// account identities, salvage owners or other-device work is represented.
    public let isolated: [IsolatedLocalInformation]
    public let unresolvedConflictCopies: UInt64
}
/// Read-only facts, without a confirmation handle or offered actions. This
/// cannot be passed to publication, adoption, Resume or Keep Local.
public struct LocalPublicationInformation: Sendable, Equatable {
    public let generation: UInt64
    public let collectionPresent: Bool
    public let condition: LocalPublicationCondition?
    /// nil includes legacy Pending with no recorded reason; do not infer one.
    public let pendingReason: LocalPendingDecisionReason?
    public let localCounts: [CloudCountKind: UInt64]
    public let localEmpty: Bool?
    /// Canonical max revlog ID and col.mod, in epoch milliseconds. No review
    /// rows means nil; no collection means both are nil.
    public let lastReviewMilliseconds: Int64?
    public let lastChangeMilliseconds: Int64?
    /// nil means no available local tracking runtime, never zero pending work.
    public let tracking: LocalTrackingInformation?

    init(_ value: MeguriCoreBindings.CloudLocalPublicationInformation) {
        generation = value.generation
        collectionPresent = value.collectionPresent
        condition = value.condition.map { value in
            switch value { case .pending: .pending; case .clear: .clear; case .localOnly: .localOnly }
        }
        pendingReason = value.pendingReason.map { value in
            switch value { case .restore: .restore; case .colpkgImport: .colpkgImport; case .manualImport: .manualImport }
        }
        localCounts = cloudKindCounts(value.localCounts)
        localEmpty = value.localEmpty
        lastReviewMilliseconds = value.lastReviewMs
        lastChangeMilliseconds = value.lastChangeMs
        tracking = value.tracking.map { value in
            LocalTrackingInformation(isolated: value.isolated.map { value in
                let content: IsolatedLocalContent = switch value.content {
                case .replacedLocally: .replacedLocally; case .fileMismatch: .fileMismatch; case .unverified: .unverified
                }
                let backup: LocalBackupKnowledge = switch value.backup {
                case .unknown: .unknown; case .notTakenForExternalReplacement: .notTakenForExternalReplacement
                }
                return IsolatedLocalInformation(pendingCounts: cloudKindCounts(value.pendingCounts), content: content, backup: backup)
            }, unresolvedConflictCopies: value.unresolvedConflictCopies)
        }
    }
}

func cloudKindCounts(_ rows: [MeguriCoreBindings.CloudKindCount]) -> [CloudCountKind: UInt64] {
    Dictionary(uniqueKeysWithValues: rows.map { row in
        let kind: CloudCountKind = switch row.kind {
        case .note: .note; case .card: .card; case .revlog: .revlog; case .deck: .deck; case .preset: .preset
        case .notetype: .notetype; case .tag: .tag; case .config: .config; case .mediaName: .mediaName
        }
        return (kind, row.count)
    })
}
