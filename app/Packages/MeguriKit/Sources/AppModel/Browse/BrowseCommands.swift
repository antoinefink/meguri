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

    /// The commands that run now (U275). The others open a sheet or a pane
    /// that a later unit builds (Change Deck, the tags, Set Due Date and
    /// Forget: U276; Find and Replace and Find Duplicates: U277; Change Note
    /// Type: U284; Edit Note: the inspector, U273): until then they are in
    /// their places, disabled.
    public var built: Bool {
        switch self {
        case .flag, .suspend, .unsuspend, .bury, .unbury, .delete, .editNoteType: true
        case .changeDeck, .addTags, .removeTags, .setDueDate, .forget, .changeNoteType, .findReplace, .findDuplicates,
             .editNote:
            false
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
    }

    public let kind: Kind
    public let count: UInt64
    /// Raised by every announcement, so two equal ones are both made.
    public let serial: UInt64
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

    @ObservationIgnored private let results: BrowseResults
    @ObservationIgnored private var factsTarget: BrowseTarget?
    /// The read under way, and the rows it reads.
    @ObservationIgnored private var factsTask: (target: BrowseTarget, task: Task<BrowseTargetFacts?, Never>)?
    @ObservationIgnored private var serial: UInt64 = 0
    @ObservationIgnored private var reading = false
    @ObservationIgnored private var readAgain = false
    /// A command recorded its undo step: its seq (the step's id) and the
    /// title Edit › Undo shows for it (V8.3).
    @ObservationIgnored public var stepRecorded: @MainActor (UInt64, String) -> Void = { _, _ in }
    /// Edit Note Type… opens Note Types on that type (W14).
    @ObservationIgnored public var openNoteType: @MainActor (Int64) -> Void = { _ in }
    /// The collection is ready for writes.
    @ObservationIgnored public var ready: @MainActor () -> Bool = { true }

    init(results: BrowseResults) {
        self.results = results
    }

    // MARK: What each command shows

    /// V8.1 for `command` on the selection, or on `rows` (a context menu
    /// opened on rows that are not the selection).
    public func state(_ command: BrowseCommand, rows: Set<Int>? = nil) -> BrowseCommandState {
        let facts: BrowseTargetFacts?
        if let rows, rows != results.selection {
            facts = results.target(rows).map { localFacts($0) }
        } else {
            facts = self.facts
        }
        return BrowseCommandState.of(command, facts: facts, available: available(rows))
    }

    /// The rows can take a command: a list that is not released, no other
    /// command running, no question open.
    private func available(_ rows: Set<Int>?) -> Bool {
        guard let list = results.list, !list.released, !results.isDimmed else { return false }
        return !busy && deleteQuestion == nil && ready()
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
            if let read, factsTarget == target, facts != read { facts = read }
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
        if counts.notes == 1 {
            facts.noteCards = target.mode == .notes ? counts.cards : try? await session.cardsOfNotes(of: target)
        }
        return facts
    }

    /// The facts of `target`, waiting for the read under way when it is the
    /// selection's.
    private func facts(of target: BrowseTarget, session: any BrowseSearching) async -> BrowseTargetFacts? {
        if target == factsTarget, let facts, facts.complete { return facts }
        if let factsTask, factsTask.target == target, let read = await factsTask.task.value { return read }
        return await read(target, local: localFacts(target), session: session)
    }

    // MARK: Running

    /// Runs `command` on the selection, or on `rows` (a context menu's).
    /// A command whose sheet is not built does nothing.
    public func perform(_ command: BrowseCommand, rows: Set<Int>? = nil) {
        let state = state(command, rows: rows)
        guard state.enabled else { return }
        switch command {
        case .flag(let flag):
            runCards(.setFlag(flag), command: command, count: nil, rows: rows)
        case .suspend: runCards(.suspend, command: command, count: state.count, rows: rows)
        case .unsuspend: runCards(.unsuspend, command: command, count: state.count, rows: rows)
        case .bury: runCards(.bury, command: command, count: state.count, rows: rows)
        case .unbury: runCards(.unbury, command: command, count: state.count, rows: rows)
        case .delete: delete(rows: rows)
        case .editNoteType:
            if let id = facts?.notetypes?.first { openNoteType(id) }
        default:
            return
        }
    }

    /// V8.1's card commands: one bulk action on the target cards.
    private func runCards(_ action: BulkCardAction, command: BrowseCommand, count: UInt64?, rows: Set<Int>?) {
        guard let target = results.target(rows), let session = results.commandSession else { return }
        busy = true
        let picks = results.userSelections
        Task {
            defer { busy = false }
            do {
                let result = try await run(action, on: target, session: session, again: rows == nil ? picks : nil)
                finished(command, count: count, result: result)
            } catch {
                failed = command
            }
        }
    }

    /// Runs `action` on `target`. When the core refuses it because the list
    /// moved on (a change pruned rows meanwhile, so the revision is not the
    /// one the rows were taken at) and the user chose no other rows, the
    /// same rows are taken again in the new revision (the selection moved
    /// with the prune) and run once more.
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
    /// restores it).
    private func delete(rows: Set<Int>?) {
        guard let target = results.target(rows), let session = results.commandSession else { return }
        busy = true
        Task {
            let question = await ask(target, session: session)
            busy = false
            if let question { deleteQuestion = question }
        }
    }

    /// The question for `target`, or nil when it was deleted at once (one
    /// note with one card) or could not be counted.
    private func ask(_ target: BrowseTarget, session: any BrowseSearching) async -> BrowseDeleteQuestion? {
        guard let facts = await facts(of: target, session: session), let notes = facts.notes, notes > 0 else {
            failed = .delete
            return nil
        }
        let cards: UInt64
        if let known = facts.noteCards { cards = known } else {
            guard let read = try? await session.cardsOfNotes(of: target) else {
                failed = .delete
                return nil
            }
            cards = read
        }
        if notes == 1, cards <= 1 {
            _ = await deleteNow(target, notes: notes, session: session, asked: false)
            return nil
        }
        let sortField = notes == 1 ? await results.sortFieldText(at: Int(target.ranges[0].lowerBound)) : nil
        serial &+= 1
        return BrowseDeleteQuestion(id: serial, target: target, notes: notes, cards: cards, sortField: sortField)
    }

    /// The question's Delete: the notes of the rows it asked about, at the
    /// revision it asked at. Rows that moved meanwhile are asked about again.
    public func confirmDelete(_ question: BrowseDeleteQuestion) {
        guard deleteQuestion == question, let session = results.commandSession else { return }
        deleteQuestion = nil
        busy = true
        Task {
            let again = await deleteNow(question.target, notes: question.notes, session: session, asked: true)
            busy = false
            if again { delete(rows: nil) }
        }
    }

    /// The question's Cancel: nothing changes.
    public func cancelDelete() {
        deleteQuestion = nil
    }

    /// Deletes the notes of `target`; true when the rows moved on since they
    /// were counted (K1 refuses them) after a question, so Delete counts the
    /// rows chosen now and asks again.
    private func deleteNow(_ target: BrowseTarget, notes: UInt64, session: any BrowseSearching, asked: Bool) async -> Bool {
        do {
            let result = try await session.run(.deleteNotes, on: target)
            finished(.delete, count: notes, result: result)
            return false
        } catch {
            if asked, results.target() != nil, results.list?.revision != target.revision { return true }
            failed = .delete
            return false
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

    /// The collection went: nothing started before lands.
    func reset() {
        factsTarget = nil
        facts = nil
        deleteQuestion = nil
        failed = nil
    }
}
