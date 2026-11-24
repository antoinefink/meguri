import CoreBridge
import Foundation
import Observation

/// The commands that collect input in a sheet before they run (U276;
/// `browser/workspace` V8.1, V8.2; N21.6). They ask no question: each is one
/// undo step.
public nonisolated enum BrowseSheetKind: Sendable, Hashable {
    case changeDeck, addTags, removeTags, setDueDate, forget

    public var command: BrowseCommand {
        switch self {
        case .changeDeck: .changeDeck
        case .addTags: .addTags
        case .removeTags: .removeTags
        case .setDueDate: .setDueDate
        case .forget: .forget
        }
    }

    init?(_ command: BrowseCommand) {
        switch command {
        case .changeDeck: self = .changeDeck
        case .addTags: self = .addTags
        case .removeTags: self = .removeTags
        case .setDueDate: self = .setDueDate
        case .forget: self = .forget
        default: return nil
        }
    }
}

/// The reads the sheets open with, besides the search session's. Each reads
/// and writes nothing. Production is `CoreBrowseSheetReads`.
@MainActor public protocol BrowseSheetReads: AnyObject {
    /// `decks/decks` D17: every deck record, in the tree's order.
    func deckNames() async throws -> DeckNameSnapshot
    /// D18: the deck that has the typed name, if any.
    func matchDeckName(_ name: String) async throws -> DeckNameMatch
    /// `decks/deck-list` H6.1: the default deck is listed only while a card
    /// has `did` 1.
    func defaultDeckHasCards() async throws -> Bool
    /// `browser/bulk-actions` B10: what Set Due Date and Forget prefill.
    func sheetDefaults() async throws -> BrowseSheetDefaults
    /// `sched/set-due-date` S2, S9: the days text read, or its refusal
    /// (`BridgeError.schedOpRefused(.invalidDays)`).
    func parseDueDays(_ text: String) async throws -> DueDays
    /// U277: a note type's name, fields in order and Sort Field, for Find
    /// and Replace's In menu and Find Duplicates' Field menu.
    func notetypeFields(_ id: Int64) async throws -> BrowseNotetypeFields
}

/// Reads without the note types' fields (the test hooks' samples): the
/// menus then offer All Fields and Tags only.
public extension BrowseSheetReads {
    func notetypeFields(_ id: Int64) async throws -> BrowseNotetypeFields { throw BrowseCommandUnsupported() }
}

/// The sheets' reads over CoreBridge.
@MainActor public final class CoreBrowseSheetReads: BrowseSheetReads {
    private let decks = CoreDeckService()
    private let sched = CoreSchedOpsService()
    private let notetypes = CoreNotetypeService()
    public init() {}
    public func deckNames() async throws -> DeckNameSnapshot { try await decks.allDeckNames() }
    public func matchDeckName(_ name: String) async throws -> DeckNameMatch { try await decks.matchDeckName(name: name) }
    public func defaultDeckHasCards() async throws -> Bool { !(try await decks.studyState(deckID: 1).empty) }
    public func sheetDefaults() async throws -> BrowseSheetDefaults { try await CoreBrowseQuery.sheetDefaults() }
    public func parseDueDays(_ text: String) async throws -> DueDays { try await sched.parseDueDays(text) }
    public func notetypeFields(_ id: Int64) async throws -> BrowseNotetypeFields {
        let record = try await notetypes.readNotetype(id: id)
        return BrowseNotetypeFields(id: record.id, name: record.name, fields: record.fields.sorted { $0.ord < $1.ord }.map(\.name),
                                    sortField: Int(record.sortField))
    }
}

/// A deck the Change Deck sheet lists: N20.1's two lines, its last name
/// component over its parents.
public nonisolated struct BrowseDeckRow: Identifiable, Hashable, Sendable {
    public let id: Int64
    public let name: String
    /// The ancestors' names, outermost first.
    public let parents: [String]
    /// A target card's home deck now (B1, V7): the sheet marks it.
    public let current: Bool
    /// The path as people read it.
    public var spokenPath: String { (parents + [name]).joined(separator: " › ") }
}

