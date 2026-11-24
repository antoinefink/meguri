import CoreBridge
import Foundation
import Observation

/// A command of `browser/workspace` V8.1, on the rows chosen in Browse.
public nonisolated enum BrowseCommand: Hashable, Sendable {
    case changeDeck, addTags, removeTags
    /// 1 to 7, or 0 for No Flag; it sets (`tags/flags-and-marked` F2).
    case flag(UInt8)
    case suspend, unsuspend, bury, unbury
    case setDueDate, forget
    case changeNoteType, editNoteType
    case findReplace, findDuplicates
    case delete
    /// N21.8's Note › Edit Note in Browse: the inspector's first field.
    case editNote

    /// The commands that run now (U275) or open their sheet (U276, U277).
    /// Change Note Type opens a sheet U284 builds: until then it is in its
    /// places, disabled.
    public var built: Bool {
        switch self {
        case .flag, .suspend, .unsuspend, .bury, .unbury, .delete, .editNoteType, .editNote: true
        case .changeDeck, .addTags, .removeTags, .setDueDate, .forget, .findReplace, .findDuplicates: true
        case .changeNoteType: false
        }
    }

    /// V8.1: disabled with no selection, except these two.
    public var needsSelection: Bool { self != .findReplace && self != .findDuplicates }
}

/// What is known of the rows a command would act on (V7, V8.1): read
/// from the rows on screen at once where they tell it, then from the core.
/// A nil member is not known yet.
public nonisolated struct BrowseTargetFacts: Equatable, Sendable {
    public var mode: BrowseMode
    public var rows: Int
    /// The target cards (B1: every card of each selected note in Notes).
    public var cards: UInt64?
    /// The target notes.
    public var notes: UInt64?
    public var suspended: UInt64?
    public var buried: UInt64?
    /// The target notes' note types, ascending.
    public var notetypes: [Int64]?
    /// The cards a Delete removes: every card of the target notes; read
    /// for one target note only (V8.2 asks for more than one note anyway).
    public var noteCards: UInt64?
    /// V7: the target cards' distinct home decks, and that deck when one.
    public var decks: UInt64?
    public var deck: Int64?
    /// Every one of those decks, ascending.
    public var deckIDs: [Int64]?

    public init(mode: BrowseMode, rows: Int, cards: UInt64? = nil, notes: UInt64? = nil, suspended: UInt64? = nil,
                buried: UInt64? = nil, notetypes: [Int64]? = nil, noteCards: UInt64? = nil) {
        self.mode = mode
        self.rows = rows
        self.cards = cards
        self.notes = notes
        self.suspended = suspended
        self.buried = buried
        self.notetypes = notetypes
        self.noteCards = noteCards
    }

    /// Every count the core gives is in.
    var complete: Bool { cards != nil && notes != nil && suspended != nil && buried != nil && notetypes != nil }
}

/// How one command shows for a target (V8.1).
public nonisolated struct BrowseCommandState: Equatable, Sendable {
    /// V8.1's "Shown when": the context menu leaves the others out; the menu
    /// bar keeps its items and disables them (a menu bar's items stay put).
    public let shown: Bool
    public let enabled: Bool
    /// The number the title names (n, s or b of V8.1), or nil when the title
    /// takes no number or it is not known yet.
    public let count: UInt64?
    /// V8.2: Delete asks first, so its title ends with "…".
    public let asks: Bool

    public init(shown: Bool, enabled: Bool, count: UInt64?, asks: Bool) {
        self.shown = shown
        self.enabled = enabled
        self.count = count
        self.asks = asks
    }

    /// V8.1's rules for `command` on rows whose facts are `facts` (nil: no
    /// row chosen). `available` is false while the rows shown cannot take a
    /// command (a released list, another command running).
    public static func of(_ command: BrowseCommand, facts: BrowseTargetFacts?, available: Bool) -> BrowseCommandState {
        let rows = facts?.rows ?? 0
        let cards = facts?.cards ?? (facts?.mode == .cards && rows > 0 ? UInt64(rows) : nil)
        let suspended = facts?.suspended, buried = facts?.buried, notetypes = facts?.notetypes
        let shown: Bool = switch command {
        case .suspend: cards.flatMap { cards in suspended.map { $0 < cards } } ?? true
        case .unsuspend: suspended.map { $0 > 0 } ?? true
        case .bury: cards.flatMap { cards in buried.map { $0 < cards } } ?? true
        case .unbury: buried.map { $0 > 0 } ?? true
        case .changeNoteType, .editNoteType: notetypes.map { $0.count == 1 } ?? true
        default: true
        }
        var enabled = command.built && available && shown && (rows > 0 || !command.needsSelection)
        // Edit Note shows one row's note in the inspector (U273).
        if command == .editNote, rows != 1 { enabled = false }
        // Edit Note Type… opens one note type: it waits until that one is known.
        if command == .editNoteType, notetypes?.count != 1 { enabled = false }
        let count: UInt64? = switch command {
        case .suspend, .bury: cards
        case .unsuspend: suspended
        case .unbury: buried
        case .delete: facts?.notes ?? (facts?.mode == .notes && rows > 0 ? UInt64(rows) : nil)
        default: nil
        }
        let asks = command == .delete && ((count ?? 0) > 1 || (facts?.noteCards ?? 0) > 1)
        return BrowseCommandState(shown: shown, enabled: enabled, count: count, asks: asks)
    }
}

