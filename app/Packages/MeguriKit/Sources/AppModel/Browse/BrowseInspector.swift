import CardRendering
import CoreBridge
import Foundation
import Observation

/// Browse's inspector (U273; `browser/workspace` V7, V12, V17; N21.3, N21.4,
/// N21.5; `browser/inspector` I2, I5, I8; `editor/edit-flows` W2, W4, W5):
/// the trailing pane beside the table. Held by `BrowseModel`, above the
/// layout switch.
///
/// - **What it shows** follows the selection: "No Selection", the summary of
///   several rows, or the one row's note in the round 5 editor (its draft
///   and field page), with its preview, Deck and Type rows and tags.
/// - **No flash.** A new row's note is read by id (pure reads) while the
///   note shown stays; it replaces that note only once read. A selection
///   that moves on meanwhile drops the reads of the rows it passed: one
///   loop runs at a time and takes the newest selection each turn.
/// - **Saving** (V12, R1223): when a field loses the focus, before another
///   row's note is shown, when Browse is left or the app goes to the
///   background. A save sends only what differs from what is stored (W4),
///   built from the shown note's own draft, never from the selection, so it
///   cannot reach another note; each is one "Edit Note" step. A failed save
///   keeps the note, its text and the row selected (W2).
/// - **Changes from elsewhere** (W5.2): the fields without an unsaved edit
///   take the stored values; the user's edited fields keep their text.
@Observable @MainActor
public final class BrowseInspector {
    public nonisolated enum Content: Equatable, Sendable {
        /// V7: "No Selection".
        case noSelection
        /// V7: two or more rows.
        case several
        /// V7, V12: one row's note (`note`).
        case note
        /// The one row could not be shown (its note went, a read failed).
        case unavailable
    }

    // MARK: State

    public private(set) var content: Content = .noSelection
    /// The note shown while `content` is `.note`; kept while the next one is read.
    public private(set) var note: BrowseInspectorNote?
    /// A read has run for half a second with nothing to show meanwhile.
    public private(set) var loading = false
    /// The selection's summary while `content` is `.several`.
    public var summary: BrowseSelectionSummary? {
        guard content == .several, let facts = commands.facts, facts.rows > 1 else { return nil }
        let deck = facts.deck.flatMap { id in decks.first { $0.id == id }?.fullName }
        return BrowseSelectionSummary(mode: facts.mode, rows: facts.rows, notes: facts.mode == .cards ? facts.notes : nil,
                                      deck: deck, decks: facts.decks.flatMap { $0 > 1 ? $0 : nil }, suspended: facts.suspended)
    }
    /// Every deck, for the Deck row.
    public private(set) var decks: [DeckNameEntry] = []
    /// W5.4's notice: the shown note was deleted with unsaved text in it.
    public private(set) var notice: String?
    public let preview = BrowseCardPreview()
    /// The Info tab's snapshot (U274, V13): the shown card's Card Info.
    public let cardInfo: CardInfoModel

    /// N21.4: the tab the pane shows for one row (kept on the device).
    public var tab: BrowseInspectorTab {
        get { device.tab ?? .note }
        set {
            guard newValue != tab else { return }
            device.tab = newValue
            saveDevice()
            cardInfo.active = newValue == .info
            followCardInfo()
        }
    }

    /// V7, V13: the card the Info tab shows: the one row's card, the one the
    /// Note tab's preview shows (in Notes mode the note's card with the lowest
    /// ordinal, or the one "Card <i> of <k>" chose); none for no row or
    /// several rows, which show as the Note tab shows them.
    public var infoCard: Int64? { content == .note ? note?.card?.id : nil }

    /// N21.3, I8: the person shows or hides the pane (kept on the device).
    public var presented: Bool {
        get { device.presented }
        set {
            guard newValue != device.presented else { return }
            device.presented = newValue
            saveDevice()
            // W2: hiding the pane saves first.
            if !newValue { saveNow() }
        }
    }
    /// N21.3: the window is under about 900 points: the pane hides itself
    /// first, and Return or a double click shows it over the table.
    public private(set) var narrow = false
    /// The pane shows over the table while the window is narrow.
    public private(set) var overlay = false
    public static let narrowWidth: Double = 900