/// A row of the Change Deck sheet that Return or a click chooses.
public nonisolated enum BrowseDeckChoice: Hashable, Sendable {
    case deck(Int64)
    /// "Create deck named “<text>”" (`decks/decks` D18; B7.2 step 0).
    case create
}

/// Set Due Date's text as the core reads it (`sched/set-due-date` S2, S9).
public nonisolated enum BrowseDueCheck: Hashable, Sendable {
    case checking
    case valid(DueDays)
    case invalid(InvalidDaysKind)
    /// The core could not read the text (it was not refused): Set stays
    /// enabled, and the operation reads it again (S9), its refusal shown then.
    case unchecked
}

/// Why a sheet's command wrote nothing (`browser/bulk-framework` K4).
public nonisolated enum BrowseSheetFailure: Hashable, Sendable {
    /// The rows moved on since the sheet opened: the core refused them (K1).
    case rowsChanged
    /// B7.2's refusals of the target deck.
    case deck(DeckRefusal)
    case invalidDays(InvalidDaysKind)
    /// U277: Find and Replace would now change more notes than its preview
    /// showed (F11); the preview shows the new count.
    case countChanged(UInt64)
    /// U277: the inputs cannot run (F10).
    case findReplace(BrowseFindReplaceInvalid)
    /// U277: a target note is damaged (F6 step 7).
    case damaged
    /// Anything else: nothing was changed.
    case failed
}

/// One open command sheet (U276). It acts on exactly the rows it opened on,
/// as index ranges of the list at that revision: when the list moved on
/// meanwhile the core refuses them and nothing is written (`browser/bulk-framework`
/// K1; the selection-at-revision rule of U275). Return confirms (never a
/// destructive default), Esc cancels, and while the command runs Cancel stops
/// it until it commits.
@Observable @MainActor
public final class BrowseSheet: Identifiable {
    public enum Phase: Hashable, Sendable {
        case editing
        /// The command runs; its progress once one is reported.
        case running(BrowseActionProgress?)
        /// Cancel was asked for; the command ends without writing.
        case cancelling
    }

    public let id: UInt64
    public let kind: BrowseSheetKind
    /// The rows the command acts on, as they were when the sheet opened.
    public let target: BrowseTarget
    /// The target's counts (V7), once read: the preview's numbers and the
    /// decks Change Deck marks.
    public internal(set) var counts: BrowseSelectionCounts? {
        didSet { if kind == .changeDeck { highlightOwnDeck() } }
    }
    @ObservationIgnored private var ownDeckOffered = false
    public internal(set) var phase: Phase = .editing
    public internal(set) var failure: BrowseSheetFailure?

    // MARK: Change Deck (B7, N20.1)

    /// The search field over the decks.
    public var deckQuery = "" {
        didSet {
            guard deckQuery != oldValue else { return }
            // After each edit the first row is highlighted again (N20.1).
            chosenDeck = nil
            failure = nil
            checkName()
        }
    }
    /// The normal decks (filtered ones are never targets, B7.1), D17's order.
    public internal(set) var decks: [DeckNameEntry]?
    public internal(set) var deckReadFailed = false
    /// The row the arrow keys or a click chose.
    public var chosenDeck: BrowseDeckChoice?
    /// D18: the text a completed read found no deck with.
    private var absentName: String?
    @ObservationIgnored private var decksGeneration: UInt64?
    @ObservationIgnored private var listsDefault = false
    @ObservationIgnored private var nameCheck: Task<Void, Never>?

    // MARK: Add Tags and Remove Tags (B9)

