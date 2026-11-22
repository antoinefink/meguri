import CoreBridge
import Foundation
import Observation

/// The Note Types section (`notetypes/workspace` W2 to W8, W13;
/// `notetypes/draft-save` D1 to D8; layout N22.1 to N22.3, N22.5): the
/// list with its counts and filter, one draft per note type that survives
/// leaving the section, the Fields and Cards tabs' edits with the editor's
/// own undo, the Cards tab's live issues, and Save with its one sheet, alone
/// (D5) or as Meguri Sync's structure save job (D6, `sync/meguri-provider`
/// MP7). Held by `ReviewCollection` above the layout switch. Nothing here
/// writes before Save.
@Observable @MainActor
public final class NoteTypesModel {
    public nonisolated enum Tab: String, Sendable, Hashable, CaseIterable { case fields, cards }
    /// `noCollection`: there is no collection to read (first run); nothing
    /// failed, and nothing is offered to try again.
    public nonisolated enum Phase: Sendable, Hashable { case idle, loading, ready, failed, noCollection }

    /// One row of the list (W2).
    public nonisolated struct Item: Identifiable, Sendable, Hashable {
        public let id: Int64
        public let name: String
        /// Blank until the counts are read.
        public var notes: UInt64?
    }

    /// The Save sheet's question (D4), with the summary it shows.
    public nonisolated struct Question: Sendable, Hashable {
        public let notetypeId: Int64
        public let name: String
        public let summary: NotetypeDraftSummary
        public let decision: NoteTypeSaveDecision
        public var rows: [NoteTypeSaveRow] { NoteTypeSaveRow.rows(summary) }
    }

    /// A running save (D5 step 5): "Updating <i> of <n> notes"; with Meguri
    /// Sync, the job's stage (D6).
    public nonisolated struct Progress: Sendable, Hashable {
        public let notetypeId: Int64
        public var completed: UInt64 = 0
        public var total: UInt64?
        /// Nil for a save on this device alone.
        public var stage: NoteTypeSyncStage?
    }

    public nonisolated enum Failure: Sendable, Hashable {
        /// The save wrote nothing (D5 step 6).
        case notSaved
        /// Meguri Sync stopped before the save (D6 step 1): nothing was
        /// changed, and the sync status says why.
        case syncStopped
        /// Meguri Sync's job ended with an error and whether it saved could
        /// not be read: the draft stays, to be checked before saving again.
        case unconfirmed
    }

    /// Said once a save has ended, when the person must know more than "saved".
    public nonisolated enum Notice: Sendable, Hashable {
        /// Save and Upload saved on this device, but the server does not hold
        /// the changes yet (D6 step 4): the sync status shows what to do.
        case notUploaded
    }

    /// A refusal of a typed field name (V5), shown under the row or field.
    public nonisolated struct RenameRefusal: Sendable, Hashable {
        public let row: Int
        public let refusal: NotetypeFieldNameRefusal
    }

    // MARK: State

    public private(set) var phase: Phase = .idle
    public private(set) var items: [Item] = []
    /// W2's filter: a simple case-folded substring of the name.
    public var filter = ""
    public var selection: Int64? {
        didSet { if selection != oldValue { selectionChanged() } }
    }
    /// The note type the Mac's detail shows: the selection once its first
    /// complete render can draw (its record read, or its read failed). Until
    /// then the previous note type stays on screen, so a selection change
    /// never shows an empty page (nothing flashes); the title follows the
    /// selection at once.
    public private(set) var shownSelection: Int64?
    public var tab: Tab = .fields {
        didSet { if tab == .cards, oldValue != .cards, let id = selection, records[id] != nil { refreshIssues(id, immediately: true) } }
    }
    /// iPad: the field whose options show in a popover.
    public var optionsField: Int?
    /// Mac: the Fields tab's inspector shows (N22.2). While it does, Revert,
    /// Save and its toggle sit in the toolbar above it, at the window's end.
    public var inspectorShown = true
    public private(set) var records: [Int64: NotetypeRecord] = [:]
    public private(set) var recordFailed: Set<Int64> = []
    public private(set) var typeCounts: [Int64: NotetypeRemovalCounts] = [:]
    public private(set) var fieldCounts: [Int64: NotetypeFieldContentCounts] = [:]
    /// D1: the note type changed elsewhere while its draft is open.
    public private(set) var changedElsewhere: Set<Int64> = []
    /// The selected row of each note type's Fields tab.
    public var selectedField: [Int64: Int] = [:]
    /// The row whose name is being edited in place (W3).
    public var renaming: Int?
    public private(set) var renameRefusal: RenameRefusal?
    /// The Cards tab: the selected card type's row per note type (W5), the
    /// part the editor shows (W6), and the card type renamed in place.
    public var selectedCard: [Int64: Int] = [:]
    public var cardPart: NoteTypeTemplatePart = .front
    public var renamingCard: Int?
    /// W5's counts per stored template ordinal, as read for the base.
    public private(set) var templateCardCounts: [Int64: [UInt64]] = [:]
    /// W7's red issues and W8's blank fronts as last read; each keeps the
    /// draft positions it was read for, and shows until the next read lands
    /// (no blank list while typing).
    public private(set) var issueReads: [Int64: NoteTypeIssueRead] = [:]
    public private(set) var blankReads: [Int64: NoteTypeBlankRead] = [:]
    /// A command for the template editor (an Insert menu item, a fix, a
    /// caret move), applied by the text view so it joins its own undo.
    public private(set) var editorRequest: NoteTypeEditorRequest?
    /// Flip found no separator with text on both sides (A15).
    public var flipRefused = false
    public private(set) var question: Question?
    /// The confirm button of the shown question is waiting for D5 step 4's read.
    public private(set) var confirming = false
    public private(set) var preparing: Int64?
    public private(set) var progress: Progress?
    public var failure: Failure?
    public var notice: Notice?
    /// D4's unreachable case: the check made as the sheet opened found that
    /// Meguri Sync does not answer. The sheet then says so and its button
    /// reads Save: the save runs now and the upload is left pending.
    public private(set) var meguriUnreachable = false
    /// "Discard changes?" for Revert (D1).
    public var confirmingRevert = false