    /// V12: the note types whose empty fields show (kept on the device).
    public func emptyFieldsOpen(_ notetype: Int64) -> Bool { device.emptyFieldsOpen.contains(notetype) }
    /// V12: the preview's header folds it away (kept on the device).
    public var previewFolded: Bool {
        get { device.previewFolded }
        set { guard newValue != device.previewFolded else { return }; device.previewFolded = newValue; saveDevice() }
    }

    // MARK: Seams

    @ObservationIgnored let backend: any BrowseInspectorBackend
    @ObservationIgnored private let results: BrowseResults
    @ObservationIgnored private let commands: BrowseCommands
    private var device: BrowseInspectorDevice
    @ObservationIgnored var store: (BrowseInspectorDevice) -> Void = { _ in }
    /// The open collection's generation, which drafts carry (S2).
    @ObservationIgnored public var generation: @MainActor () -> UInt64 = { 0 }
    /// Makes the field page for a draft (`FieldEditorHost` in production).
    @ObservationIgnored public var makeRuntime: (@MainActor (NoteEditorDraft, @escaping @MainActor () -> Bool) throws
        -> any BrowseFieldRuntime)?
    /// A save recorded its undo step: its seq and V8.3's title, "Edit Note".
    @ObservationIgnored public var stepRecorded: @MainActor (UInt64, String) -> Void = { _, _ in }
    /// The collection's media folder: the fields' thumbnails and sound chips.
    @ObservationIgnored public var media = MediaFolder(directory: nil)

    /// How long a read runs before the pane says it is loading.
    @ObservationIgnored public var loadingDelay: Duration = .milliseconds(500)

    // MARK: Internals

    /// Raised by every selection change: the loop takes the newest.
    @ObservationIgnored private var wanted: UInt64 = 0
    @ObservationIgnored private var loop: Task<Void, Never>?
    /// One save at a time; a loop waits for it.
    @ObservationIgnored private var saving: Task<SaveOutcome, Never>?
    /// The selection the note shown was read for, to go back to when its save fails.
    @ObservationIgnored private var shownFor: (token: UInt64, revision: UInt64, selection: Set<Int>)?
    /// The mode the note shown was read in.
    @ObservationIgnored private var shownMode: BrowseMode = .cards
    /// A field page that shows no note now, kept to show the next one in.
    @ObservationIgnored private var parked: (any BrowseFieldRuntime)?
    /// W1: own patches by seq; change sets wait while one is in flight.
    @ObservationIgnored private var patchInFlight = false
    @ObservationIgnored private var ownPatches: Set<UInt64> = []
    @ObservationIgnored private var heldChanges: [(seq: UInt64, changes: ChangeSet)] = []
    @ObservationIgnored private var reloadWanted = false
    /// Notes a change set named while no note was shown (one was being read).
    @ObservationIgnored private var changedWhileReading: Set<Int64> = []
    @ObservationIgnored private var changedAll = false
    @ObservationIgnored private var cardsWanted = false
    @ObservationIgnored private var epoch: UInt64 = 0
    @ObservationIgnored private var decksRead = false
    @ObservationIgnored private var loadingTask: Task<Void, Never>?

    init(backend: any BrowseInspectorBackend, results: BrowseResults, commands: BrowseCommands, device: BrowseInspectorDevice,
         cardInfo: any CardInfoReading = CoreCardInfoReader()) {
        self.backend = backend
        self.results = results
        self.commands = commands
        self.device = device
        self.cardInfo = CardInfoModel(reader: cardInfo)
        self.cardInfo.active = (device.tab ?? .note) == .info
    }

    /// V13: the Info tab reads the shown card while it shows; the Note tab
    /// leaves the last snapshot alone (no read for a tab not shown).
    public func followCardInfo() {
        guard tab == .info else { return }
        cardInfo.show(card: infoCard)
    }

    /// View › Card Info (⌥⌘I, V13): the pane shows with Info selected; with
    /// Info already showing, the pane hides.
    public func toggleCardInfo() {
        if shown, tab == .info {
            toggle()
        } else {
            show()
            tab = .info
        }
    }

    // MARK: Showing the pane (N21.3)

    /// The pane is on screen: beside the table, or over it while narrow.
    public var shown: Bool { narrow ? overlay : presented }