    /// The token field's tags, as typed.
    public var tags: [String] = [] {
        didSet { if tags != oldValue { failure = nil } }
    }
    /// Remove Tags: the tags the target notes store (V8.1), once read.
    public internal(set) var selectionTags: [String]?
    /// The token field holds something while it is edited (tokens or typed
    /// text): the default button is enabled for it, and the field hands its
    /// tokens to `tags` before the command reads them.
    public var typingTag = false

    // MARK: Set Due Date (B10, `sched/set-due-date`)

    /// The days text, sent as typed (S6 stores it so).
    public var dueText = "" {
        didSet {
            guard dueText != oldValue else { return }
            failure = nil
            dueEdited = true
            checkDue()
        }
    }
    public internal(set) var due: BrowseDueCheck = .checking
    /// The person changed the text: a late prefill leaves it alone.
    @ObservationIgnored private var dueEdited = false
    /// Raised when the text is filled from the remembered key: the field
    /// selects it, so typing replaces it.
    public internal(set) var dueSelectRequests: UInt64 = 0
    @ObservationIgnored private var dueToken: UInt64 = 0

    // MARK: Forget (B10 Reset, `sched/manual-ops` M6)

    public var restorePosition = true { didSet { touched = true } }
    public var resetCounts = false { didSet { touched = true } }
    @ObservationIgnored private var touched = false

    @ObservationIgnored private weak var commands: BrowseCommands?
    @ObservationIgnored private let reads: any BrowseSheetReads
    @ObservationIgnored private var running: BrowseRunningAction?
    @ObservationIgnored private var loading: Task<Void, Never>?

    init(id: UInt64, kind: BrowseSheetKind, target: BrowseTarget, commands: BrowseCommands, reads: any BrowseSheetReads,
         defaults: BrowseSheetDefaults?) {
        self.id = id
        self.kind = kind
        self.target = target
        self.commands = commands
        self.reads = reads
        if let defaults {
            dueText = defaults.setDueDays
            restorePosition = defaults.restorePosition
            resetCounts = defaults.resetCounts
        }
    }

    /// The remembered inputs arrived after the sheet opened: they fill what
    /// the person has not changed yet.
    func prefill(_ defaults: BrowseSheetDefaults) {
        guard phase == .editing else { return }
        if kind == .setDueDate, !dueEdited {
            dueText = defaults.setDueDays
            dueEdited = false
            dueSelectRequests &+= 1
        }
        if kind == .forget, !touched {
            restorePosition = defaults.restorePosition
            resetCounts = defaults.resetCounts
        }
    }

    // MARK: Change Deck's rows

    /// The decks the text finds (H6.2's filter; every deck while it is
    /// empty), each marked when a target card is in it now.
    public var deckRows: [BrowseDeckRow] {
        guard let decks else { return [] }
        let query = deckQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        let filter = query.isEmpty ? nil : GoToDeckFilter(query: query)
        let current = Set(counts?.deckIDs ?? [])
        return decks.compactMap { entry in
            guard entry.id != 1 || listsDefault, filter?.matches(fullName: entry.fullName) ?? true else { return nil }
            let path = entry.fullName.components(separatedBy: "::")
            return BrowseDeckRow(id: entry.id, name: path.last ?? entry.fullName, parents: Array(path.dropLast()),
                                 current: current.contains(entry.id))
        }
    }

    /// D18: the create row shows once a completed read found no deck with
    /// the typed name; an unfinished or failed read never counts as none.
    public var offersCreate: Bool {
        !Self.blank(deckQuery) && absentName == deckQuery
    }

    /// The rows in order, then the create row when it shows.
    public var deckChoices: [BrowseDeckChoice] {
        deckRows.map { .deck($0.id) } + (offersCreate ? [.create] : [])
    }

    /// The highlighted row: the one chosen while it is listed; with no text
    /// the selection's own deck when it has one; otherwise the first row.
    public var highlightedDeck: BrowseDeckChoice? {
        let choices = deckChoices
        if let chosenDeck, choices.contains(chosenDeck) { return chosenDeck }
        return choices.first
    }