/// V8.2's question before a Delete: the notes and cards that go, and for
/// one note its Sort Field text. It travels with the rows it was asked
/// for: the delete runs on those rows at that revision, or asks again.
public nonisolated struct BrowseDeleteQuestion: Identifiable, Equatable, Sendable {
    public let id: UInt64
    let target: BrowseTarget
    public let notes: UInt64
    public let cards: UInt64
    /// One note: its Sort Field cell text (V8.2 cuts it).
    public let sortField: String?
}

/// V8.3: what VoiceOver announces after a command, from its `affected`.
public nonisolated struct BrowseAnnouncement: Equatable, Sendable {
    public enum Kind: Sendable, Hashable {
        case suspended, unsuspended, buried, unburied, flagged, unflagged, deleted
        /// U276's sheets: cards moved, notes changed (tags), cards given a
        /// due date, cards forgotten.
        case moved, tagsChanged, rescheduled, forgotten
        /// U277: Find and Replace's notes changed (V9).
        case notesChanged
    }

    public let kind: Kind
    public let count: UInt64
    /// Raised by every announcement, so two equal ones are both made.
    public let serial: UInt64
}

/// Why a command that needs no selection is disabled (its help).
public nonisolated enum BrowseUnavailable: Sendable, Hashable {
    /// The results are not shown yet, or their search went.
    case searching
    /// Another command, sheet or question is open.
    case busy
    /// The collection is not open for writes.
    case notReady
}

/// V8.3's display names of Browse's undo steps, in Apple's style ("Undo
/// Suspend Cards"), as `ReviewStepNames` gives the review's. The title
/// follows the command chosen, singular when its title counted one.
public nonisolated enum BrowseStepNames {
    public static func display(_ command: BrowseCommand, count: UInt64?) -> String? {
        let one = count == 1
        return switch command {
        case .suspend: one ? "Suspend Card" : "Suspend Cards"
        case .unsuspend: one ? "Unsuspend Card" : "Unsuspend Cards"
        case .bury: one ? "Bury Card" : "Bury Cards"
        case .unbury: one ? "Unbury Card" : "Unbury Cards"
        case .flag: "Flag"
        case .delete: one ? "Delete Note" : "Delete Notes"
        case .changeDeck: "Change Deck"
        case .addTags: "Add Tags"
        case .removeTags: "Remove Tags"
        case .setDueDate: "Set Due Date"
        case .forget: "Forget"
        case .changeNoteType: "Change Note Type"
        case .findReplace: "Find and Replace"
        case .editNote: "Edit Note"
        case .editNoteType, .findDuplicates: nil
        }
    }
}

/// Browse's commands (U275; `browser/workspace` V7, V8; N21.5, N21.8): what
/// each shows for the selection or the rows a context menu was opened on,
/// and the commands that need no sheet, run on the real core, each one
/// undo step titled after the command. Held by `BrowseModel`.
@Observable @MainActor
public final class BrowseCommands {
    /// What is known of the selection (nil with no selection).
    public private(set) var facts: BrowseTargetFacts?
    /// A command is running: the others wait.
    public private(set) var busy = false
    /// V8.2: the question shown before a Delete.
    public private(set) var deleteQuestion: BrowseDeleteQuestion?
    /// V8.3: the last result VoiceOver announces.
    public private(set) var announcement: BrowseAnnouncement?
    /// A command that was refused or failed: nothing was written (K4).
    public private(set) var failed: BrowseCommand?
    /// U276: the command sheet open, on the rows it opened on.
    public private(set) var sheet: BrowseSheet?
    /// U277: Find and Replace… and Find Duplicates…, on the rows they opened on.
    public private(set) var findReplaceSheet: BrowseFindReplaceSheet?
    public private(set) var findDuplicatesSheet: BrowseFindDuplicatesSheet?