    /// The toolbar's toggle and View › Show/Hide Inspector.
    public func toggle() {
        if narrow {
            overlay.toggle()
            if !overlay { saveNow() }
        } else {
            presented.toggle()
        }
    }

    /// N21.3: Return or a double click on a row shows the pane.
    public func show() {
        if narrow { overlay = true } else { presented = true }
    }

    /// The window's width changed. Under 900 points the pane hides itself
    /// (it saves first, W2); wider again, the person's choice shows again.
    public func widthChanged(_ width: Double) {
        let next = width < Self.narrowWidth
        guard next != narrow else { return }
        let wasShown = shown
        narrow = next
        overlay = false
        if wasShown, !shown { saveNow() }
    }

    // MARK: Following the selection (V7, I2)

    /// The selection or the list it indexes changed (`BrowseResults`).
    func selectionChanged() {
        wanted &+= 1
        if loop == nil { startLoop() }
    }

    private func startLoop() {
        let epoch = epoch
        loop = Task {
            await follow(epoch: epoch)
            guard self.epoch == epoch else { return }
            loop = nil
            // A change that waited for the loop.
            if reloadWanted || cardsWanted { handleHeldChanges() }
        }
    }

    /// What the selection asks the pane to show.
    private enum Wanted: Equatable {
        case none, several
        case row(token: UInt64, revision: UInt64, index: Int, selection: Set<Int>)
    }

    private func wantedNow() -> Wanted {
        let selection = results.selection
        guard let list = results.list, !selection.isEmpty else { return .none }
        guard selection.count == 1, let index = selection.first else { return .several }
        return .row(token: list.token, revision: list.revision, index: index, selection: selection)
    }

    /// One turn per newest selection, until the selection holds still.
    private func follow(epoch: UInt64) async {
        var last: UInt64?
        while self.epoch == epoch, last != wanted {
            let turn = wanted
            last = turn
            switch wantedNow() {
            case .none:
                guard await leaveNote(epoch: epoch) else { return }
                if self.epoch == epoch, turn == wanted { show(.noSelection) }
            case .several:
                guard await leaveNote(epoch: epoch) else { return }
                if self.epoch == epoch, turn == wanted { show(.several) }
                await readDecksOnce()
            case .row(let token, let revision, let index, let selection):
                await showRow(index: index, for: (token, revision, selection), turn: turn, epoch: epoch)
            }
        }
    }

    private func show(_ next: Content) {
        endLoading()
        if next != .note { preview.clear() }
        if content != next { content = next }
        followCardInfo()
    }

    /// The one row's note: read by id while the note shown stays; then, if
    /// it is another note, the shown note is saved and the new one opens.
    private func showRow(index: Int, for selection: (token: UInt64, revision: UInt64, selection: Set<Int>), turn: UInt64,
                         epoch: UInt64) async {
        beginLoading(epoch: epoch)
        let mode = results.list?.mode ?? .cards
        guard let ids = await results.rowIDs(at: index), self.epoch == epoch else {
            if self.epoch == epoch, turn == wanted, await leaveNote(epoch: epoch), turn == wanted { show(.unavailable) }
            return
        }
        guard turn == wanted else { return }
        // The same note (another of its cards, a new list or mode that keeps it): no save, no reopen.
        if let note, note.noteID == ids.note, note.runtime != nil {
            await readDecksOnce()
            if let card = ids.card {
                // Cards mode: the row's card.
                if let at = note.cards.firstIndex(where: { $0.id == card }) { note.cardIndex = at }
                else { await refreshCards(note, card: card, epoch: epoch) }
            } else if shownMode != mode {
                // Into Notes mode: V7's card, the lowest ordinal; "Card <i> of <k>" keeps its choice after.
                note.cardIndex = 0
            }
            guard self.epoch == epoch, turn == wanted else { return }
            shownFor = selection
            shownMode = mode
            // A hold a save or a reload owns is theirs to release.
            if saving == nil, let runtime = note.runtime, runtime.editable, runtime.held { try? await runtime.release() }
            show(.note)
            if preview.card != note.card?.id { showPreview() }
            return
        }
        // The reads (pure, so the save cannot change what they read of
        // another note), then the shown note's save.
        changedWhileReading = []; changedAll = false
        let loaded = await readNote(ids, mode: mode)
        guard self.epoch == epoch, turn == wanted else { return }
        guard await leaveNote(epoch: epoch), self.epoch == epoch, turn == wanted else { return }
        guard let loaded else { return show(.unavailable) }
        shownMode = mode
        await mount(loaded, for: selection, epoch: epoch)
    }

