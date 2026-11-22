import CoreBridge
import Foundation
import Observation

/// The Note Types section (`notetypes/workspace` W2 to W4, W13;
/// `notetypes/draft-save` D1 to D5, D8; layout N22.1, N22.2, N22.5): the
/// list with its counts and filter, one draft per note type that survives
/// leaving the section, the Fields tab's edits with the editor's own undo,
/// and Save with its one sheet. Held by `ReviewCollection` above the layout
/// switch. Nothing here writes before Save.
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

    /// A running save (D5 step 5): "Updating <i> of <n> notes".
    public nonisolated struct Progress: Sendable, Hashable {
        public let notetypeId: Int64
        public var completed: UInt64 = 0
        public var total: UInt64?
    }

    public nonisolated enum Failure: Sendable, Hashable {
        /// The save wrote nothing (D5 step 6).
        case notSaved
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
    public var tab: Tab = .fields
    /// iPad: the field whose options show in a popover.
    public var optionsField: Int?
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
    public private(set) var question: Question?
    /// The confirm button of the shown question is waiting for D5 step 4's read.
    public private(set) var confirming = false
    public private(set) var preparing: Int64?
    public private(set) var progress: Progress?
    public var failure: Failure?
    /// "Discard changes?" for Revert (D1).
    public var confirmingRevert = false

    private var drafts: [Int64: NoteTypeDraft] = [:]
    private var revisions: [Int64: UInt64] = [:]
    private var summaries: [Int64: (revision: UInt64, summary: NotetypeDraftSummary)] = [:]
    private var references: [Int64: (texts: [String], references: [[String]])] = [:]
    private var listToken: UInt64 = 0
    @ObservationIgnored private var summaryTasks: [Int64: Task<Void, Never>] = [:]
    @ObservationIgnored private var detailTasks: [Int64: Task<Void, Never>] = [:]
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
    /// D6's Save and Upload: Meguri Sync's structure save job (U269, U285).
    /// Not built yet: with no job, the sheet's Save and Upload is disabled
    /// and nothing is saved on this device alone (D3: never D5's local save
    /// for a schema-classified save with a ready binding).
    /// The job runs with the note type locked like a local save; it returns
    /// when the save committed and throws the save's refusal otherwise
    /// (`LossesChanged`, `Stale`), which is handled as D5 step 6 says.
    @ObservationIgnored public var meguriSyncSave: (@MainActor (NotetypeStructureDraft, NotetypeDraftSummary) async throws -> Void)?
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
        drafts[id] ?? records[id].map(NoteTypeDraft.init)
    }

    public var selectedDraft: NoteTypeDraft? { selection.flatMap(draft) }

    private func selectionChanged() {
        renaming = nil
        renameRefusal = nil
        guard let id = selection else { return }
        loadDetail(id)
    }

    private func loadDetail(_ id: Int64, force: Bool = false) {
        // A cached note type whose counts were dropped (its notes changed) reads them again.
        if !force, records[id] != nil, !recordFailed.contains(id), typeCounts[id] == nil || fieldCounts[id] == nil {
            readCounts(id)
        }
        guard force || records[id] == nil || recordFailed.contains(id) else { return }
        detailTasks[id]?.cancel()
        recordFailed.remove(id)
        let epoch = epoch
        detailTasks[id] = Task {
            do {
                let record = try await backend.read(id)
                guard !Task.isCancelled, self.epoch == epoch else { return }
                records[id] = record
                if drafts[id] == nil, selectedField[id] == nil { selectedField[id] = record.fields.first.map { Int($0.ord) } }
                readCounts(id)
                await readReferences(id)
            } catch {
                guard !Task.isCancelled else { return }
                if records[id] == nil { recordFailed.insert(id) }
            }
        }
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
        guard let texts = draft(id)?.referenceTexts, references[id]?.texts != texts else { return }
        guard let refs = try? await backend.fieldReferences(texts), draft(id)?.referenceTexts == texts else { return }
        references[id] = (texts, refs)
    }

    /// W1's Used On for each row of the note type, by card type position;
    /// nil until the references are read.
    public func usedOn(_ id: Int64) -> [Int: [Int]]? {
        guard let draft = draft(id), let read = references[id], read.texts == draft.referenceTexts else { return nil }
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
        progress?.notetypeId != id && preparing != id && !(question?.notetypeId == id)
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

    // MARK: The editor's own undo (W13)

    /// The draft step Edit › Undo names while Note Types shows ("Delete
    /// Field"); nil when the draft has none, and the collection's Undo applies.
    public var undoStep: NoteTypeDraftStep? { selection.flatMap { id in editable(id) ? drafts[id]?.undoStep : nil } }
    public var redoStep: NoteTypeDraftStep? { selection.flatMap { id in editable(id) ? drafts[id]?.redoStep : nil } }

    public func undo() {
        guard let id = selection, editable(id), drafts[id]?.undo() != nil else { return }
        renaming = nil; renameRefusal = nil
        draftChanged(id)
    }

    public func redo() {
        guard let id = selection, editable(id), drafts[id]?.redo() != nil else { return }
        renaming = nil; renameRefusal = nil
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
        summaryTasks[id]?.cancel()
        guard let draft = drafts[id], draft.isEdited else { summaries[id] = nil; return }
        let revision = revisions[id] ?? 0
        let structure = draft.structureDraft()
        let delay = summaryDelay
        let epoch = epoch
        summaryTasks[id] = Task {
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled, let summary = try? await backend.summary(structure), !Task.isCancelled,
                  self.epoch == epoch, revisions[id] == revision else { return }
            summaries[id] = (revision, summary)
        }
        Task { await readReferences(id) }
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
            && !saveRunning && (renameRefusal == nil || !pendingEdits.isEmpty) && writable()
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
            let summary: NotetypeDraftSummary
            do { summary = try await backend.summary(draft.structureDraft()) } catch { refused(id, error); return }
            guard revisions[id] == revision else { return }
            let decision = NoteTypeSaveDecision.decide(summary, service: service())
            if decision.asks {
                ask(Question(notetypeId: id, name: draft.name, summary: summary, decision: decision))
                return
            }
            preparing = nil
            await submit(id, draft: draft, confirmed: .init(), route: decision.route, summary: summary)
        }
    }

    /// Meguri Sync's structure save job exists (U269); until it does, the
    /// sheet says why Save and Upload is off.
    public var canSaveWithMeguriSync: Bool { meguriSyncSave != nil }

    /// The sheet's confirm button is enabled: a local save, or Meguri
    /// Sync's job once it exists.
    public var canConfirm: Bool {
        guard let question, !confirming else { return false }
        return question.decision.route == .local || meguriSyncSave != nil
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
            closeQuestion()
            await submit(id, draft: draft, confirmed: confirmed, route: route, summary: latest)
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
        self.question = question
        if !presentQuestion() {
            self.question = nil
            failure = .notSaved
        }
    }

    private func closeQuestion() {
        question = nil
        dismissQuestion()
    }

    private func submit(_ id: Int64, draft: NoteTypeDraft, confirmed: NotetypeConfirmedLosses, route: NoteTypeSaveDecision.Route,
                        summary: NotetypeDraftSummary) async {
        let meguriSyncSave = meguriSyncSave
        // D6: Meguri Sync's job (U269) owns the round, the save and the upload.
        guard route == .local || meguriSyncSave != nil else { return }
        guard writable() else { failure = .notSaved; return }
        // D5 step 3's deck creations: the Fields tab sets no deck override.
        let epoch = epoch
        progress = Progress(notetypeId: id)
        do {
            if route == .local {
                try await backend.save(draft.structureDraft(confirmed: confirmed)) { [weak self] completed, total in
                    guard self?.progress?.notetypeId == id else { return }
                    self?.progress?.completed = completed
                    self?.progress?.total = total
                }
            } else if let meguriSyncSave {
                try await meguriSyncSave(draft.structureDraft(confirmed: confirmed), summary)
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
        drafts[id] = nil
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
            if let id = selection, records[id] != nil { readCounts(id) }
            // Other loaded note types read theirs when they are shown again; reads
            // already running are cancelled, so no pre-change count lands.
            for id in records.keys where id != selection {
                for task in countTasks[id] ?? [] { task.cancel() }
                countTasks[id] = nil
                fieldCounts[id] = nil; typeCounts[id] = nil
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
        let touched = changes.wholeCollection ? Set(records.keys) : Set(changes.notetypes.map(\.id))
        for id in touched where records[id] != nil {
            if notesMoved || changes.wholeCollection || changes.notetypes.contains(where: { $0.id == id }) {
                typeCounts[id] = nil
                fieldCounts[id] = nil
            }
            recheck(id)
        }
        if notesMoved, !changes.wholeCollection {
            for id in records.keys where !touched.contains(id) { readCounts(id) }
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
        summaryTasks = [:]; detailTasks = [:]; countTasks = [:]; fieldCountsBase = [:]; renameTokens = [:]
        if question != nil { closeQuestion() }
        phase = .idle; items = []; selection = nil; records = [:]; recordFailed = []; typeCounts = [:]; fieldCounts = [:]
        drafts = [:]; revisions = [:]; summaries = [:]; references = [:]; changedElsewhere = []; stale = []; selectedField = [:]
        renaming = nil; renameRefusal = nil; preparing = nil; progress = nil; failure = nil; confirming = false
        // A section on screen reads the new collection's list at once.
        if appeared { reloadList() }
    }

    /// Waits for a save that runs (the app goes to the background or closes
    /// the collection): a save is never cancelled halfway by the app.
    public func cancelAndDrain() async {
        await saveTask?.value
    }

    // MARK: Tests and test hooks

    /// Waits until every read and save this model started has ended.
    func settle() async {
        for _ in 0..<50 { await Task.yield() }
        await saveTask?.value
        for task in summaryTasks.values { await task.value }
        for task in detailTasks.values { await task.value }
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
}