    @ObservationIgnored let results: BrowseResults
    /// The sheets' own reads (U276).
    @ObservationIgnored let sheetReads: any BrowseSheetReads
    /// What Set Due Date and Forget prefill (B10), read ahead so a sheet
    /// opens filled; read again after each of them runs.
    @ObservationIgnored private var sheetDefaults: BrowseSheetDefaults?
    @ObservationIgnored private var factsTarget: BrowseTarget?
    /// The rows the core's counts in `facts` were last read for.
    @ObservationIgnored private(set) var factsRead: BrowseTarget?
    /// The read under way, and the rows it reads.
    @ObservationIgnored private var factsTask: (target: BrowseTarget, task: Task<BrowseTargetFacts?, Never>)?
    @ObservationIgnored private var serial: UInt64 = 0
    @ObservationIgnored private var reading = false
    /// Raised when the collection goes: a command started before lands nothing.
    @ObservationIgnored private(set) var generation: UInt64 = 0
    @ObservationIgnored private var readAgain = false
    /// A command recorded its undo step: its seq (the step's id) and the
    /// title Edit › Undo shows for it (V8.3).
    @ObservationIgnored public var stepRecorded: @MainActor (UInt64, String) -> Void = { _, _ in }
    /// Edit Note Type… opens Note Types on that type (W14).
    @ObservationIgnored public var openNoteType: @MainActor (Int64) -> Void = { _ in }
    /// Edit Note: the inspector shows and its first field takes the focus (U273).
    @ObservationIgnored var editNote: @MainActor () -> Void = {}
    /// The collection is ready for writes.
    @ObservationIgnored public var ready: @MainActor () -> Bool = { true }
    /// U277: what Find and Replace remembers on the device (F2), and its write.
    @ObservationIgnored var findReplaceMemory: @MainActor () -> BrowseFindReplaceDevice? = { nil }
    @ObservationIgnored var keepFindReplaceMemory: @MainActor (BrowseFindReplaceDevice) -> Void = { _ in }
    /// The note types in A2's order (the field menus' groups).
    @ObservationIgnored var notetypes: @MainActor () -> [NotetypeName] = { [] }
    /// V10: Find Duplicates found groups: the query becomes their token.
    @ObservationIgnored var duplicatesFound: @MainActor (FindDuplicatesResult, String) -> Void = { _, _ in }
    /// F2: the last Replace's texts in this launch.
    @ObservationIgnored private var lastReplace: (find: String, replace: String)?

    init(results: BrowseResults, sheetReads: any BrowseSheetReads) {
        self.results = results
        self.sheetReads = sheetReads
    }

    // MARK: What each command shows

    /// V8.1 for `command` on the selection, or on `rows` (a context menu
    /// opened on rows that are not the selection).
    public func state(_ command: BrowseCommand, rows: Set<Int>? = nil) -> BrowseCommandState {
        guard let rows else { return BrowseCommandState.of(command, facts: facts, available: available(nil)) }
        return state(command, on: results.target(rows))
    }

    /// The rows a context menu was opened on, as its commands take them: read
    /// once, when the menu is drawn, so a list that moves on while the menu is
    /// open makes the core refuse them (K1) instead of acting on other rows.
    public func target(rows: Set<Int>) -> BrowseTarget? { results.target(rows) }

    /// V8.1 for `command` on `target` (a context menu's rows).
    public func state(_ command: BrowseCommand, on target: BrowseTarget?) -> BrowseCommandState {
        guard let target else { return BrowseCommandState.of(command, facts: nil, available: available(nil)) }
        let facts = target == results.target() ? self.facts : localFacts(target)
        return BrowseCommandState.of(command, facts: facts, available: available(nil))
    }

    /// The rows can take a command: a list that is not released, no other
    /// command running, no question open.
    private func available(_ rows: Set<Int>?) -> Bool {
        guard let list = results.list, !list.released, !results.isDimmed else { return false }
        return !busy && deleteQuestion == nil && sheet == nil && findReplaceSheet == nil && findDuplicatesSheet == nil && ready()
    }

    /// Why the rows cannot take a command now, for the help of a disabled
    /// Find and Replace… or Find Duplicates…, which need no selection.
    public var unavailable: BrowseUnavailable? {
        guard let list = results.list, !list.released, !results.isDimmed else { return .searching }
        if busy || deleteQuestion != nil || sheet != nil || findReplaceSheet != nil || findDuplicatesSheet != nil { return .busy }
        return ready() ? nil : .notReady
    }

    /// The selection or the list changed: its facts are read again, the
    /// rows on screen first, then the core's counts.
    func selectionChanged() { readFacts(keepShown: false) }

    /// `keepShown`: the facts shown stay until the core's arrive (after a
    /// command, whose rows on screen are read again only later).
    private func readFacts(keepShown: Bool) {
        guard let target = results.target() else {
            factsTarget = nil
            if facts != nil { facts = nil }
            return
        }
        guard target != factsTarget else { return }
        factsTarget = target
        let local = localFacts(target)
        if !keepShown || facts?.rows != local.rows || facts?.mode != local.mode, facts != local { facts = local }
        startRead()
    }