    private struct Loaded {
        let note: NoteRecord
        let notetype: NotetypeRecord
        let cards: [NoteCard]
        let cardIndex: Int
    }

    private func readNote(_ ids: (card: Int64?, note: Int64), mode: BrowseMode) async -> Loaded? {
        do {
            // Reads by id on the query lane: a few milliseconds each.
            let record = try await backend.note(ids.note)
            let list = try await backend.cards(ofNote: ids.note)
            let type = try await backend.notetype(record.notetypeId)
            await readDecksOnce()
            // V7: in Notes mode the note's card with the lowest ordinal, then the lowest id.
            let index = mode == .cards ? (ids.card.flatMap { card in list.firstIndex { $0.id == card } } ?? 0) : 0
            return Loaded(note: record, notetype: type, cards: list, cardIndex: index)
        } catch {
            return nil
        }
    }

    /// Opens `loaded` in the parked page (or a new one); the page is the
    /// round 5 editor's, so the fields show as Add and Edit Note show them.
    private func mount(_ loaded: Loaded, for selection: (token: UInt64, revision: UInt64, selection: Set<Int>), epoch: UInt64) async {
        let deck = loaded.cards.indices.contains(loaded.cardIndex) ? deckEntry(loaded.cards[loaded.cardIndex].deckId) : nil
        let draft: NoteEditorDraft
        do {
            draft = try NoteEditorDraft(editing: loaded.note, notetype: loaded.notetype,
                                        deck: deck ?? DeckNameEntry(id: 0, fullName: "", filtered: false), generation: generation())
        } catch {
            let shown = BrowseInspectorNote(noteID: loaded.note.id, notetype: loaded.notetype, draft: try! NoteEditorDraft.placeholder(),
                                            cards: loaded.cards, cardIndex: loaded.cardIndex, emptyFields: [])
            shown.error = String(describing: error)
            note = shown
            shownFor = selection
            show(.note)
            return
        }
        let empty = loaded.note.fields.indices.filter { FieldEmptiness.isEmpty(loaded.note.fields[$0]) }
        if !emptyFieldsOpen(loaded.notetype.id) { draft.hiddenFields = Set(empty) }
        let shown = BrowseInspectorNote(noteID: loaded.note.id, notetype: loaded.notetype, draft: draft, cards: loaded.cards,
                                        cardIndex: loaded.cardIndex, emptyFields: empty)
        // The page stays current while it shows this draft, and while it waits,
        // parked, for the next note: its messages then must not end it.
        let current: @MainActor () -> Bool = { [weak self, weak draft] in
            guard let self, let draft, self.epoch == epoch else { return false }
            return self.note?.draft === draft || self.parked?.draft === draft
        }
        let runtime: (any BrowseFieldRuntime)?
        do {
            if let parked, let next = parked.successor(for: draft, isCurrent: current) as? any BrowseFieldRuntime {
                runtime = next
            } else {
                parked?.shutDown()
                runtime = try makeRuntime?(draft, current)
            }
        } catch {
            runtime = nil
            shown.error = String(describing: error)
        }
        parked = nil
        ownPatches = []
        if changedAll || changedWhileReading.contains(loaded.note.id) { reloadWanted = true }
        changedWhileReading = []; changedAll = false
        shown.runtime = runtime
        runtime?.onFieldBlur = { [weak self, weak shown] in
            guard let self, let shown, self.note === shown else { return }
            self.saveNow()
        }
        // Shown now, so a new page loads in the pane; the page it reuses shows the old fields until it opens these.
        note = shown
        shownFor = selection
        show(.note)
        do { try await runtime?.start() } catch {
            guard self.epoch == epoch, note === shown else { return }
            shown.error = String(describing: error)
        }
        guard self.epoch == epoch, note === shown else { return }
        showPreview()
        // A change that arrived while the page opened.
        if reloadWanted || cardsWanted { handleHeldChanges() }
    }

