import CoreBridge
import Foundation

/// The service change confirmation (`sync/status-strings` T21.1): a question,
/// one paragraph saying what happens, then what the old service has not
/// received, as far as this device knows. Display only: the change itself is
/// `changeProvider`'s, and its refusals stay the core's (MP3 step 1).
public struct ServiceSwitchConfirmation: Sendable, Equatable {
    /// One line of the "not yet sent" list, each with a known count above 0.
    public enum Unsent: Sendable, Equatable {
        case changes(UInt64), heldBack(UInt64), media(UInt64)
        /// The service reports pending changes without a count (Meguri Sync's round).
        case someChanges
    }
    /// The sentence after the list, or in its place.
    public enum Closing: Sendable, Equatable {
        /// Every count is known and 0.
        case allSent
        /// Every count is known; the list names what is not sent.
        case listed
        /// Nothing is known: one sentence, no list.
        case unknown
        /// The list names what is known; something else is not known.
        case unknownBeyond
    }
    public let titleKey: String
    public let messageKey: String
    public let confirmKey: String
    public let unsent: [Unsent]
    public let closing: Closing
    public let acceptanceUnknown: Bool

    /// `destination` is the Service control's tag: "localOnly", "meguri" or "iCloud".
    public init(source: SyncProviderSelection, destination: String, facts: ServiceSwitchFacts) {
        switch destination {
        case "meguri":
            titleKey = "sync.switch.title.meguri"; messageKey = "sync.switch.message.meguri"; confirmKey = "sync.switch.confirm.meguri"
        case "iCloud":
            titleKey = "sync.switch.title.iCloud"; messageKey = "sync.switch.message.iCloud"; confirmKey = "sync.switch.confirm.iCloud"
        default:
            titleKey = "sync.switch.title.off"; confirmKey = "sync.switch.confirm.off"
            messageKey = source == .meguri ? "sync.switch.message.off.meguri" : "sync.switch.message.off.iCloud"
        }
        var unsent: [Unsent] = []
        var unknown = false
        switch facts.changes {
        case .count(let count): if count > 0 { unsent.append(.changes(count)) }
        case .some: unsent.append(.someChanges)
        case .unknown: unknown = true
        }
        // Held answers are not counted in pending changes (P14.4); unreported is unknown, never none.
        if let held = facts.heldBack { if held > 0 { unsent.append(.heldBack(held)) } } else { unknown = true }
        if let media = facts.media { if media > 0 { unsent.append(.media(media)) } } else { unknown = true }
        self.unsent = unsent
        closing = unknown ? (unsent.isEmpty ? .unknown : .unknownBeyond) : (unsent.isEmpty ? .allSent : .listed)
        acceptanceUnknown = facts.acceptanceUnknown
    }

    /// The old service's name in the lines: T19.1's names, iCloud without its label inside a sentence.
    public static func serviceName(_ source: SyncProviderSelection) -> String {
        SyncText.value(source == .meguri ? "sync.service.meguri" : "sync.service.iCloud.name")
    }

    /// The alert's message: the paragraph, then the list and its closing
    /// sentence, then the acceptance line, each block on lines of its own.
    public func message(source: SyncProviderSelection, locale: Locale = .autoupdatingCurrent) -> String {
        let service = Self.serviceName(source)
        var blocks = [SyncText.value(messageKey)]
        var detail: [String] = []
        if !unsent.isEmpty {
            detail.append(SyncText.format("sync.switch.unsent.heading", locale: locale, [service]))
            detail += unsent.map { line in
                switch line {
                case .changes(let count): SyncText.format("sync.switch.unsent.changes", locale: locale, [Int64(clamping: count)])
                case .heldBack(let count): SyncText.format("sync.switch.unsent.heldBack", locale: locale, [Int64(clamping: count)])
                case .media(let count): SyncText.format("sync.switch.unsent.media", locale: locale, [Int64(clamping: count)])
                case .someChanges: SyncText.value("sync.switch.unsent.some")
                }
            }
        }
        switch closing {
        case .allSent: detail.append(SyncText.format("sync.switch.unsent.none", locale: locale, [service]))
        case .listed: detail.append(SyncText.value("sync.switch.unsent.kept"))
        case .unknown: detail.append(SyncText.format("sync.switch.unknown", locale: locale, [service]))
        case .unknownBeyond: detail.append(SyncText.value("sync.switch.unknown.more"))
        }
        blocks.append(detail.joined(separator: "\n"))
        if acceptanceUnknown { blocks.append(SyncText.value("sync.switch.acceptanceUnknown")) }
        return blocks.joined(separator: "\n\n")
    }
}