    /// One read of the core's counts at a time: a newer counts call of the
    /// session cancels the older one in the core, so two at once could
    /// cancel each other and leave neither. A change meanwhile reads again
    /// once the read under way ends.
    private func startRead() {
        guard !reading else {
            readAgain = true
            return
        }
        guard let target = factsTarget, let session = results.commandSession else { return }
        reading = true
        readAgain = false
        let local = localFacts(target)
        let task = Task<BrowseTargetFacts?, Never> { [weak self] in
            await self?.read(target, local: local, session: session)
        }
        factsTask = (target, task)
        Task {
            let read = await task.value
            reading = false
            if let read, factsTarget == target {
                if facts != read { facts = read }
                factsRead = target
            }
            if readAgain || factsTarget != target { startRead() }
        }
    }

    /// The collection changed: the counts of the same rows are read again
    /// (a suspension another window or a sync made).
    func rowsChanged() {
        factsTarget = nil
        readFacts(keepShown: true)
    }

    /// What the rows on screen tell of `target`: in Cards mode, with every
    /// row read, its cards, notes, suspended and buried rows.
    private func localFacts(_ target: BrowseTarget) -> BrowseTargetFacts {
        var facts = BrowseTargetFacts(mode: target.mode, rows: target.rows)
        guard target.mode == .cards, target.rows <= 2_000 else { return facts }
        var suspended: UInt64 = 0, buried: UInt64 = 0, notes = Set<Int64>()
        for range in target.ranges {
            for index in range {
                guard let row = results.row(at: Int(index)), row.status == .row, let state = row.state,
                      let note = row.noteID else { return facts }
                if state.suspended { suspended += 1 }
                if state.buried { buried += 1 }
                notes.insert(note)
            }
        }
        facts.cards = UInt64(target.rows)
        facts.suspended = suspended
        facts.buried = buried
        facts.notes = UInt64(notes.count)
        return facts
    }

    /// The core's counts of `target` (V7, V8.1), and for one target note
    /// the cards a Delete removes.
    private func read(_ target: BrowseTarget, local: BrowseTargetFacts, session: any BrowseSearching) async
        -> BrowseTargetFacts? {
        guard let counts = try? await session.counts(of: target) else { return nil }
        var facts = local
        facts.cards = counts.cards
        facts.notes = counts.notes
        facts.suspended = counts.suspended
        facts.buried = counts.buried
        facts.notetypes = counts.notetypes
        facts.decks = counts.decks
        facts.deck = counts.deck
        facts.deckIDs = counts.deckIDs
        if counts.notes == 1 {
            facts.noteCards = target.mode == .notes ? counts.cards : try? await session.cardsOfNotes(of: target)
        }
        return facts
    }

    // MARK: Running

    /// Runs `command` on the selection, or on `rows`. A command whose sheet
    /// is not built does nothing.
    public func perform(_ command: BrowseCommand, rows: Set<Int>? = nil) {
        guard let rows else { return run(command, menu: nil) }
        guard let target = results.target(rows) else { return }
        run(command, menu: target)
    }

    /// Runs `command` on the rows a context menu was opened on, as they were
    /// when it was drawn.
    public func perform(_ command: BrowseCommand, on target: BrowseTarget) {
        run(command, menu: target)
    }

    /// The inspector saves its note before a command runs (W2), so a
    /// command on the edited row (Delete included) never drops the edit.
    @ObservationIgnored var saveFirst: (@MainActor () async -> Bool)?
    /// The inspector holds an edit not saved yet.
    @ObservationIgnored var unsavedEdit: @MainActor () -> Bool = { false }

    /// `menu` nil: the selection now (the menu bar's commands).
    private func run(_ command: BrowseCommand, menu: BrowseTarget?) {
        let state = menu.map { state(command, on: $0) } ?? state(command)
        guard state.enabled else { return }
        if command != .editNote, command != .editNoteType, unsavedEdit(), let saveFirst {
            // The rows as they are now: a list that moves on during the save
            // makes the core refuse them (K1); a failed save runs nothing.
            let target = menu ?? results.target()
            Task {
                guard await saveFirst(), let target else { return }
                runNow(command, menu: target)
            }
            return
        }
        runNow(command, menu: menu)
    }