    private func showPreview() {
        guard let card = note?.card?.id else { return preview.clear() }
        preview.show(card: card)
    }

    /// Before another note shows, nothing does, or the pane hides: the
    /// shown note is saved. False when its save failed: the note stays,
    /// with its text, and the selection goes back to its row (W2).
    private func leaveNote(epoch: UInt64) async -> Bool {
        guard let note else { return true }
        let outcome = await save(note, hold: true)
        guard self.epoch == epoch else { return false }
        switch outcome {
        case .failed:
            try? await note.runtime?.release()
            if let shownFor, results.list?.token == shownFor.token, results.list?.revision == shownFor.revision,
               results.selection != shownFor.selection {
                results.select(shownFor.selection)
            }
            return false
        case .deleted, .saved, .unchanged:
            guard self.note === note else { return true }
            parked = note.runtime
            parked?.onFieldBlur = nil
            note.runtime = nil
            self.note = nil
            return true
        }
    }

    // MARK: Saving (V12, R1223)

    enum SaveOutcome: Equatable { case unchanged, saved, failed, deleted }

    /// V12: a field lost the focus, the pane hid, Browse was left.
    public func saveNow() {
        guard let note else { return }
        Task { _ = await save(note, hold: false) }
    }

    /// The shown note holds input or a change not saved yet.
    var unsavedEdit: Bool { note?.runtime != nil && note?.draft.hasChanges == true }

    /// Browse is left (V12): the shown note is saved.
    public func browseLeft() { saveNow() }

    /// The app goes to the background (W3): the shown note is saved, waited for.
    /// False when the note's save failed: its text stays in the inspector.
    @discardableResult public func flushForBackground() async -> Bool {
        guard let note else { return true }
        return await save(note, hold: false) != .failed
    }

    /// The page's pending input committed, then one patch of what differs
    /// from what is stored, or nothing. One at a time.
    private func save(_ note: BrowseInspectorNote, hold: Bool) async -> SaveOutcome {
        await saved()
        guard let runtime = note.runtime, note.error == nil else { return .unchanged }
        let work = Task { () -> SaveOutcome in
            do { try await runtime.flush(hold: hold) } catch {
                // A page that still takes input but could not flush may hold
                // input not committed yet: the note stays, nothing is dropped.
                if runtime.editable { return .failed }
                // A page that went (its process ended) still saves what it committed.
                guard note.draft.committedPatch() != nil else { return .unchanged }
            }
            guard let patch = note.draft.committedPatch() else { return .unchanged }
            patchInFlight = true
            defer { patchInFlight = false }
            do {
                let result = try await backend.save(patch)
                ownPatches.insert(result.summary.seq)
                note.draft.acknowledgeSaved(patch)
                note.saveFailure = nil
                if result.written, result.summary.undoName != nil {
                    stepRecorded(result.summary.seq, BrowseStepNames.display(.editNote, count: nil) ?? "Edit Note")
                }
                return .saved
            } catch {
                switch error as? BridgeError {
                case .commitCompleted?, .mediaCommitPending?:
                    note.draft.acknowledgeSaved(patch)
                    return .saved
                case .noteNotFound?:
                    noteGone(note, droppingChanges: true)
                    return .deleted
                case .stale?:
                    // S2's Stale path: the note type changed under the edit. The
                    // changed fields are sent again on the new layout, matched by
                    // field id (then name); the note then opens again on it.
                    if await resendOnNewLayout(note, patch) { return .saved }
                    note.saveFailure = .notSaved
                case .invalidArgument?:
                    note.saveFailure = .checkDatabase
                default:
                    note.saveFailure = .notSaved
                }
                return .failed
            }
        }
        let epoch = epoch
        saving = work
        let outcome = await work.value
        // A save from before a reset leaves the newer state alone.
        guard self.epoch == epoch else { return outcome }
        if saving == work { saving = nil }
        handleHeldChanges()
        if outcome == .saved, self.note === note { showPreview() }
        return outcome
    }