    /// With no text and nothing chosen, the selection's own deck is
    /// highlighted, once both the decks and the counts are in; it is set as a
    /// choice, so a later read never moves the highlight under the person.
    private func highlightOwnDeck() {
        guard !ownDeckOffered, decks != nil, let counts else { return }
        ownDeckOffered = true
        guard chosenDeck == nil, Self.blank(deckQuery), let deck = counts.deck, deckChoices.contains(.deck(deck)) else { return }
        chosenDeck = .deck(deck)
    }

    /// ↑ and ↓ over every row, the create row included, wrapping (H6.3).
    public func moveHighlight(by offset: Int) {
        let choices = deckChoices
        guard !choices.isEmpty else { return }
        guard let current = highlightedDeck, let index = choices.firstIndex(of: current) else {
            chosenDeck = offset < 0 ? choices.last : choices.first
            return
        }
        let count = choices.count
        chosenDeck = choices[((index + offset) % count + count) % count]
    }

    /// Return or a double click on a row: that row is chosen and confirmed.
    public func choose(_ choice: BrowseDeckChoice) {
        chosenDeck = choice
        confirm()
    }

    private func checkName() {
        nameCheck?.cancel()
        let text = deckQuery
        guard !Self.blank(text) else { return }
        nameCheck = Task { [weak self, reads] in
            guard let match = try? await reads.matchDeckName(text), !Task.isCancelled, let self, self.deckQuery == text else {
                return
            }
            if match.deck == nil, self.decksGeneration == nil || self.decksGeneration == match.generation {
                self.absentName = text
            }
        }
    }

    /// H7's blank rule: empty, or white space only.
    static func blank(_ text: String) -> Bool {
        text.unicodeScalars.allSatisfy { $0.properties.isWhitespace }
    }

    // MARK: Set Due Date's text

    private func checkDue() {
        dueToken &+= 1
        let token = dueToken, text = dueText
        // S2: the empty text is refused at once; the core reads the rest.
        guard !text.isEmpty else {
            due = .invalid(.empty)
            return
        }
        due = .checking
        Task { [weak self, reads] in
            let check: BrowseDueCheck
            do {
                check = .valid(try await reads.parseDueDays(text))
            } catch BridgeError.schedOpRefused(.invalidDays(let kind)) {
                check = .invalid(kind)
            } catch {
                check = .unchecked
            }
            guard let self, self.dueToken == token else { return }
            self.due = check
        }
    }

    // MARK: Confirming

    /// The tags the field holds, split at white space as B9's entry splits
    /// them, each once.
    public var tagList: [String] {
        var seen = Set<String>(), out: [String] = []
        for tag in tags.flatMap({ $0.split(whereSeparator: \.isWhitespace).map(String.init) }) where seen.insert(tag).inserted {
            out.append(tag)
        }
        return out
    }

    /// The default button is enabled: the input is complete and valid, and
    /// nothing runs.
    public var canConfirm: Bool {
        guard phase == .editing else { return false }
        switch kind {
        case .changeDeck: return highlightedDeck != nil
        case .addTags, .removeTags: return !tagList.isEmpty || typingTag
        case .setDueDate:
            switch due {
            case .valid, .unchecked: return true
            case .checking, .invalid: return false
            }
        case .forget: return true
        }
    }

    public var isRunning: Bool { phase != .editing }

    /// The command on the sheet's rows, with what it collected.
    public func confirm() {
        guard canConfirm, let commands else { return }
        let action: Either
        switch kind {
        case .changeDeck:
            switch highlightedDeck {
            case .deck(let id): action = .card(.changeDeck(deck: id))
            case .create: action = .card(.changeDeckToName(deckQuery))
            case nil: return
            }
        case .addTags, .removeTags:
            // B9: the op refuses a text that gives no tag; the entry never sends one.
            let text = tagList.joined(separator: " ")
            guard !text.isEmpty else { return }
            action = .note(kind == .addTags ? .addTags(text) : .removeTags(text))
        case .setDueDate: action = .card(.setDueDate(dueText))
        case .forget: action = .card(.forget(restorePosition: restorePosition, resetCounts: resetCounts))
        }
        failure = nil
        phase = .running(nil)
        commands.runSheet(self, action)
    }

