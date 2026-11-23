import AppModel
import CoreBridge
import Foundation
import Observation

/// The deck list's search (N14.2): Go to Deck's filter over the loaded tree,
/// and whether to offer creating the typed deck.
public nonisolated struct DeckSearch: Sendable {
    public struct Match: Identifiable, Hashable, Sendable {
        public let id: Int64
        public let name: String
        /// The ancestors' names, outermost first.
        public let ancestors: [String]
        public var fullName: String { (ancestors + [name]).joined(separator: "::") }
        /// The path as people read it (N20.3).
        public var spokenPath: String { (ancestors + [name]).joined(separator: " › ") }
    }

    public let matches: [Match]

    /// H6.1: the D17 snapshot's entries in its order, every deck record,
    /// filtered ones included; the default deck (id 1) only when `listsDefault`.
    public init(entries: [DeckNameEntry], listsDefault: Bool, query: String) {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { matches = []; return }
        let filter = GoToDeckFilter(query: query)
        matches = entries.compactMap { entry in
            guard entry.id != 1 || listsDefault, filter.matches(fullName: entry.fullName) else { return nil }
            let path = entry.fullName.components(separatedBy: "::")
            return Match(id: entry.id, name: path.last ?? entry.fullName, ancestors: Array(path.dropLast()))
        }
    }
}

/// A row of the deck search's results that Return or a click can open.
public nonisolated enum DeckSearchTarget: Hashable, Sendable {
    case deck(Int64)
    case create
}

public extension DeckSearch {
    /// The results in order, then the create row when it shows.
    func targets(offeringCreate: Bool) -> [DeckSearchTarget] {
        matches.map { .deck($0.id) } + (offeringCreate ? [.create] : [])
    }

    /// N20.1 (Mac): the highlighted row is the one the arrows chose while it is
    /// still listed; otherwise the first result, or the create row when nothing
    /// is listed.
    static func highlight(_ chosen: DeckSearchTarget?, in targets: [DeckSearchTarget]) -> DeckSearchTarget? {
        if let chosen, targets.contains(chosen) { return chosen }
        return targets.first
    }

    /// ↑ and ↓ over all rows, the create row included, wrapping at both ends (H6.3).
    static func move(_ current: DeckSearchTarget?, by offset: Int, in targets: [DeckSearchTarget]) -> DeckSearchTarget? {
        guard !targets.isEmpty else { return nil }
        guard let current, let index = targets.firstIndex(of: current) else {
            return offset < 0 ? targets.last : targets.first
        }
        let count = targets.count
        return targets[((index + offset) % count + count) % count]
    }
}

/// The deck search (N14.2, N20.1), held by `AppNavigation` so a fold, an
/// unfold or a layout switch keeps the query: the touch list's field and the
/// sidebar's share it.
@Observable @MainActor
public final class DeckListSearch {
    public var text = "" {
        didSet {
            guard text != oldValue else { return }
            // After each edit the first row is highlighted again (H6.3).
            chosen = nil
            // Typing into a cleared field starts a new search: its reads are its own (D18).
            if Self.blank(oldValue), !Self.blank(text) { presentation &+= 1 }
        }
    }
    /// The search the field holds now; D18's name read belongs to one.
    public private(set) var presentation: UInt64 = 0
    public var presented = false
    /// The row the Mac's arrow keys chose; see `DeckSearch.highlight`.
    public var chosen: DeckSearchTarget?
    /// Asks the sidebar's field for the focus (Find…, ⇧⌘O, "/").
    public private(set) var focusRequest: UInt64 = 0
    public init() {}
    /// The query to show results for; nil while the field is blank.
    public var query: String? {
        let query = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return query.isEmpty ? nil : query
    }
    /// A request the field has not taken yet: a sidebar that was hidden takes it
    /// when it appears.
    public private(set) var focusPending = false
    public func requestFocus() { focusPending = true; focusRequest &+= 1 }
    public func focusTaken() { focusPending = false }

    // MARK: Whether a deck has the typed name (D18)

    public enum NameCheck: Equatable, Sendable { case checking, absent, exists, failed }
    public struct NameCheckKey: Hashable, Sendable {
        public let name: String
        public let generation: UInt64
        public let presentation: UInt64
        public let attempt: UInt64
        public init(name: String, generation: UInt64, presentation: UInt64, attempt: UInt64) {
            self.name = name; self.generation = generation; self.presentation = presentation; self.attempt = attempt
        }
    }
    /// Raised by Try Again after a failed read.
    public private(set) var nameCheckAttempt: UInt64 = 0
    private var nameCheckKey: NameCheckKey?
    private var nameCheckState: NameCheck?

