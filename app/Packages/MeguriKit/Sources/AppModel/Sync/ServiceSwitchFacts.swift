import CoreBridge
import Foundation

/// What the service change confirmation can say about this device's work
/// that the old service has not received (MP3 step 1, `sync/status-strings`
/// T19.2). Read from local state only: opening the confirmation starts no request.
public struct ServiceSwitchFacts: Sendable, Equatable {
    /// Changes not yet sent: a count, "some" when the service reports pending
    /// work without a count, or unknown.
    public enum Changes: Sendable, Equatable { case count(UInt64), some, unknown }
    public var changes: Changes
    /// Review answers held back (iCloud's P14.4 `heldBack`); nil when not reported.
    public var heldBack: UInt64?
    /// Media files not yet transferred; nil when not known.
    public var media: UInt64?
    /// A Meguri Sync upload whose acceptance is unknown (MP3), whatever the old service.
    public var acceptanceUnknown: Bool
    public init(changes: Changes, heldBack: UInt64? = nil, media: UInt64? = nil, acceptanceUnknown: Bool = false) {
        self.changes = changes; self.heldBack = heldBack; self.media = media; self.acceptanceUnknown = acceptanceUnknown
    }
}

extension SyncCoordinator {
    /// The old service's local facts for the confirmation. iCloud reports its
    /// counts (P14.4); Meguri Sync's round reports pending work without a count,
    /// and media counts only after a round or setup reported them.
    public func serviceSwitchFacts(source: SyncProviderSelection) -> ServiceSwitchFacts {
        if let facts = renderServiceSwitch?.facts { return facts }
        let acceptanceUnknown = meguriStage == .acceptanceUnknown || meguriRoutine == .acceptanceUnknown
        if source == .iCloud, providerState?.selection == .iCloud, let status {
            return .init(changes: .count(status.pending), heldBack: status.heldBack,
                         media: Self.sum(status.media.uploadFiles, status.media.downloadFiles),
                         acceptanceUnknown: acceptanceUnknown)
        }
        guard source == .meguri else { return .init(changes: .unknown, acceptanceUnknown: acceptanceUnknown) }
        var media: UInt64?
        if let report = meguriRoundReport?.media, case .media(let facts) = report.outcome,
           let excluded = facts.excluded, let pending = facts.pending, let awaiting = facts.awaiting,
           let local = Self.sum(excluded, pending) {
            media = Self.sum(local, awaiting)
        } else if let accounting = meguriAccounting {
            media = Self.sum(UInt64(accounting.files.filter { $0.state != .transferred }.count), accounting.remotePending)
        }
        return .init(changes: meguriRoutine == .pending ? .some : .unknown, media: media,
                     acceptanceUnknown: acceptanceUnknown)
    }
    private static func sum(_ first: UInt64?, _ second: UInt64?) -> UInt64? {
        guard let first, let second else { return nil }
        let (value, overflow) = first.addingReportingOverflow(second)
        return overflow ? nil : value
    }
}