    private func runNow(_ command: BrowseCommand, menu: BrowseTarget?) {
        let state = menu.map { state(command, on: $0) } ?? state(command)
        guard state.enabled else { return }
        switch command {
        case .flag(let flag):
            runCards(.setFlag(flag), command: command, count: nil, menu: menu)
        case .suspend: runCards(.suspend, command: command, count: state.count, menu: menu)
        case .unsuspend: runCards(.unsuspend, command: command, count: state.count, menu: menu)
        case .bury: runCards(.bury, command: command, count: state.count, menu: menu)
        case .unbury: runCards(.unbury, command: command, count: state.count, menu: menu)
        case .delete: delete(menu: menu)
        case .editNoteType:
            if let id = facts?.notetypes?.first { openNoteType(id) }
        case .editNote:
            editNote()
        case .changeDeck, .addTags, .removeTags, .setDueDate, .forget:
            guard let kind = BrowseSheetKind(command), let target = menu ?? results.target() else { return }
            openSheet(kind, on: target)
        case .findReplace:
            openFindReplace(selected: menu ?? results.target())
        case .findDuplicates:
            openFindDuplicates()
        default:
            return
        }
    }

    /// V8.1's card commands: one bulk action on the target cards.
    private func runCards(_ action: BulkCardAction, command: BrowseCommand, count: UInt64?, menu: BrowseTarget?) {
        guard let target = menu ?? results.target(), let session = results.commandSession else { return }
        busy = true
        let picks = results.userSelections, generation = generation
        Task {
            defer { if self.generation == generation { busy = false } }
            var count = count
            // Unsuspend and Unbury on rows whose counts the menu did not know
            // yet (a context menu's rows outside the selection): with none
            // suspended or buried there is nothing to do, and the core would
            // refuse the empty list.
            if action == .unsuspend || action == .unbury, count == nil {
                guard let counts = try? await session.counts(of: target) else { return fail(command, generation) }
                let left = action == .unsuspend ? counts.suspended : counts.buried
                guard left > 0 else { return }
                count = left
            }
            do {
                let result = try await run(action, on: target, session: session, again: menu == nil ? picks : nil)
                guard self.generation == generation else { return }
                finished(command, count: count, result: result)
            } catch {
                fail(command, generation)
            }
        }
    }

    /// The inspector's Deck row (V12): Change Deck on `target`, the rows it
    /// was chosen for (the shown card, or every card of the note in Notes
    /// mode, B1), one "Change Deck" step.
    func changeDeck(to deck: Int64, on target: BrowseTarget) {
        guard available(nil) else { return }
        runCards(.changeDeck(deck: deck), command: .changeDeck, count: nil, menu: target)
    }

    private func fail(_ command: BrowseCommand, _ generation: UInt64) {
        guard self.generation == generation else { return }
        failed = command
    }

    /// Runs `action` on `target`. When the core refuses it because the list
    /// moved on (a change pruned rows meanwhile, so the revision is not the
    /// one the rows were taken at) and the user chose no other rows, the
    /// selection's rows are taken again in the new revision (the selection
    /// moved with the prune) and run once more. A context menu's rows are
    /// not taken again: nothing tells which rows they became.
    private func run(_ action: BulkCardAction, on target: BrowseTarget, session: any BrowseSearching,
                     again picks: UInt64?) async throws -> BulkActionResult {
        do { return try await session.run(action, on: target) } catch {
            guard let picks, picks == results.userSelections, let next = results.target(), next.handle == target.handle,
                  next.revision != target.revision, next.rows == target.rows else { throw error }
            return try await session.run(action, on: next)
        }
    }

    private func finished(_ command: BrowseCommand, count: UInt64?, result: BulkActionResult) {
        if result.summary.undoName != nil, let title = BrowseStepNames.display(command, count: count) {
            stepRecorded(result.summary.seq, title)
        }
        let kind: BrowseAnnouncement.Kind? = switch command {
        case .suspend: .suspended
        case .unsuspend: .unsuspended
        case .bury: .buried
        case .unbury: .unburied
        case .flag(0): .unflagged
        case .flag: .flagged
        case .delete: .deleted
        case .changeDeck: .moved
        case .addTags, .removeTags: .tagsChanged
        case .setDueDate: .rescheduled
        case .forget: .forgotten
        default: nil
        }
        guard let kind else { return }
        serial &+= 1
        announcement = BrowseAnnouncement(kind: kind, count: result.affected, serial: serial)
        // The counts of the same rows changed with them.
        rowsChanged()
    }

    // MARK: Delete (V8.2)

    /// Delete: asks when more than one note goes, or when the one note has
    /// other cards that go with it; otherwise deletes at once (Undo
    /// restores it). The rule reads the core's counts afresh: counts kept
    /// from before a change (a card added to the note) must not skip it.
    private func delete(menu: BrowseTarget?) {
        guard let target = menu ?? results.target(), let session = results.commandSession else { return }
        busy = true
        let generation = generation
        Task {
            let question = await ask(target, session: session, generation: generation)
            guard self.generation == generation else { return }
            busy = false
            if let question { deleteQuestion = question }
        }
    }