    public func nameCheckKey(generation: UInt64) -> NameCheckKey {
        NameCheckKey(name: text, generation: generation, presentation: presentation, attempt: nameCheckAttempt)
    }
    /// The state of the read for this text and generation; nil before it starts.
    public func nameCheck(generation: UInt64) -> NameCheck? {
        nameCheckKey == nameCheckKey(generation: generation) ? nameCheckState : nil
    }
    /// The create row shows only after a completed read for this very text and
    /// collection generation found no deck with the name. An unfinished, failed
    /// or out-of-date read never counts as no match.
    public func offersCreate(generation: UInt64) -> Bool {
        !Self.blank(text) && nameCheck(generation: generation) == .absent
    }
    /// Runs the core's name match for `key`; a result for an older text,
    /// generation or attempt is dropped.
    public func checkName(_ key: NameCheckKey, match: @MainActor (String) async throws -> DeckNameMatch) async {
        guard !Self.blank(key.name) else { return }
        nameCheckKey = key; nameCheckState = .checking
        let state: NameCheck
        do {
            let result = try await match(key.name)
            state = result.generation == key.generation ? (result.deck == nil ? .absent : .exists) : .failed
        } catch {
            state = .failed
        }
        guard nameCheckKey == key, !Task.isCancelled else { return }
        nameCheckState = state
    }
    public func retryNameCheck() { nameCheckAttempt &+= 1 }
    private var nameCheckTree: Int?
    /// A change to the deck tree (a deck added, renamed or deleted elsewhere)
    /// reads the name again for the same text, so the create row follows it.
    public func deckTreeChanged(_ tree: Int) {
        if let nameCheckTree, nameCheckTree != tree { nameCheckAttempt &+= 1 }
        nameCheckTree = tree
    }

    // MARK: The records searched (D17, H6.1)

    public struct SnapshotKey: Hashable, Sendable {
        public let generation: UInt64
        /// Changes whenever the deck tree does, so a write is searched at once.
        public let tree: Int
        public let attempt: UInt64
    }
    public private(set) var snapshot: DeckNameSnapshot?
    /// H6.1: the default deck is listed only while a card has `did` 1.
    public private(set) var listsDefault = false
    public private(set) var snapshotFailed = false
    public private(set) var snapshotAttempt: UInt64 = 0
    /// The key of the last read that finished and published.
    private var loadedKey: SnapshotKey?
    /// The newest read started: only it may publish, so an older read still
    /// pending (a cancelled view's) can neither clear nor replace a newer one.
    private var loadToken: UInt64 = 0

    public func snapshotKey(generation: UInt64, tree: Int) -> SnapshotKey {
        SnapshotKey(generation: generation, tree: tree, attempt: snapshotAttempt)
    }
    /// The entries of a snapshot of this generation; nil until one is read.
    public func entries(generation: UInt64) -> [DeckNameEntry]? {
        guard let snapshot, snapshot.generation == generation else { return nil }
        return snapshot.decks
    }
    /// Reads every deck record (D17); a result for an older key is dropped,
    /// and one of another generation counts as a failed read.
    public func loadSnapshot(_ key: SnapshotKey, read: @MainActor () async throws -> DeckNameSnapshot,
                             defaultHasCards: @MainActor () async throws -> Bool) async {
        guard loadedKey != key else { return }
        loadToken &+= 1
        let token = loadToken
        snapshotFailed = false
        let outcome: Result<(DeckNameSnapshot, Bool), any Error>
        do {
            let result = try await read()
            let listed = result.decks.contains { $0.id == 1 } ? try await defaultHasCards() : false
            outcome = .success((result, listed))
        } catch {
            outcome = .failure(error)
        }
        // A cancelled read, or one a newer read replaced, publishes nothing and
        // leaves the key unread, so the next view reads again.
        guard token == loadToken, !Task.isCancelled else { return }
        loadedKey = key
        switch outcome {
        case .success(let (result, listed)):
            guard result.generation == key.generation else { snapshot = nil; snapshotFailed = true; return }
            snapshot = result; listsDefault = listed
        case .failure:
            // The results shown may be older than this read: the error shows with them,
            // and a successful Retry replaces them.
            snapshotFailed = true
        }
    }
    public func retrySnapshot() { snapshotAttempt &+= 1; loadedKey = nil }
    /// H7's blank rule: empty, or Unicode White_Space only.
    static func blank(_ text: String) -> Bool {
        text.unicodeScalars.allSatisfy { $0.properties.isWhitespace }
    }
}