    private func resendOnNewLayout(_ note: BrowseInspectorNote, _ patch: NoteFieldPatch) async -> Bool {
        guard let stored = try? await backend.note(note.noteID),
              let type = try? await backend.notetype(stored.notetypeId) else { return false }
        var fields: [Int: String] = [:]
        for (ord, html) in patch.fields {
            let old = note.notetype.fields[ord]
            guard let new = type.fields.first(where: { $0.id != nil && $0.id == old.id }) ?? type.fields.first(where: { $0.name == old.name })
            else { return false }
            // Two edits on one field of the new layout: nothing is sent.
            guard fields[Int(new.ord)] == nil else { return false }
            fields[Int(new.ord)] = html
        }
        let again = NoteFieldPatch(id: note.noteID, notetype: type, fields: fields, tags: patch.tags, generation: patch.generation)
        guard let result = try? await backend.save(again) else { return false }
        ownPatches.insert(result.summary.seq)
        note.draft.acknowledgeSaved(patch)
        note.saveFailure = nil
        if result.written, result.summary.undoName != nil { stepRecorded(result.summary.seq, "Edit Note") }
        reloadWanted = true
        return true
    }

    /// Waits until no save or reload runs. A finished one is cleared here
    /// too: awaiting a finished task does not suspend, so its owner might
    /// not get the main actor back to clear it.
    private func saved() async {
        while let current = saving {
            _ = await current.value
            if saving == current { saving = nil }
        }
    }

    // MARK: The Deck row and Edit Note (V12, N21.8)

    /// V12: another deck runs Change Deck on the shown card (Cards mode) or
    /// on every card of the note (Notes mode), after a save (W2).
    public func chooseDeck(_ id: Int64) {
        // The rows as they are now: a list that moves on meanwhile makes the core refuse them (K1).
        guard let note, note.card?.deckId != id, let shownFor, results.list?.token == shownFor.token,
              results.list?.revision == shownFor.revision, results.selection == shownFor.selection,
              let target = results.target() else { return }
        Task {
            guard await save(note, hold: false) != .failed, self.note === note else { return }
            commands.changeDeck(to: id, on: target)
        }
    }

    /// N21.8: Note › Edit Note in Browse: the pane shows, its first field
    /// takes the focus.
    public var canEditNote: Bool { content == .note && note?.runtime?.editable == true }

    public func editNote() {
        guard canEditNote, let note, let runtime = note.runtime else { return }
        show()
        let first = note.draft.fields.indices.first { !note.draft.hiddenFields.contains($0) } ?? 0
        Task { try? await runtime.focusField(first) }
    }

    /// V12: "<e> Empty Fields" opens: every field shows, in field order, and
    /// stays so for that note type on this device until closed.
    public func setEmptyFieldsOpen(_ open: Bool) {
        guard let note else { return }
        if open { device.emptyFieldsOpen.insert(note.notetype.id) } else { device.emptyFieldsOpen.remove(note.notetype.id) }
        saveDevice()
        // A field filled since it was shown stays in view.
        let hidden: Set<Int> = open ? [] : Set(note.emptyFields.filter { FieldEmptiness.isEmpty(note.draft.values[$0]) })
        note.draft.hiddenFields = hidden
        Task { try? await note.runtime?.setHiddenFields(hidden) }
    }

    /// V12: "Card <i> of <k>" steps through the note's cards.
    public func showCard(_ index: Int) {
        guard let note, note.cards.indices.contains(index), index != note.cardIndex else { return }
        note.cardIndex = index
        showPreview()
        followCardInfo()
    }

    /// The tags the person left in the token field (`editor/tag-editor`):
    /// saved at once, as a field is when it loses the focus.
    public func setTags(_ tags: [String], for shown: BrowseInspectorNote) {
        // The field the tags were typed in belongs to `shown`: never another note.
        guard let note, note === shown, tags != note.draft.tags else { return }
        note.draft.setTags(tags)
        saveNow()
    }

    // MARK: Changes from elsewhere (W1, W5)