    enum Either {
        case card(BulkCardAction)
        case note(BulkNoteAction)
    }

    /// Cancel or Esc: closes the sheet; while the command runs, asks it to
    /// stop (until its commit; one that already commits finishes).
    public func cancel() {
        switch phase {
        case .editing:
            commands?.closeSheet(self)
        case .running(let progress):
            guard progress?.cancellable ?? true else { return }
            phase = .cancelling
            if let running { Task { await running.cancel() } }
        case .cancelling:
            return
        }
    }

    /// Cancel is enabled: always while editing; while running only before
    /// the commit.
    public var canCancel: Bool {
        switch phase {
        case .editing: true
        case .running(let progress): progress?.cancellable ?? true
        case .cancelling: false
        }
    }

    // MARK: Driven by BrowseCommands

    func started(_ running: BrowseRunningAction) {
        self.running = running
        if phase == .cancelling { Task { await running.cancel() } }
    }

    func progressed(_ progress: BrowseActionProgress) {
        if case .running = phase { phase = .running(progress) }
    }

    func failed(_ failure: BrowseSheetFailure) {
        running = nil
        phase = .editing
        self.failure = failure
    }

    /// The reads the sheet opens with.
    func load(session: (any BrowseSearching)?, facts: BrowseSelectionCounts?, waitFirst: Task<BrowseTargetFacts?, Never>?,
              defaultsKnown: Bool) {
        counts = facts
        loading = Task { [weak self, target, kind] in
            if facts == nil, let session {
                // Two counts reads of a session at once cancel the older one in the
                // core: after the selection's read, and once more if a newer read
                // cancelled this one.
                _ = await waitFirst?.value
                var read = try? await session.counts(of: target)
                if read == nil, !Task.isCancelled { read = try? await session.counts(of: target) }
                self?.counts = read
            }
            if kind == .removeTags, let session, !Task.isCancelled {
                // A failed read leaves the suggestions unknown, never "no tags".
                if let tags = try? await session.tags(of: target) { self?.selectionTags = tags }
            }
        }
        if kind == .changeDeck {
            Task { [weak self, reads] in
                do {
                    let snapshot = try await reads.deckNames()
                    let listed = snapshot.decks.contains { $0.id == 1 } ? ((try? await reads.defaultDeckHasCards()) ?? true) : false
                    guard let self else { return }
                    self.decksGeneration = snapshot.generation
                    self.listsDefault = listed
                    self.decks = snapshot.decks.filter { !$0.filtered }
                    self.highlightOwnDeck()
                } catch {
                    self?.deckReadFailed = true
                }
            }
        }
        // Without the remembered text yet, the field waits for it rather than
        // showing the empty text's refusal.
        if kind == .setDueDate, defaultsKnown { checkDue() }
    }

    /// The sheet closed: its reads stop (a large selection's tags read holds
    /// a search reader until it ends).
    func closed() {
        loading?.cancel()
        nameCheck?.cancel()
    }

    static func failure(_ error: any Error) -> BrowseSheetFailure? {
        guard let error = error as? BridgeError else { return .failed }
        switch error {
        case .cancelled: return nil
        // K1: a selection the core cannot resolve at its revision (the list
        // moved on, or its rows went) is refused as an invalid argument.
        case .browse(.staleRevision), .browse(.unknownHandle), .browse(.outOfRange), .invalidArgument: return .rowsChanged
        case .deckRefused(let refusal): return .deck(refusal)
        case .schedOpRefused(.invalidDays(let kind)): return .invalidDays(kind)
        default: return .failed
        }
    }
}