    private var drafts: [Int64: NoteTypeDraft] = [:]
    /// After Save or Reload, the version shown before, read-only, until the
    /// note type is read again: the detail never empties in between.
    private var rereading: [Int64: NoteTypeDraft] = [:]
    private var revisions: [Int64: UInt64] = [:]
    private var summaries: [Int64: (revision: UInt64, summary: NotetypeDraftSummary)] = [:]
    private var references: [Int64: (texts: [String], references: [[String]], cards: [Int])] = [:]
    private var listToken: UInt64 = 0
    @ObservationIgnored private var summaryTasks: [Int64: Task<Void, Never>] = [:]
    @ObservationIgnored private var detailTasks: [Int64: Task<Void, Never>] = [:]
    /// Records read ahead (`warm`) that are still in flight.
    @ObservationIgnored private var warming: [Int64: UInt64] = [:]
    @ObservationIgnored private var warmToken: UInt64 = 0
    /// Note types whose detail loaded (counts, issues, references) because
    /// they were selected; a record only read ahead is not among them, and
    /// changes elsewhere read nothing more for it.
    @ObservationIgnored private var loaded: Set<Int64> = []
    @ObservationIgnored private var countTasks: [Int64: [Task<Void, Never>]] = [:]
    /// The base each note type's Notes counts were read for: counts read for
    /// another field layout are not shown on the draft's rows.
    private var fieldCountsBase: [Int64: [UInt8]] = [:]
    /// The newest rename request per row: an older answer never replaces a newer name.
    @ObservationIgnored private var renameTokens: [RenameKey: UInt64] = [:]
    private struct RenameKey: Hashable { let notetype: Int64; let row: Int }
    /// Note types whose stored version changed under their draft (D1), kept
    /// after Keep Editing hides the row, so Revert reads the newer version.
    private var stale: Set<Int64> = []
    /// The section has shown once: a new collection's list is read at once.
    @ObservationIgnored private var appeared = false
    @ObservationIgnored private var saveTask: Task<Void, Never>?
    @ObservationIgnored private var countsTask: Task<Void, Never>?
    @ObservationIgnored private var issueTasks: [Int64: Task<Void, Never>] = [:]
    @ObservationIgnored private var referencesTask: [Int64: Task<Void, Never>] = [:]
    @ObservationIgnored private var blankTasks: [Int64: Task<Void, Never>] = [:]
    /// What W8 was last read for (the fronts and fields), so typing in a
    /// back or the styling starts no pass over the notes.
    @ObservationIgnored private var blankKeys: [Int64: [String]] = [:]
    @ObservationIgnored private var cardCountsBase: [Int64: [UInt8]] = [:]
    @ObservationIgnored private var blankGeneration: UInt64 = 0
    @ObservationIgnored private var blankOwners: [Int64: UInt64] = [:]
    @ObservationIgnored private var editorToken: UInt64 = 0

    // MARK: Seams

    @ObservationIgnored let backend: any NoteTypesBackend
    /// D3: the service and its binding, read at Save (and for the button's title).
    @ObservationIgnored public var service: @MainActor () -> NoteTypeSaveService = { .other }
    /// Whether a collection exists to read: false on first run, before one
    /// is created, imported or synced (N18.3).
    @ObservationIgnored public var hasCollection: @MainActor () -> Bool = { true }
    /// Whether the collection takes a write now (no review, ready, not closing).
    @ObservationIgnored public var writable: @MainActor () -> Bool = { true }
    /// Shows the Save sheet (`SheetRoute.noteTypeSave`); false when another
    /// sheet holds the window, and then nothing is asked or saved.
    @ObservationIgnored public var presentQuestion: @MainActor () -> Bool = { true }
    @ObservationIgnored public var dismissQuestion: @MainActor () -> Void = {}
    /// D6's Save and Upload: Meguri Sync's structure save job (MP7). Without
    /// a ready binding Save and Upload is disabled and the sheet says why;
    /// nothing is saved on this device alone (D3: never D5's local save for
    /// a schema-classified save while Meguri Sync is chosen). The job runs
    /// with the note type locked like a local save; its refusals
    /// (`LossesChanged`, `Stale`) are handled as D5 step 6 says.
    @ObservationIgnored public var meguriSync: (any NoteTypesMeguriSync)?
    /// W7's Show Notes: Browse on these notes (`browser/workspace` V15).
    /// Nil until Browse can take them, and the fix is not offered.
    @ObservationIgnored public var showNotes: (@MainActor ([Int64]) -> Void)?
    /// The reachability check that belongs to the shown question.
    @ObservationIgnored private var reachabilityToken: UInt64 = 0
    @ObservationIgnored private var reachabilityTask: Task<Void, Never>?
    /// Meguri Sync's job is running for a Save (its cancellation reaches the job).
    @ObservationIgnored private var syncJobRunning = false
    /// The app is leaving (background, close): a Save that reaches its
    /// submission now starts no Meguri Sync job.
    @ObservationIgnored private var leaving = false
    /// Text a view holds but has not committed (a rename field, the
    /// inspector's Name or Placeholder), committed before Save reads the draft.
    /// Each returns false when the text was refused (V5), and Save stops.
    public var pendingEdits: [String: @MainActor () async -> Bool] = [:]
    /// A Save is between its first commit and its end: a second one waits.
    @ObservationIgnored private var saveRunning = false
    /// Commits a view started (a rename still being checked after focus
    /// left), awaited by Save whatever field has the focus now.
    @ObservationIgnored private var inflightCommits: [TrackedCommitKey: Task<Bool, Never>] = [:]
    private struct TrackedCommitKey: Hashable { let notetype: Int64; let key: String }
    /// Save is committing typed text: text inputs wait, so nothing typed now is lost.
    public private(set) var committingForSave = false
    /// Raised when the collection goes: work started before must not land.
    private var epoch: UInt64 = 0
    /// How long a summary read waits after the last change (W7's pause).
    @ObservationIgnored var summaryDelay: Duration = .milliseconds(300)

    public init(backend: any NoteTypesBackend) {
        self.backend = backend
    }

    // MARK: The list (W2)