    /// Every committed change set, from `BrowseModel`.
    func changesCommitted(_ changes: ChangeSet, seq: UInt64) {
        cardInfo.changesCommitted(changes)
        if changes.wholeCollection || !changes.decks.isEmpty { decksRead = false; Task { await readDecksOnce() } }
        // A note being read now may be one of these: it reloads once it shows.
        if loop != nil {
            changedWhileReading.formUnion(changes.notes.map(\.id))
            if changes.wholeCollection || !changes.notetypes.isEmpty { changedAll = true }
        }
        guard let note else { return }
        let touchesNote = changes.wholeCollection || changes.notes.contains { $0.id == note.noteID }
            || changes.notetypes.contains { $0.id == note.notetype.id }
        let touchesCards = changes.wholeCollection || changes.cards.contains { card in note.cards.contains { $0.id == card.id } }
        guard touchesNote || touchesCards else { return }
        heldChanges.append((seq, changes))
        if !patchInFlight, saving == nil { handleHeldChanges() }
    }

    private func handleHeldChanges() {
        let events = heldChanges.sorted { $0.seq < $1.seq }
        heldChanges.removeAll()
        let foreign = events.filter { !ownPatches.contains($0.seq) }
        guard let note else { return }
        if foreign.contains(where: { event in event.changes.notes.contains { $0.id == note.noteID && $0.change == .removed } }) {
            return noteGone(note, droppingChanges: note.draft.hasChanges)
        }
        if foreign.contains(where: { event in
            event.changes.wholeCollection || event.changes.notes.contains { $0.id == note.noteID }
                || event.changes.notetypes.contains { $0.id == note.notetype.id }
        }) { reloadWanted = true }
        // The note's cards (their deck, suspension, flags) changed, own saves included.
        if events.contains(where: { event in event.changes.wholeCollection || event.changes.cards.contains { card in
            note.cards.contains { $0.id == card.id } } }) { cardsWanted = true }
        guard reloadWanted || cardsWanted, note.runtime?.editable == true, loop == nil, saving == nil else { return }
        let reload = reloadWanted
        reloadWanted = false
        cardsWanted = false
        let epoch = epoch
        Task {
            if reload {
                // One at a time with the saves: a save never lands between the
                // reload's read and its loading of the unedited fields.
                await saved()
                let work = Task { () -> SaveOutcome in await self.reload(note, epoch: epoch); return .unchanged }
                saving = work
                _ = await work.value
                if saving == work { saving = nil }
                if reloadWanted || cardsWanted { handleHeldChanges() }
            } else {
                await refreshCards(note, card: note.card?.id, epoch: epoch)
                if self.note === note { showPreview() }
            }
        }
    }

    /// W5.2: flush, read the note again, then every field without an unsaved
    /// edit takes the stored value; the edited fields keep the user's text.
    private func reload(_ note: BrowseInspectorNote, epoch: UInt64) async {
        guard let runtime = note.runtime else { return }
        var held = false
        do {
            try await runtime.flush(hold: true)
            held = true
            let stored = try await backend.note(note.noteID)
            let notetype = try await backend.notetype(stored.notetypeId)
            guard self.epoch == epoch, self.note === note else { if held { try? await runtime.release() }; return }
            let layout = { (type: NotetypeRecord) in type.fields.map { "\($0.ord)|\(String(describing: $0.id))|\($0.name)" } }
            if stored.notetypeId == note.notetype.id, stored.fields.count == note.draft.fields.count,
               layout(notetype) == layout(note.notetype) {
                let ords = note.draft.reloadUnchanged(stored: stored.fields, tags: stored.tags)
                if !ords.isEmpty { try await runtime.reload(fields: ords) }
                try? await runtime.release()
                held = false
            } else if !note.draft.hasChanges {
                // Another layout or note type, nothing unsaved: the note opens again,
                // the page held until the new draft opens in it (no input between).
                guard let selection = shownFor else { try? await runtime.release(); return }
                let mode = results.list?.mode ?? .cards
                if let loaded = await readNote((note.card?.id, note.noteID), mode: mode), self.epoch == epoch, self.note === note,
                   !note.draft.hasChanges {
                    parked = note.runtime
                    note.runtime = nil
                    self.note = nil
                    await mount(loaded, for: selection, epoch: epoch)
                } else {
                    try? await runtime.release()
                }
                return
            } else {
                // Unsaved text on another layout: the next save is refused (Stale) and says so.
                try? await runtime.release()
                held = false
            }
            await refreshCards(note, card: note.card?.id, epoch: epoch)
            if self.note === note { showPreview() }
        } catch {
            if held { try? await runtime.release() }
            if case .noteNotFound? = error as? BridgeError, self.note === note { noteGone(note, droppingChanges: note.draft.hasChanges) }
        }
    }

