import CoreBridge
import Foundation

/// What the service change confirmation can say about this device's work
/// that the old service has not received (MP3 step 1, `sync/status-strings`
/// T21.1). Read from local state only: opening the confirmation starts no request.
public struct ServiceSwitchFacts: Sendable, Equatable {
    /// Changes not yet sent: a count, "some" when the service reports pending
    /// work without a count, or unknown.
    public enum Changes: Sendable, Equatable { case count(UInt64), some, unknown }
    public var changes: Changes
    /// Review answers held back; nil when not reported.
    public var heldBack: UInt64?
    /// Media files on this device not yet sent; nil when not known. Files still
    /// to download from the service are not counted: they were never this device's to send.
    public var media: UInt64?
    /// A Meguri Sync upload whose acceptance is unknown (MP3), whatever the old service.
    public var acceptanceUnknown: Bool
    public init(changes: Changes, heldBack: UInt64? = nil, media: UInt64? = nil, acceptanceUnknown: Bool = false) {
        self.changes = changes; self.heldBack = heldBack; self.media = media; self.acceptanceUnknown = acceptanceUnknown
    }
}

extension SyncCoordinator {
    /// The old service's local facts for the confirmation. Meguri Sync's
    /// round reports pending work without a count,
    /// and media counts only after a round or setup reported them.
    public func serviceSwitchFacts(source: SyncProviderSelection) -> ServiceSwitchFacts {
        if let facts = renderServiceSwitch?.facts { return facts }
        let acceptanceUnknown = meguriStage == .acceptanceUnknown || meguriRoutine == .acceptanceUnknown
        guard source == .meguri else { return .init(changes: .unknown, acceptanceUnknown: acceptanceUnknown) }
        var media: UInt64?
        // This device's side only (T18.6): files waiting to upload or excluded from it.
        // A setup's accounting is newer than a retained round report: a round clears it.
        if let accounting = meguriAccounting {
            media = UInt64(accounting.files.filter { $0.state != .transferred }.count)
        } else if let report = meguriRoundReport?.media, case .media(let facts) = report.outcome,
                  let excluded = facts.excluded, let pending = facts.pending {
            media = Self.sum(excluded, pending)
        }
        // Meguri Sync sends review answers as changes, so it holds none back apart
        // from them: what is known about its changes covers the answers too.
        let changes: ServiceSwitchFacts.Changes = meguriRoutine == .pending ? .some : .unknown
        return .init(changes: changes, heldBack: changes == .unknown ? nil : 0, media: media,
                     acceptanceUnknown: acceptanceUnknown)
    }
    private static func sum(_ first: UInt64?, _ second: UInt64?) -> UInt64? {
        guard let first, let second else { return nil }
        let (value, overflow) = first.addingReportingOverflow(second)
        return overflow ? nil : value
    }
}