    /// The question for `target`, or nil when it was deleted at once (one
    /// note with one card) or could not be counted.
    private func ask(_ target: BrowseTarget, session: any BrowseSearching, generation: UInt64) async -> BrowseDeleteQuestion? {
        // The selection's read under way first: two counts reads of a session
        // at once cancel the older one in the core.
        _ = await factsTask?.task.value
        var counts = try? await session.counts(of: target)
        if counts == nil { counts = try? await session.counts(of: target) }
        guard let counts, counts.notes > 0, let cards = try? await session.cardsOfNotes(of: target) else {
            fail(.delete, generation)
            return nil
        }
        if counts.notes == 1, cards <= 1 {
            await deleteNow(target, notes: 1, session: session, generation: generation)
            return nil
        }
        let sortField = counts.notes == 1 ? await results.sortFieldText(at: Int(target.ranges[0].lowerBound), in: target) : nil
        serial &+= 1
        return BrowseDeleteQuestion(id: serial, target: target, notes: counts.notes, cards: cards, sortField: sortField)
    }

    /// The question's Delete: the notes of the rows it asked about, at the
    /// revision it asked at. When the list moved on meanwhile the core
    /// refuses them (K1): nothing is deleted and the failure says so.
    public func confirmDelete(_ question: BrowseDeleteQuestion) {
        guard deleteQuestion == question, let session = results.commandSession else { return }
        deleteQuestion = nil
        busy = true
        let generation = generation
        Task {
            await deleteNow(question.target, notes: question.notes, session: session, generation: generation)
            if self.generation == generation { busy = false }
        }
    }

    /// The question's Cancel: nothing changes.
    public func cancelDelete() {
        deleteQuestion = nil
    }

    private func deleteNow(_ target: BrowseTarget, notes: UInt64, session: any BrowseSearching, generation: UInt64) async {
        do {
            let result = try await session.run(.deleteNotes, on: target)
            guard self.generation == generation else { return }
            finished(.delete, count: notes, result: result)
        } catch {
            fail(.delete, generation)
        }
    }

    /// The failure's alert was dismissed.
    public func dismissFailure() { failed = nil }

    // MARK: Selection commands (V7)

    /// Select Sibling Cards (Cards mode): every card of the selected cards'
    /// notes that the results hold.
    public var canSelectSiblings: Bool {
        results.list?.mode == .cards && results.target() != nil && !busy
    }

    public func selectSiblings() {
        guard canSelectSiblings, let target = results.target(), let session = results.commandSession,
              let token = results.list?.token else { return }
        let picks = results.userSelections
        Task {
            guard let rows = try? await session.siblingRows(of: target), picks == results.userSelections else { return }
            results.select(rows, token: token, revision: target.revision)
        }
    }

    public var canSelectAll: Bool { (results.list?.count ?? 0) > 0 }

    public func selectAll() { results.selectAll() }

    // MARK: Sheets (U276)

    /// The sheet of `kind` on `target`, the rows as they are now: its
    /// command acts on exactly these, at this revision.
    private func openSheet(_ kind: BrowseSheetKind, on target: BrowseTarget) {
        serial &+= 1
        let sheet = BrowseSheet(id: serial, kind: kind, target: target, commands: self, reads: sheetReads,
                                defaults: sheetDefaults)
        self.sheet = sheet
        // The selection's counts when they are the ones read for these rows.
        let known: BrowseSelectionCounts? = if target == factsRead, let facts, facts.complete, let cards = facts.cards,
            let notes = facts.notes, let suspended = facts.suspended, let buried = facts.buried,
            let notetypes = facts.notetypes {
            BrowseSelectionCounts(cards: cards, notes: notes, suspended: suspended, buried: buried, decks: facts.decks ?? 0,
                                  deck: facts.deck, deckIDs: facts.deckIDs ?? [], notetypes: notetypes)
        } else { nil }
        sheet.load(session: results.commandSession, facts: known, waitFirst: factsTask?.task, defaultsKnown: sheetDefaults != nil)
        if sheetDefaults == nil, kind == .setDueDate || kind == .forget {
            let generation = generation
            Task { [weak sheet] in
                let read = try? await sheetReads.sheetDefaults()
                guard self.generation == generation else { return }
                if let read { sheetDefaults = read }
                // A failed read fills the keys' own defaults (`sched/set-due-date` S6,
                // `sched/manual-ops` M6.4), so the sheet never waits for nothing.
                sheet?.prefill(read ?? BrowseSheetDefaults(setDueDays: "0", restorePosition: true, resetCounts: false))
            }
        }
    }