    /// The list as the filter leaves it.
    public var filteredItems: [Item] {
        let query = filter.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return items }
        return items.filter { $0.name.range(of: query, options: [.caseInsensitive]) != nil }
    }

    /// Reads the list once, when the section first shows; later changes
    /// refresh it from the change events.
    public func appear() {
        appeared = true
        guard phase == .idle || phase == .failed || phase == .noCollection else { return }
        reloadList()
    }

    public func reloadList() {
        listToken &+= 1
        let token = listToken
        // No collection: nothing to read and nothing failed.
        guard hasCollection() else { items = []; selection = nil; phase = .noCollection; return }
        if items.isEmpty { phase = .loading }
        Task {
            do {
                let names = try await backend.list()
                guard token == listToken else { return }
                let notes = Dictionary(uniqueKeysWithValues: items.map { ($0.id, $0.notes) })
                items = names.map { Item(id: $0.id, name: $0.name, notes: notes[$0.id] ?? nil) }
                phase = .ready
                if selection.map({ id in !items.contains { $0.id == id } }) ?? true { selection = items.first?.id }
                refreshCounts()
            } catch {
                guard token == listToken else { return }
                // A collection that went while the list was read is not a failure,
                // and its rows go with it.
                guard hasCollection() else { items = []; selection = nil; phase = .noCollection; return }
                phase = items.isEmpty ? .failed : .ready
            }
        }
    }

    private func refreshCounts() {
        countsTask?.cancel()
        countsTask = Task {
            guard let counts = try? await backend.noteCounts(), !Task.isCancelled else { return }
            for index in items.indices { items[index].notes = counts[items[index].id] ?? 0 }
        }
    }

    public func item(_ id: Int64) -> Item? { items.first { $0.id == id } }

    /// W2's "Edited": the draft differs from its base.
    public func isEdited(_ id: Int64) -> Bool { drafts[id]?.isEdited == true }

    // MARK: The detail

    /// The draft the detail shows: the open draft, else the base as read.
    public func draft(_ id: Int64) -> NoteTypeDraft? {
        drafts[id] ?? records[id].map(NoteTypeDraft.init) ?? rereading[id]
    }

    public var selectedDraft: NoteTypeDraft? { selection.flatMap(draft) }

    private func selectionChanged() {
        renaming = nil
        renameRefusal = nil
        renamingCard = nil
        guard let id = selection else { shownSelection = nil; return }
        loadDetail(id)
        showSelectionWhenReady()
        warmNeighbours(of: id)
    }

    /// The selection becomes the shown note type as soon as it can draw;
    /// with nothing shown before, at once (the detail's own progress then).
    private func showSelectionWhenReady() {
        guard let id = selection else { shownSelection = nil; return }
        guard shownSelection != id, shownSelection == nil || draft(id) != nil || recordFailed.contains(id) else { return }
        // A rename begun on the note type shown before never carries over to this one.
        renaming = nil
        renameRefusal = nil
        renamingCard = nil
        shownSelection = id
    }

    /// Reads ahead, with pure reads, the records of the rows beside the
    /// selection, so moving to them with the arrow keys draws at once.
    private func warmNeighbours(of id: Int64) {
        let rows = filteredItems
        guard let at = rows.firstIndex(where: { $0.id == id }) else { return }
        for neighbour in [at - 1, at + 1] where rows.indices.contains(neighbour) { warm(rows[neighbour].id) }
    }

    /// Reads a note type's record ahead (pure read): a row about to open
    /// (iPad) or beside the selection. Nothing is selected or counted.
    public func warm(_ id: Int64) {
        // Not while its record is read already (a load in flight).
        guard records[id] == nil, detailTasks[id] == nil, !recordFailed.contains(id) else { return }
        let epoch = epoch
        warmToken &+= 1
        let token = warmToken
        warming[id] = token
        detailTasks[id] = Task {
            // Only the read that still owns the entry clears it: a newer one may have replaced it.
            defer { if warming[id] == token { warming[id] = nil } }
            do {
                let record = try await backend.read(id)
                guard !Task.isCancelled, self.epoch == epoch, records[id] == nil else { return }
                records[id] = record
                if drafts[id] == nil, selectedField[id] == nil { selectedField[id] = record.fields.first.map { Int($0.ord) } }
                // Selected while it was read: it loads as a selection does.
                if selection == id { finishLoading(id) }
            } catch {
                guard !Task.isCancelled, self.epoch == epoch, selection == id else { return }
                recordFailed.insert(id)
                showSelectionWhenReady()
            }
        }
    }

    private func loadDetail(_ id: Int64, force: Bool = false) {
        // Read ahead and now selected: it loads as a selection does (counts, issues, Used On).
        if !force, records[id] != nil, !recordFailed.contains(id), !loaded.contains(id) {
            finishLoading(id)
            return
        }
        // A cached note type whose counts were dropped (its notes changed) reads them again.
        if !force, records[id] != nil, !recordFailed.contains(id), typeCounts[id] == nil || fieldCounts[id] == nil || templateCardCounts[id] == nil {
            readCounts(id)
        }
        // Notes changed since W8 was read: it reads again when the note type shows.
        if !force, records[id] != nil, blankKeys[id] == nil { refreshIssues(id, immediately: true) }
        // A read ahead in flight finishes the load itself (`warm`).
        if !force, warming[id] != nil { return }
        guard force || records[id] == nil || recordFailed.contains(id) else { return }
        detailTasks[id]?.cancel()
        recordFailed.remove(id)
        let epoch = epoch
        detailTasks[id] = Task {
            do {
                let record = try await backend.read(id)
                guard !Task.isCancelled, self.epoch == epoch else { return }
                records[id] = record
                rereading[id] = nil
                if drafts[id] == nil, selectedField[id] == nil { selectedField[id] = record.fields.first.map { Int($0.ord) } }
                finishLoading(id)
            } catch {
                guard !Task.isCancelled else { return }
                rereading[id] = nil
                if records[id] == nil { recordFailed.insert(id) }
                if selection == id { showSelectionWhenReady() }
            }
        }
    }

    /// A record just read: its counts, issues and references, and the
    /// detail swaps to it in one step if it is the selection.
    private func finishLoading(_ id: Int64) {
        loaded.insert(id)
        if selection == id { showSelectionWhenReady() }
        readCounts(id)
        refreshIssues(id, immediately: true)
        referencesTask[id]?.cancel()
        referencesTask[id] = Task { await readReferences(id) }
    }

    /// The subtitle's counts and W3.1's Notes column, read in the background.
    private func readCounts(_ id: Int64) {
        let epoch = epoch
        let base = records[id]?.base
        for task in countTasks[id] ?? [] { task.cancel() }
        countTasks[id] = [
            Task {
                let counts = try? await backend.typeCounts(id)
                guard !Task.isCancelled, self.epoch == epoch, let counts else { return }
                typeCounts[id] = counts
            },
            // W5: by stored ordinal, for the base the rows show; a draft whose
            // note type changed elsewhere waits for Reload or Revert, as Notes does.
            Task {
                guard !stale.contains(id) else { return }
                let counts = try? await backend.templateCardCounts(id)
                guard !Task.isCancelled, self.epoch == epoch, let counts, records[id]?.base == base, !stale.contains(id) else { return }
                templateCardCounts[id] = counts
                cardCountsBase[id] = base
            },
        ]
        // A note type changed elsewhere keeps the draft's older base, while the
        // core counts the newer layout's fields: those counts would sit on the
        // wrong rows, so the Notes column waits for Reload or Revert.
        guard !stale.contains(id) else { return }
        countTasks[id, default: []].append(Task {
            let counts = try? await backend.fieldContentCounts(id)
            guard !Task.isCancelled, self.epoch == epoch, let counts, records[id]?.base == base, !stale.contains(id) else { return }
            fieldCounts[id] = counts
            fieldCountsBase[id] = base
        })
    }

    private func readReferences(_ id: Int64) async {
        guard let draft = draft(id) else { return }
        let texts = draft.referenceTexts
        // Read again when the texts or the card types they belong to changed.
        guard references[id]?.texts != texts || references[id]?.cards != draft.keptCardRows.map(\.id) else { return }
        guard let refs = try? await backend.fieldReferences(texts), let now = self.draft(id), now.referenceTexts == texts else { return }
        references[id] = (texts, refs, now.keptCardRows.map(\.id))
    }

    /// W1's Used On for each row of the note type, by card type position;
    /// nil until the references are read.
    public func usedOn(_ id: Int64) -> [Int: [Int]]? {
        // The last read shows while typing's next read runs, for the same card types.
        guard let draft = draft(id), let read = references[id], read.cards == draft.keptCardRows.map(\.id) else { return nil }
        return draft.usedOn(references: read.references)
    }

    /// W3's Notes for a row: the notes with content of its base field (a new
    /// field shows 0), with the type's notes; nil until known.
    public func notesWithContent(_ id: Int64, row: NoteTypeFieldRow) -> (count: UInt64, of: UInt64)? {
        // Counts read for another field layout (changed elsewhere) are not shown on these rows.
        guard let counts = fieldCounts[id], let base = draft(id)?.base.base, fieldCountsBase[id] == base else { return nil }
        guard let origin = row.origin else { return (0, counts.notes) }
        return counts.fields[safe: Int(origin)].map { ($0, counts.notes) }
    }

    /// Editing stops while a save prepares or runs (D5 step 5).
    public func editable(_ id: Int64) -> Bool {
        progress?.notetypeId != id && preparing != id && !(question?.notetypeId == id) && rereading[id] == nil
            // The note type shown in the selected one's place for a moment takes no edits.
            && !(shownSelection != selection && id == shownSelection)
    }

    // MARK: Editing (W3, W4)

    /// Applies `change` to the note type's draft, making it from the base on
    /// the first change (D1).
    @discardableResult public func edit(_ id: Int64, _ change: (inout NoteTypeDraft) -> Bool) -> Bool {
        guard editable(id), var draft = draft(id), change(&draft) else { return false }
        drafts[id] = draft
        draftChanged(id)
        return true
    }

    public func deleteField(_ id: Int64, row: Int) {
        guard edit(id, { $0.delete(row) }) else { return }
        if renaming == row { renaming = nil }
    }

    public func restoreField(_ id: Int64, row: Int) { edit(id) { $0.restore(row) } }

    public func moveFields(_ id: Int64, fromOffsets offsets: IndexSet, toOffset destination: Int) {
        edit(id) { $0.move(fromOffsets: offsets, toOffset: destination) }
    }

    public func setSortField(_ id: Int64, row: Int) { edit(id) { $0.setSortField(row) } }

    public func setOption(_ id: Int64, row: Int, _ change: (inout StructureDraftField) -> Void) {
        edit(id) { $0.setOption(row, change) }
    }

    /// + : a new field after the last, selected and in rename mode.
    public func addField(_ id: Int64) {
        var added: Int?
        guard edit(id, { added = $0.addField(); return true }), let added else { return }
        selectedField[id] = added
        renameRefusal = nil
        renaming = added
    }

    /// Return in a rename field, or the inspector's Name: V5 on the typed
    /// text. A refusal keeps the old name and is shown under the row.
    @discardableResult public func commitRename(_ id: Int64, row: Int, typed: String) async -> Bool {
        // A crossed-out field's name is free (S4.2): it is left out of V5's comparison.
        func names() -> (names: [String], at: Int)? {
            guard let draft = draft(id), let at = draft.rows.firstIndex(where: { $0.id == row }) else { return nil }
            return (draft.rows.map { $0.deleted ? "" : $0.name }, at)
        }
        let key = RenameKey(notetype: id, row: row)
        renameTokens[key, default: 0] &+= 1
        let token = renameTokens[key]
        let epoch = epoch
        var check: NotetypeFieldNameCheck?
        // The answer counts only for the names it checked: another rename that
        // landed meanwhile is checked against again.
        for _ in 0..<3 {
            guard let before = names() else { return true }
            guard let answer = try? await backend.checkFieldName(typed, fields: before.names, renaming: UInt32(before.at)) else { return false }
            // A newer rename of this row owns it now: this answer is dropped.
            guard self.epoch == epoch, renameTokens[key] == token else { return true }
            if let after = names(), after.names == before.names, after.at == before.at { check = answer; break }
        }
        guard let check else { return false }
        // The rename field and its refusal belong to the shown note type: an
        // answer for another one changes its draft only.
        let shown = selection == id
        switch check {
        case .ignored:
            if shown { renameRefusal = nil; if renaming == row { renaming = nil } }
        case .accepted(let name):
            if shown { renameRefusal = nil; if renaming == row { renaming = nil } }
            edit(id) { $0.rename(row, to: name) }
        case .refused(let refusal):
            if shown { renameRefusal = RenameRefusal(row: row, refusal: refusal) }
            return false
        }
        return true
    }

    /// A commit a view started on its own (focus left a field): Save waits
    /// for it. A newer commit under the same key replaces an older one, so a
    /// corrected name is the one that counts.
    /// `notetype`: the note type whose draft the commit changes; only its own
    /// Save waits for it, and only its refusal stops that Save.
    public func trackCommit(_ notetype: Int64, _ key: String, _ commit: @escaping @MainActor () async -> Bool) {
        let key = TrackedCommitKey(notetype: notetype, key: key)
        let task = Task { await commit() }
        inflightCommits[key] = task
        Task { _ = await task.value; if inflightCommits[key] == task { inflightCommits[key] = nil } }
    }

    public func cancelRename() {
        // A check still running for the row being renamed no longer applies.
        if let id = selection, let row = renaming ?? renameRefusal?.row {
            renameTokens[RenameKey(notetype: id, row: row), default: 0] &+= 1
        }
        renaming = nil
        renameRefusal = nil
    }

    // MARK: The Cards tab (W5 to W8)

    /// The card type the Cards tab shows: the selected row while the draft
    /// has it, else the first that stays.
    public func shownCard(_ id: Int64) -> Int? {
        guard let draft = draft(id) else { return nil }
        if let row = selectedCard[id], draft.cardRow(row) != nil { return row }
        return draft.keptCardRows.first?.id ?? draft.cardRows.first?.id
    }

    /// W5's count for a card type: its base ordinal's cards (0 for a new
    /// one); nil until known.
    public func cardCount(_ id: Int64, row: NoteTypeCardTypeRow) -> UInt64? {
        guard !stale.contains(id), let counts = templateCardCounts[id], let base = draft(id)?.base.base, cardCountsBase[id] == base else {
            return nil
        }
        guard let origin = row.origin else { return 0 }
        return counts[safe: Int(origin)] ?? 0
    }

    /// + and Duplicate (A6): a copy of the shown card type, selected and in rename mode.
    public func addCardType(_ id: Int64) {
        guard let source = shownCard(id) else { return }
        var added: Int?
        guard edit(id, { added = $0.addCardType(copying: source); return added != nil }), let added else { return }
        selectedCard[id] = added
        renamingCard = added
    }

    public func deleteCardType(_ id: Int64, row: Int) {
        guard edit(id, { $0.deleteCardType(row) }) else { return }
        if renamingCard == row { renamingCard = nil }
    }

    public func restoreCardType(_ id: Int64, row: Int) { edit(id) { $0.restoreCardType(row) } }

    /// Return in the rename field (A8); a blank name changes nothing.
    public func renameCardType(_ id: Int64, row: Int, typed: String) {
        edit(id) { $0.renameCardType(row, to: typed) }
        if renamingCard == row { renamingCard = nil }
    }

    public func moveCardTypes(_ id: Int64, fromOffsets offsets: IndexSet, toOffset destination: Int) {
        edit(id) { $0.moveCardTypes(fromOffsets: offsets, toOffset: destination) }
    }

    public func moveCardType(_ id: Int64, row: Int, by delta: Int) { edit(id) { $0.moveCardType(row, by: delta) } }

    /// Typing in the template editor (no draft step: the text view's own undo).
    public func setText(_ id: Int64, card: Int, part: NoteTypeTemplatePart, _ text: String) {
        edit(id) { $0.setText(card, part, text) }
    }

    /// A15's Flip of a card type's front and back: the texts it gives, with
    /// the front it replaces; nil, and `flipRefused`, when the back has no
    /// separator with text on both sides. Nothing changes yet: the Back's
    /// editor writes both as one step of its own undo.
    public func flipped(_ id: Int64, card: Int) -> NoteTypeFlip? {
        guard editable(id), let texts = draft(id)?.texts(card) else { return nil }
        guard let flipped = NoteTypeTemplateEditing.flip(front: texts.front, back: texts.back) else { flipRefused = true; return nil }
        return NoteTypeFlip(front: flipped.front, back: flipped.back, previousFront: texts.front)
    }

    /// Flip without an editor: both texts in one draft change, so a Save
    /// never takes one without the other.
    public func flip(_ id: Int64, card: Int) {
        guard let flipped = flipped(id, card: card) else { return }
        edit(id) { draft in
            let front = draft.setText(card, .front, flipped.front)
            let back = draft.setText(card, .back, flipped.back)
            return front || back
        }
    }

    /// Writes a part that is not shown (Flip's front, and its undo) when it
    /// still reads `expected`; false, changing nothing, once it was edited since.
    @discardableResult
    public func replaceText(_ id: Int64, card: Int, part: NoteTypeTemplatePart, expected: String, with text: String) -> Bool {
        guard editable(id), draft(id)?.text(card, part) == expected else { return false }
        edit(id) { $0.setText(card, part, text) }
        return true
    }

    /// Asks the template editor to apply `command` on a card type's part.
    public func request(_ id: Int64, card: Int, part: NoteTypeTemplatePart, _ command: NoteTypeEditorRequest.Command) {
        // Only a caret move is asked of a locked note type (a save runs).
        if case .moveCaret = command {} else if !editable(id) { return }
        // A whole-text change still waiting (Flip's back, a fix) lands in the
        // draft before another request takes the slot, so no half is lost.
        if let pending = editorRequest {
            switch pending.command {
            case .replaceAll(let text):
                edit(pending.notetype) { $0.setText(pending.card, pending.part, text) }
            case .append(let suffix):
                if let text = draft(pending.notetype)?.text(pending.card, pending.part) {
                    edit(pending.notetype) { $0.setText(pending.card, pending.part, text + suffix) }
                }
            default: break
            }
        }
        editorToken &+= 1
        editorRequest = NoteTypeEditorRequest(token: editorToken, notetype: id, card: card, part: part, command: command)
    }

    /// The text view applied (or dropped) the request.
    public func consumeEditorRequest(_ token: UInt64) {
        if editorRequest?.token == token { editorRequest = nil }
    }

    /// Each kept card type's front and back, as an issue read saw them.
    private static func partTexts(_ draft: NoteTypeDraft) -> [Int: [NoteTypeTemplatePart: String]] {
        Dictionary(uniqueKeysWithValues: draft.keptCardRows.map { row in
            (row.id, [.front: draft.text(row.id, .front) ?? "", .back: draft.text(row.id, .back) ?? ""])
        })
    }

    /// W6's colouring of a text (pure: no collection is read).
    public func syntax(_ text: String, styling: Bool) async -> [NotetypeSyntaxSpan] {
        (try? await backend.syntax(text, styling: styling)) ?? []
    }

    /// W7's lines for the note type: red, then yellow.
    public func liveIssues(_ id: Int64) -> [NoteTypeLiveIssue] {
        guard let draft = draft(id) else { return [] }
        return NoteTypeLiveIssue.lines(red: issueReads[id], yellow: blankReads[id], draft: draft)
    }

    /// Whether the issue reads have answered for the note type (W7's, and
    /// W8's unless it is a cloze kind): before that the list says nothing
    /// rather than "No issues" it cannot know yet.
    public func issuesKnown(_ id: Int64) -> Bool {
        guard let draft = draft(id), issueReads[id] != nil else { return false }
        return draft.isCloze || blankReads[id] != nil
    }

    public func redIssueCount(_ id: Int64) -> Int { liveIssues(id).filter { $0.severity == .red }.count }

    /// W7: red issues block Save. A draft that leaves the templates and the
    /// styling as stored saves unchecked (V1), so issues already stored do
    /// not hold a Fields tab edit.
    public func issuesBlockSave(_ id: Int64) -> Bool {
        guard let draft = drafts[id], !draft.fieldsOnly else { return false }
        return redIssueCount(id) > 0
    }

    /// Clicking an issue: its card type and part show, the caret on its line.
    public func showIssue(_ id: Int64, _ issue: NoteTypeLiveIssue) {
        selectedCard[id] = issue.card
        guard let part = issue.part else { return }
        cardPart = part
        if let line = issue.line { request(id, card: issue.card, part: part, .moveCaret(line: line)) }
    }

    /// An issue's one-click fix (W7).
    public func applyFix(_ id: Int64, _ issue: NoteTypeLiveIssue) {
        guard let fix = issue.fix else { return }
        switch fix {
        case .showNotes(let notes):
            showNotes?(notes)
        case .addClose(let close):
            // The read may be older than the text: a close typed since needs no second one.
            guard let part = issue.part, let text = draft(id)?.text(issue.card, part),
                  issueReads[id]?.texts[issue.card]?[part] == text else { return }
            selectedCard[id] = issue.card
            cardPart = part
            request(id, card: issue.card, part: part, .append(close))
        case .useField(let from, let to):
            guard editable(id), let part = issue.part, let text = draft(id)?.text(issue.card, part) else { return }
            let epoch = epoch
            Task {
                guard let fixed = try? await backend.replaceFieldReferences(text, from: from, to: to), self.epoch == epoch,
                      draft(id)?.text(issue.card, part) == text, fixed != text else { return }
                selectedCard[id] = issue.card
                cardPart = part
                request(id, card: issue.card, part: part, .replaceAll(fixed))
            }
        }
    }

    /// W7 and W8 for the draft as it is: the red issues about 300 ms after
    /// the last change, W8's pass only when a front or the fields changed.
    private func refreshIssues(_ id: Int64, immediately: Bool = false) {
        guard let draft = draft(id) else { return }
        let structure = draft.structureDraft()
        let cards = draft.keptCardRows.map(\.id)
        let fields = structure.fields.map(\.name)
        let texts = Self.partTexts(draft)
        let delay = immediately ? Duration.zero : summaryDelay
        let epoch = epoch
        let revision = revisions[id]
        issueTasks[id]?.cancel()
        issueTasks[id] = Task {
            if delay > .zero { try? await Task.sleep(for: delay) }
            guard !Task.isCancelled else { return }
            let found = try? await backend.templateIssues(structure)
            guard !Task.isCancelled, self.epoch == epoch, revisions[id] == revision else { return }
            // A draft the core cannot read (changed elsewhere) shows no issues.
            issueReads[id] = found.map { NoteTypeIssueRead(cards: cards, fields: fields, issues: $0, texts: texts) }
        }
        guard !draft.isCloze else { blankReads[id] = nil; return }
        // W8 reads every note of the type: only for the Cards tab, which shows it.
        guard tab == .cards, selection == id else { return }
        let key = structure.templates.map(\.front) + ["\u{0}"] + structure.fields.map { "\($0.origin.map(String.init) ?? "+")\u{1}\($0.name)" }
            + ["\u{0}"] + cards.map(String.init)
        guard key != blankKeys[id] else { return }
        blankKeys[id] = key
        blankTasks[id]?.cancel()
        // Only the newest task owns the key: a cancelled one never clears it.
        blankGeneration &+= 1
        let generation = blankGeneration
        blankTasks[id] = Task {
            if delay > .zero { try? await Task.sleep(for: delay) }
            guard !Task.isCancelled else { return }
            // Left the Cards tab or this note type while it waited: the next look reads it.
            guard tab == .cards, selection == id else {
                if blankOwners[id] == generation { blankKeys[id] = nil }
                return
            }
            let fronts = try? await backend.notesWithoutCard(structure)
            guard !Task.isCancelled, self.epoch == epoch, blankOwners[id] == generation else { return }
            if let fronts { blankReads[id] = NoteTypeBlankRead(cards: cards, fields: fields, fronts: fronts) } else { blankKeys[id] = nil }
        }
        blankOwners[id] = generation
    }

    // MARK: The editor's own undo (W13)

    /// The draft step Edit › Undo names while Note Types shows ("Delete
    /// Field"); nil when the draft has none, and the collection's Undo applies.
    public var undoStep: NoteTypeDraftStep? { selection.flatMap { id in editable(id) ? drafts[id]?.undoStep : nil } }
    public var redoStep: NoteTypeDraftStep? { selection.flatMap { id in editable(id) ? drafts[id]?.redoStep : nil } }

    public func undo() {
        guard let id = selection, editable(id), drafts[id]?.undo() != nil else { return }
        renaming = nil; renameRefusal = nil; renamingCard = nil
        draftChanged(id)
    }

    public func redo() {
        guard let id = selection, editable(id), drafts[id]?.redo() != nil else { return }
        renaming = nil; renameRefusal = nil; renamingCard = nil
        draftChanged(id)
    }

    /// Revert after "Discard changes?" (D1).
    public func revert(_ id: Int64) {
        confirmingRevert = false
        guard editable(id) else { return }
        // The note type changed elsewhere: Revert shows the newer version, read again.
        if stale.contains(id) { reload(id); return }
        drafts[id] = nil
        changedElsewhere.remove(id)
        renaming = nil; renameRefusal = nil
        draftChanged(id)
    }

    /// D1's Reload: the draft goes and the note type is read again.
    public func reload(_ id: Int64) {
        guard editable(id) else { return }
        rereading[id] = draft(id)
        drafts[id] = nil
        // No edit lands on the old base while the newer one is read.
        records[id] = nil
        changedElsewhere.remove(id)
        stale.remove(id)
        if selection == id { cancelRename() }
        draftChanged(id)
        loadDetail(id, force: true)
    }

    /// D1's Keep Editing: the row goes; Save meets `Stale` and offers Reload again.
    public func keepEditing(_ id: Int64) { changedElsewhere.remove(id) }  // `stale` stays: Revert reads again

    private func draftChanged(_ id: Int64) {
        revisions[id, default: 0] &+= 1
        refreshIssues(id)
        // Used On reads again for the texts shown, a reverted draft's too.
        let delay = summaryDelay
        referencesTask[id]?.cancel()
        referencesTask[id] = Task {
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled else { return }
            await readReferences(id)
        }
        summaryTasks[id]?.cancel()
        guard let draft = drafts[id], draft.isEdited else { summaries[id] = nil; return }
        let revision = revisions[id] ?? 0
        let structure = draft.structureDraft()
        let epoch = epoch
        summaryTasks[id] = Task {
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled, let summary = try? await backend.summary(structure), !Task.isCancelled,
                  self.epoch == epoch, revisions[id] == revision else { return }
            summaries[id] = (revision, summary)
        }
    }

    // MARK: Save (D3, D5)

    /// Whether Save will ask (its title then reads "Save…", D3): from the
    /// summary of the draft as it is, or, until that is read, from the
    /// draft's own changes and the Notes counts.
    public func saveAsks(_ id: Int64) -> Bool {
        guard let draft = drafts[id], draft.isEdited else { return false }
        let service = service()
        if let read = summaries[id], read.revision == revisions[id] {
            return NoteTypeSaveDecision.decide(read.summary, service: service).asks
        }
        let losesContent = draft.rows.contains { row in
            guard row.deleted, let origin = row.origin else { return false }
            return (fieldCounts[id]?.fields[safe: Int(origin)] ?? 1) > 0
        }
        return losesContent || (service == .meguriSync && draft.changesStructure)
    }

    public func canSave(_ id: Int64) -> Bool {
        // A refused name (V5) holds Save until it is corrected (typed text Save commits first) or cancelled.
        (isEdited(id) || !pendingEdits.isEmpty) && editable(id) && progress == nil && preparing == nil && question == nil
            && !saveRunning && (renameRefusal == nil || !pendingEdits.isEmpty) && writable() && !issuesBlockSave(id)
    }

    /// Save (⌘S): D2's summary of the draft as it is, then D3; the sheet
    /// when it asks, else the save at once with nothing confirmed.
    /// `id`: the note type on screen (iPad's page); the selection otherwise.
    public func save(_ notetype: Int64? = nil) {
        guard let id = notetype ?? selection, canSave(id) else { return }
        let epoch = epoch
        saveRunning = true
        saveTask = Task {
            defer { saveRunning = false }
            // Typed text that is not committed yet goes into the draft first; a refused name stops Save.
            committingForSave = true
            var accepted = true
            // Commits that start while Save waits are awaited too.
            // Another note type's commits change its own draft only: they run on, unawaited.
            while true {
                let batch = inflightCommits.filter { $0.key.notetype == id }
                if batch.isEmpty { break }
                for key in batch.keys { inflightCommits[key] = nil }
                for task in batch.values { accepted = await task.value && accepted }
            }
            for commit in pendingEdits.values where accepted { accepted = await commit() }
            committingForSave = false
            guard accepted, renameRefusal == nil else { return }
            saveRunning = false
            guard self.epoch == epoch, canSave(id), isEdited(id), let draft = drafts[id] else { return }
            saveRunning = true
            renaming = nil; renameRefusal = nil
            preparing = id
            let revision = revisions[id]
            defer { if preparing == id { preparing = nil } }
            // W7: a checked save with red issues is not submitted; they show at once.
            if !draft.fieldsOnly {
                let found: [NotetypeTemplateIssue]
                do { found = try await backend.templateIssues(draft.structureDraft()) } catch { refused(id, error); return }
                guard revisions[id] == revision else { return }
                if !found.isEmpty {
                    issueReads[id] = NoteTypeIssueRead(cards: draft.keptCardRows.map(\.id), fields: draft.keptRows.map(\.name), issues: found,
                                                       texts: Self.partTexts(draft))
                    return
                }
            }
            let summary: NotetypeDraftSummary
            do { summary = try await backend.summary(draft.structureDraft()) } catch { refused(id, error); return }
            guard revisions[id] == revision else { return }
            let decision = NoteTypeSaveDecision.decide(summary, service: service())
            if decision.asks {
                ask(Question(notetypeId: id, name: draft.name, summary: summary, decision: decision))
                return
            }
            preparing = nil
            await submit(id, draft: draft, confirmed: .init(), route: decision.route, reachable: true)
        }
    }

    /// Whether Save and Upload can run now; the sheet says why not.
    public var meguriSyncState: NoteTypeSyncState { meguriSync?.noteTypeSyncState ?? .notSetUp }

    /// The sheet's confirm button is enabled: a local save, or Meguri Sync's
    /// job with a ready binding.
    public var canConfirm: Bool {
        guard let question, !confirming else { return false }
        return question.decision.route == .local || (meguriSync != nil && meguriSyncState == .ready)
    }

    /// The sheet's confirm button: D5 step 4, then the op with the counts
    /// the sheet showed.
    public func confirm() {
        guard canConfirm, let shown = question, let draft = drafts[shown.notetypeId] else { return }
        let id = shown.notetypeId
        let confirmed = NotetypeConfirmedLosses(confirming: shown.summary)
        confirming = true
        let epoch = epoch
        saveTask = Task {
            defer { if self.epoch == epoch { confirming = false } }
            let read: Result<NotetypeDraftSummary, any Error>
            do { read = .success(try await backend.summary(draft.structureDraft())) } catch { read = .failure(error) }
            // The collection went while this read ran: nothing of it is submitted.
            guard self.epoch == epoch, !Task.isCancelled, question == shown else { return }
            let latest: NotetypeDraftSummary
            switch read {
            case .success(let summary): latest = summary
            case .failure(let error): closeQuestion(); refused(id, error); return
            }
            let service = service()
            if NoteTypeSaveDecision.asksAgain(confirmed: confirmed, shown: shown.decision, latest: latest, service: service) {
                ask(Question(notetypeId: id, name: draft.name, summary: latest, decision: .decide(latest, service: service)))
                return
            }
            let route = NoteTypeSaveDecision.decide(latest, service: service).route
            let reachable = !meguriUnreachable
            closeQuestion()
            await submit(id, draft: draft, confirmed: confirmed, route: route, reachable: reachable)
        }
    }

    /// The sheet's Cancel (and Esc): nothing is saved; the draft stays.
    public func cancelQuestion() {
        guard question != nil, !confirming else { return }
        closeQuestion()
    }

    /// The sheet left the screen without Cancel (another sheet replaced it).
    public func questionClosed() {
        guard !confirming else { return }
        question = nil
    }

    private func ask(_ question: Question) {
        preparing = nil
        let fresh = self.question == nil
        self.question = question
        if !presentQuestion() {
            self.question = nil
            failure = .notSaved
            return
        }
        if question.decision.route == .meguriSync { checkReachability(fresh: fresh) }
    }

    /// D4: the check made as the sheet opens. The sheet shows Meguri Sync's
    /// paragraph until the check finds the server does not answer; a sheet
    /// shown again keeps what the last check found until this one answers.
    private func checkReachability(fresh: Bool) {
        reachabilityToken &+= 1
        let token = reachabilityToken
        if fresh { meguriUnreachable = false }
        reachabilityTask?.cancel()
        guard let meguriSync else { return }
        reachabilityTask = Task {
            let reachable = await meguriSync.noteTypeSyncReachable()
            guard !Task.isCancelled, token == reachabilityToken, question != nil, !confirming else { return }
            meguriUnreachable = !reachable
        }
    }

    private func closeQuestion() {
        question = nil
        reachabilityToken &+= 1
        reachabilityTask?.cancel()
        dismissQuestion()
    }

    private func submit(_ id: Int64, draft: NoteTypeDraft, confirmed: NotetypeConfirmedLosses, route: NoteTypeSaveDecision.Route,
                        reachable: Bool) async {
        let meguriSync = meguriSync
        // D6: Meguri Sync's job owns the round, the save and the upload; it
        // runs only with a ready binding, read again now.
        guard route == .local || (meguriSync != nil && meguriSyncState == .ready) else {
            if route == .meguriSync { failure = .notSaved }
            return
        }
        guard writable() else { failure = .notSaved; return }
        // Leaving waits for this Save: a job started now would make it wait for a whole round and upload.
        if route == .meguriSync, leaving { return }
        // D5 step 3's deck creations: the Fields tab sets no deck override.
        let epoch = epoch
        progress = Progress(notetypeId: id, stage: route == .local ? nil : (reachable ? .syncing : .saving))
        do {
            if route == .local {
                try await backend.save(draft.structureDraft(confirmed: confirmed)) { [weak self] completed, total in
                    guard self?.progress?.notetypeId == id else { return }
                    self?.progress?.completed = completed
                    self?.progress?.total = total
                }
            } else if let meguriSync {
                syncJobRunning = true
                defer { syncJobRunning = false }
                let result = try await meguriSync.saveNotetypeStructure(draft.structureDraft(confirmed: confirmed), reachable: reachable) {
                    [weak self] stage, completed, total in
                    guard self?.progress?.notetypeId == id else { return }
                    self?.progress?.stage = stage
                    self?.progress?.completed = completed
                    self?.progress?.total = total
                }
                guard self.epoch == epoch else { return }
                progress = nil
                switch result {
                case .saved(let uploaded):
                    saved(id)
                    // Offline, the sheet already said the upload waits (D4); otherwise say it did not happen.
                    if !uploaded, reachable { notice = .notUploaded }
                case .syncStopped: failure = .syncStopped
                case .cancelled: break
                case .notSaved: failure = .notSaved
                case .unconfirmed: failure = .unconfirmed
                }
                return
            }
            guard self.epoch == epoch else { return }
            progress = nil
            saved(id)
        } catch {
            guard self.epoch == epoch else { return }
            progress = nil
            if case BridgeError.commitCompleted = error { saved(id); return }
            refused(id, error)
        }
    }

    /// D5 step 6: the draft is gone and the detail shows the saved note type.
    private func saved(_ id: Int64) {
        rereading[id] = draft(id)
        drafts[id] = nil
        // The saved card types have other rows: W8 reads again for them.
        blankKeys[id] = nil
        // No edit starts from the pre-save version while the saved one is read.
        records[id] = nil
        summaries[id] = nil
        changedElsewhere.remove(id)
        stale.remove(id)
        draftChanged(id)
        loadDetail(id, force: true)
        fieldCounts[id] = nil
        reloadList()
    }

    /// D5 step 6's refusals: the draft stays.
    private func refused(_ id: Int64, _ error: any Error) {
        switch error as? BridgeError {
        case .notetypeStructureRefused(.lossesChanged(let summary))?:
            // D8: the sheet again with the new counts; nothing was saved.
            let decision = NoteTypeSaveDecision.decide(summary, service: service())
            ask(Question(notetypeId: id, name: drafts[id]?.name ?? "", summary: summary, decision: decision))
        case .stale?:
            changedElsewhere.insert(id)
            stale.insert(id)
        case .cancelled?:
            break
        default:
            failure = .notSaved
        }
    }

    // MARK: Changes elsewhere

    /// A committed change: the list and counts follow; a note type with a
    /// draft that changed elsewhere shows D1's row.
    public func changesCommitted(_ changes: ChangeSet) {
        guard phase != .idle else { return }
        let notesMoved = changes.notes.contains { $0.change != .updated }
        // A note edited elsewhere can fill or empty a field: the shown type's Notes column reads again.
        if !notesMoved, changes.notetypes.isEmpty, !changes.wholeCollection, !changes.notes.isEmpty {
            // An edited note can fill or empty a front's field (W8): every read is old.
            blankKeys = [:]
            if let id = selection, records[id] != nil {
                readCounts(id)
                refreshIssues(id, immediately: true)
            }
            // Other loaded note types read theirs when they are shown again; reads
            // already running are cancelled, so no pre-change count lands.
            for id in records.keys where id != selection {
                for task in countTasks[id] ?? [] { task.cancel() }
                countTasks[id] = nil
                fieldCounts[id] = nil; typeCounts[id] = nil; templateCardCounts[id] = nil
                // W8 reads the notes: the next look reads it again.
                blankKeys[id] = nil
            }
            return
        }
        guard changes.wholeCollection || !changes.notetypes.isEmpty || notesMoved else { return }
        // A rename elsewhere arrives as an update: the list reads its names again.
        if changes.wholeCollection || !changes.notetypes.isEmpty {
            reloadList()
        } else {
            refreshCounts()
        }
        // A read ahead still in flight may hold the version from before the change:
        // it is dropped, and read again at once if its note type is selected.
        for id in Array(warming.keys) where changes.wholeCollection || changes.notetypes.contains(where: { $0.id == id }) {
            detailTasks[id]?.cancel()
            detailTasks[id] = nil
            warming[id] = nil
            if selection == id { loadDetail(id) }
        }
        // A record only read ahead goes, to be read again when it is needed.
        for id in records.keys where !loaded.contains(id) && warming[id] == nil
            && (changes.wholeCollection || changes.notetypes.contains(where: { $0.id == id })) {
            records[id] = nil
            detailTasks[id] = nil
        }
        let touched = changes.wholeCollection ? Set(records.keys) : Set(changes.notetypes.map(\.id))
        for id in touched where records[id] != nil {
            if notesMoved || changes.wholeCollection || changes.notetypes.contains(where: { $0.id == id }) {
                typeCounts[id] = nil
                fieldCounts[id] = nil
                templateCardCounts[id] = nil
                blankKeys[id] = nil
            }
            recheck(id)
        }
        if notesMoved, !changes.wholeCollection {
            for id in records.keys where !touched.contains(id) && loaded.contains(id) { readCounts(id) }
        }
        // Notes added or removed change W8's notes: the shown type reads it again.
        if notesMoved || changes.wholeCollection {
            blankKeys = [:]
            if let id = selection, records[id] != nil { refreshIssues(id, immediately: true) }
        }
    }

    private func recheck(_ id: Int64) {
        let epoch = epoch
        Task {
            let read = try? await backend.read(id)
            guard self.epoch == epoch else { return }
            guard let record = read else { readCounts(id); return }
            // Counts are read for the record the rows show, once it is in place.
            guard record.base != records[id]?.base else { readCounts(id); return }
            if let draft = drafts[id], !draft.isEdited { drafts[id] = nil; draftChanged(id) }
            if let draft = drafts[id], draft.base.base != record.base, progress?.notetypeId != id {
                // The draft keeps its base: Save meets `Stale` and offers Reload (D1).
                changedElsewhere.insert(id)
                stale.insert(id)
                // The subtitle's counts were dropped with the change: they read again (type counts only).
                readCounts(id)
                return
            }
            records[id] = record
            readCounts(id)
            if drafts[id] == nil { refreshIssues(id, immediately: true) }
            await readReferences(id)
        }
    }

    /// The collection closed or was replaced: every draft and read goes.
    public func invalidate() {
        listToken &+= 1
        epoch &+= 1
        saveTask?.cancel()
        countsTask?.cancel()
        for task in summaryTasks.values { task.cancel() }
        for task in detailTasks.values { task.cancel() }
        for task in countTasks.values.flatMap({ $0 }) { task.cancel() }
        for task in issueTasks.values { task.cancel() }
        for task in blankTasks.values { task.cancel() }
        for task in referencesTask.values { task.cancel() }
        referencesTask = [:]
        summaryTasks = [:]; detailTasks = [:]; warming = [:]; loaded = []; countTasks = [:]; fieldCountsBase = [:]; renameTokens = [:]
        issueTasks = [:]; blankTasks = [:]; blankKeys = [:]; cardCountsBase = [:]
        selectedCard = [:]; renamingCard = nil; templateCardCounts = [:]; issueReads = [:]; blankReads = [:]; editorRequest = nil
        flipRefused = false
        if question != nil { closeQuestion() }
        phase = .idle; items = []; selection = nil; records = [:]; recordFailed = []; typeCounts = [:]; fieldCounts = [:]
        drafts = [:]; rereading = [:]; revisions = [:]; summaries = [:]; references = [:]; changedElsewhere = []; stale = []; selectedField = [:]
        renaming = nil; renameRefusal = nil; preparing = nil; progress = nil; failure = nil; notice = nil; confirming = false
        meguriUnreachable = false
        // A section on screen reads the new collection's list at once.
        if appeared { reloadList() }
    }

    /// Waits for a save that runs (the app goes to the background or closes
    /// the collection): a save is never cancelled halfway by the app. Meguri
    /// Sync's job is asked to stop where MP7 allows (before the save nothing
    /// is saved; after it the upload is left pending), so leaving never waits
    /// for a whole upload.
    public func cancelAndDrain() async {
        leaving = true
        defer { leaving = false }
        if syncJobRunning { meguriSync?.cancelNotetypeStructureSave() }
        await saveTask?.value
    }

    // MARK: Tests and test hooks

    /// Waits until every read and save this model started has ended.
    func settle() async {
        for _ in 0..<50 { await Task.yield() }
        await saveTask?.value
        await reachabilityTask?.value
        for task in summaryTasks.values { await task.value }
        for task in detailTasks.values { await task.value }
        for task in issueTasks.values { await task.value }
        for task in blankTasks.values { await task.value }
        for task in referencesTask.values { await task.value }
        for task in countTasks.values.flatMap({ $0 }) { await task.value }
        await countsTask?.value
        for _ in 0..<50 { await Task.yield() }
    }

    /// Reads the list and the note type `id`, selected, before the route stages anything.
    @_spi(MeguriTestHooks) public func prepareForTestHooks(select id: Int64) async {
        phase = .loading
        if let names = try? await backend.list() {
            items = names.map { Item(id: $0.id, name: $0.name, notes: nil) }
            phase = .ready
        }
        if let counts = try? await backend.noteCounts() {
            for index in items.indices { items[index].notes = counts[items[index].id] ?? 0 }
        }
        selection = id
        await settle()
    }

    @_spi(MeguriTestHooks) public func settleForTestHooks() async { await settle() }

    /// Puts a draft in place, as the person's edits would have (test hooks' routes).
    @_spi(MeguriTestHooks) public func stage(_ id: Int64, _ change: (inout NoteTypeDraft) -> Void) {
        guard var draft = draft(id) else { return }
        change(&draft)
        drafts[id] = draft
        draftChanged(id)
    }

    /// Shows the Save sheet for the staged draft (test hooks' routes).
    @_spi(MeguriTestHooks) public func stageQuestion(_ id: Int64, summary: NotetypeDraftSummary) {
        guard let draft = drafts[id] else { return }
        ask(Question(notetypeId: id, name: draft.name, summary: summary, decision: .decide(summary, service: service())))
    }

    /// Shows a running save's progress for the staged draft, the note type
    /// locked (test hooks' routes).
    @_spi(MeguriTestHooks) public func stageProgress(_ id: Int64, stage: NoteTypeSyncStage?, completed: UInt64 = 0, total: UInt64? = nil) {
        progress = Progress(notetypeId: id, completed: completed, total: total, stage: stage)
    }
}

/// A15's Flip of one card type: the front and back it writes, and the front
/// it replaces (what Undo puts back).
public nonisolated struct NoteTypeFlip: Sendable, Hashable {
    public let front: String
    public let back: String
    public let previousFront: String

    public init(front: String, back: String, previousFront: String) {
        self.front = front
        self.back = back
        self.previousFront = previousFront
    }
}