    /// W5.4: the note was deleted: the editor closes without an alert; a
    /// notice says so when unsaved text was dropped.
    private func noteGone(_ note: BrowseInspectorNote, droppingChanges: Bool) {
        guard self.note === note else { return }
        if droppingChanges { notice = "This note was deleted, so your last changes couldn’t be saved." }
        parked = note.runtime
        parked?.onFieldBlur = nil
        note.runtime = nil
        self.note = nil
        show(.unavailable)
        // The row leaves the table with Browse's own refresh, which moves the selection.
    }

    public func dismissNotice() { notice = nil }

    private func refreshCards(_ note: BrowseInspectorNote, card: Int64?, epoch: UInt64) async {
        let before = note.card?.id
        guard let cards = try? await backend.cards(ofNote: note.noteID), self.epoch == epoch, self.note === note else { return }
        // The card asked for (the selected row's), unless the person chose
        // another one ("Card <i> of <k>") while the list was read.
        let chosen = note.card?.id
        let wanted = chosen != before ? chosen : card ?? chosen
        note.cards = cards
        note.cardIndex = wanted.flatMap { id in cards.firstIndex { $0.id == id } } ?? 0
        followCardInfo()
    }

    // MARK: Decks

    private func readDecksOnce() async {
        guard !decksRead else { return }
        decksRead = true
        let epoch = epoch
        let names = try? await backend.deckNames()
        guard self.epoch == epoch else { return }
        if let names { decks = names } else { decksRead = false }
    }

    /// The deck entry for `id`.
    public func deckEntry(_ id: Int64) -> DeckNameEntry? { decks.first { $0.id == id } }

    // MARK: Loading (V17)

    private func beginLoading(epoch: UInt64) {
        guard loadingTask == nil else { return }
        let delay = loadingDelay
        loadingTask = Task {
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled, self.epoch == epoch, loop != nil else { return }
            // Only an empty pane says it is loading; a note shown stays shown.
            if note == nil || content != .note { loading = true }
        }
    }

    private func endLoading() {
        loadingTask?.cancel()
        loadingTask = nil
        if loading { loading = false }
    }

    // MARK: The collection

    /// The collection went or opened again: nothing shown before stays.
    func reset() {
        epoch &+= 1
        loop?.cancel(); loop = nil
        saving = nil
        endLoading()
        note?.runtime?.shutDown()
        parked?.shutDown(); parked = nil
        note = nil
        content = .noSelection
        heldChanges = []; ownPatches = []; patchInFlight = false; reloadWanted = false; cardsWanted = false
        decks = []; decksRead = false
        shownFor = nil
        notice = nil
        preview.close()
        cardInfo.reset()
        if !results.selection.isEmpty { selectionChanged() }
    }

    private func saveDevice() { store(device) }

    /// Waits until the inspector has followed the selection (tests).
    func settled() async {
        while let loop { await loop.value }
        await saved()
    }
}

/// What the inspector keeps on the device (I8, V12; never in the collection).
nonisolated public struct BrowseInspectorDevice: Codable, Equatable, Sendable {
    public var presented = true
    public var previewFolded = false
    public var emptyFieldsOpen: Set<Int64> = []
    /// The tab chosen last (U274); nil before, the Note tab.
    public var tab: BrowseInspectorTab?
    public init() {}
}

/// N21.4's Note | Info.
public nonisolated enum BrowseInspectorTab: String, Codable, Sendable, CaseIterable {
    case note, info
}

extension NoteEditorDraft {
    /// A draft for a note the editor cannot show (its note type is not
    /// supported), so the pane can say why.
    static func placeholder() throws -> NoteEditorDraft {
        let field = NotetypeField(ord: 0, id: nil, name: "", sticky: false, rtl: false, fontName: "", fontSize: 0, description: "",
                                  plainText: false, collapsed: false, excludeFromSearch: false)
        return try NoteEditorDraft(generation: 0, notetype: NotetypeRecord(id: 0, name: "", kind: 0, sortField: 0, css: "", base: [],
                                   fields: [field], templates: []), deck: DeckNameEntry(id: 0, fullName: "", filtered: false))
    }
}