    /// Reads ahead what Set Due Date and Forget prefill, so their sheets
    /// open filled (a pure read).
    func preloadSheetDefaults() {
        let generation = generation
        Task {
            guard let defaults = try? await sheetReads.sheetDefaults(), self.generation == generation else { return }
            sheetDefaults = defaults
        }
    }

    /// The sheet's Cancel, Esc, or the end of its command.
    func closeSheet(_ sheet: BrowseSheet) {
        sheet.closed()
        guard self.sheet === sheet else { return }
        self.sheet = nil
        // The sheet's counts read may have cancelled the selection's: read them again.
        rowsChanged()
    }

    /// The sheet closed by the system (a window closing).
    public func dismissSheet() {
        guard let sheet else { return }
        if sheet.isRunning { sheet.cancel() } else { closeSheet(sheet) }
    }

    /// Runs the sheet's command on its rows. No second try on other rows:
    /// when the list moved on the core refuses the rows and the sheet says
    /// so, nothing written.
    func runSheet(_ sheet: BrowseSheet, _ action: BrowseSheet.Either) {
        guard let session = results.commandSession else { return sheet.failed(.failed) }
        let generation = generation, target = sheet.target, command = sheet.kind.command
        // A list that already moved on is refused here, as the core would.
        if let list = results.list, list.handle != target.handle || list.revision != target.revision {
            return sheet.failed(.rowsChanged)
        }
        Task {
            do {
                let running = switch action {
                case .card(let action): try await session.start(action, on: target)
                case .note(let action): try await session.start(action, on: target)
                }
                guard self.generation == generation else {
                    // The collection went while the action started: it must not commit unseen.
                    await running.cancel()
                    return
                }
                sheet.started(running)
                let progress = Task { @MainActor in
                    for await report in running.progress { sheet.progressed(report) }
                }
                defer { progress.cancel() }
                let result = try await running.result()
                guard self.generation == generation else { return }
                closeSheet(sheet)
                finished(command, count: nil, result: result)
                if command == .setDueDate || command == .forget { preloadSheetDefaults() }
            } catch {
                guard self.generation == generation else { return }
                if let failure = BrowseSheet.failure(error) {
                    sheet.failed(failure)
                } else {
                    // Cancelled: nothing was written.
                    closeSheet(sheet)
                }
            }
        }
    }

    // MARK: Find and Replace, Find Duplicates (U277)

    /// V9: the sheet on the selected rows (Selected) and the whole results
    /// (All Found), as they are now; it opens on Selected when there is a
    /// selection.
    private func openFindReplace(selected: BrowseTarget?) {
        guard let session = results.commandSession else { return }
        serial &+= 1
        let memory = findReplaceMemory()
        let sheet = BrowseFindReplaceSheet(id: serial, selected: selected, commands: self, reads: sheetReads, memory: memory,
                                           prefill: lastReplace, typeOrder: notetypes().map(\.id))
        findReplaceSheet = sheet
        sheet.load(session: session, results: results, history: memory, waitFirst: factsTask?.task)
    }

    /// F2: Replace enters the texts in the histories and keeps the options
    /// and the In choice, before the run (a refused run enters them too).
    func rememberFindReplace(request: FindReplaceRequest, entry: BrowseFieldEntry?, sheet: BrowseFindReplaceSheet) {
        var memory = findReplaceMemory() ?? BrowseFindReplaceDevice()
        memory.matchCase = !request.ignoreCase
        memory.wholeWords = request.wholeWords
        memory.regex = request.regex
        memory.inEntry = entry?.kind.memory
        memory.inPosition = entry?.id
        memory.finds = BrowseFindReplaceDevice.entering(request.find, into: memory.finds)
        memory.replaces = BrowseFindReplaceDevice.entering(request.replace, into: memory.replaces)
        keepFindReplaceMemory(memory)
        lastReplace = (request.find, request.replace)
    }

    /// The run on the sheet's target: one "Find and Replace" step, the rows
    /// updated in place. A list that moved on is refused, nothing written;
    /// a count larger than the preview's is refused and shown again (F11).
    func runFindReplace(_ sheet: BrowseFindReplaceSheet, request: FindReplaceRequest, target: BrowseTarget, confirmed: UInt64) {
        guard let session = results.commandSession else { return sheet.failed(.failed) }
        let generation = generation
        // A list that already moved on is refused here, as the core would.
        if let list = results.list, list.handle != target.handle || list.revision != target.revision || list.mode != target.mode {
            return sheet.failed(.rowsChanged)
        }
        Task {
            do {
                // A Cancel before the job exists: nothing is started.
                if sheet.phase == .cancelling { return closeFindReplace(sheet) }
                let running = try await session.startFindReplace(request, on: target, confirmed: confirmed)
                guard self.generation == generation else {
                    await running.cancel()
                    return
                }
                sheet.started(running)
                let progress = Task { @MainActor in
                    for await report in running.progress { sheet.progressed(report) }
                }
                defer { progress.cancel() }
                let result = try await running.result()
                guard self.generation == generation else { return }
                closeFindReplace(sheet)
                // V8.3: the command's own title, also for In Tags (whose step the core names Update Tag).
                if result.summary.undoName != nil, let title = BrowseStepNames.display(.findReplace, count: nil) {
                    stepRecorded(result.summary.seq, title)
                }
                serial &+= 1
                announcement = BrowseAnnouncement(kind: .notesChanged, count: result.affected, serial: serial)
            } catch {
                guard self.generation == generation else { return }
                if let failure = BrowseFindReplaceSheet.failure(error) {
                    sheet.failed(failure)
                } else {
                    // Cancelled: nothing was written.
                    closeFindReplace(sheet)
                }
            }
        }
    }

    func closeFindReplace(_ sheet: BrowseFindReplaceSheet) {
        sheet.closed()
        guard findReplaceSheet === sheet else { return }
        findReplaceSheet = nil
        rowsChanged()
    }

    /// The sheet closed by the system (a window closing).
    public func dismissFindReplace() {
        guard let findReplaceSheet else { return }
        if findReplaceSheet.isRunning { findReplaceSheet.cancel() } else { closeFindReplace(findReplaceSheet) }
    }

    /// V10: the sheet, its field menu starting on the Sort Field of the
    /// current row's note type (else the first row's), All Found on the
    /// whole results.
    private func openFindDuplicates() {
        guard let session = results.commandSession else { return }
        serial &+= 1
        let start = results.currentIndex.flatMap { results.target([$0]) } ?? results.target([0])
        let sheet = BrowseFindDuplicatesSheet(id: serial, startRow: start, commands: self, reads: sheetReads,
                                              notetypes: notetypes())
        findDuplicatesSheet = sheet
        sheet.load(session: session, results: results, waitFirst: factsTask?.task)
    }

    /// Find: the groups replace the query (V10); none keeps the sheet,
    /// which says so. It writes nothing.
    func runFindDuplicates(_ sheet: BrowseFindDuplicatesSheet, field: String, target: BrowseTarget?) {
        guard let session = results.commandSession else { return sheet.failed(.failed) }
        let generation = generation
        if let target, let list = results.list, list.handle != target.handle || list.revision != target.revision {
            return sheet.failed(.rowsChanged)
        }
        Task {
            do {
                let running = try await session.startFindDuplicates(field: field, on: target)
                guard self.generation == generation else {
                    await running.cancel()
                    return
                }
                sheet.started(running)
                let progress = Task { @MainActor in
                    for await report in running.progress { sheet.progressed(report) }
                }
                defer { progress.cancel() }
                let result = try await running.result()
                guard self.generation == generation, findDuplicatesSheet === sheet else { return }
                // Cancel was asked for: what the read found does not land.
                if sheet.phase == .cancelling { return closeFindDuplicates(sheet) }
                guard !result.groups.isEmpty else { return sheet.finished(result) }
                closeFindDuplicates(sheet)
                duplicatesFound(result, field)
            } catch {
                guard self.generation == generation else { return }
                if let failure = BrowseSheet.failure(error) {
                    sheet.failed(failure)
                } else {
                    // Cancelled: the sheet closes, as the other sheets' Cancel does.
                    closeFindDuplicates(sheet)
                }
            }
        }
    }

    func closeFindDuplicates(_ sheet: BrowseFindDuplicatesSheet) {
        sheet.closed()
        guard findDuplicatesSheet === sheet else { return }
        findDuplicatesSheet = nil
        rowsChanged()
    }

    public func dismissFindDuplicates() {
        guard let findDuplicatesSheet else { return }
        if findDuplicatesSheet.isRunning { findDuplicatesSheet.cancel() } else { closeFindDuplicates(findDuplicatesSheet) }
    }

    /// The collection went: nothing started before lands.
    func reset() {
        generation &+= 1
        busy = false
        factsTarget = nil
        factsRead = nil
        facts = nil
        deleteQuestion = nil
        failed = nil
        // A command under way is asked to stop; its result lands nowhere.
        if let sheet, sheet.isRunning { sheet.cancel() }
        sheet?.closed()
        sheet = nil
        if let findReplaceSheet, findReplaceSheet.isRunning { findReplaceSheet.cancel() }
        findReplaceSheet?.closed()
        findReplaceSheet = nil
        findDuplicatesSheet?.closed()
        findDuplicatesSheet = nil
        sheetDefaults = nil
    }
}
