import CoreBridge
import Foundation
import Observation

/// Edit Note during a review (`review/edit-current`, N20.8): round 5's Add
/// Note form on the current card's note. The form is a draft until Done,
/// which sends one field patch; Cancel drops it, asking first when it holds
/// a change. Collection-owned like Add, so a layout change keeps the draft.
@Observable @MainActor
public final class EditNoteFlow {
    public typealias RuntimeFactory = AddNoteFlow.RuntimeFactory
    /// "<deck> · <note type>": the card's deck by its last name component.
    public struct Subtitle: Equatable, Sendable {
        public let deck: String
        public let notetype: String
    }
    /// W9's failures of Done's patch; the text stays in the form.
    public enum SaveFailure: Equatable, Sendable {
        case checkDatabase
        case notSaved
    }
    public private(set) var draft: NoteEditorDraft?
    public private(set) var runtime: (any AddNoteFieldRuntime)?
    public private(set) var isPresented = false
    public private(set) var opening = false
    public private(set) var saving = false
    public private(set) var guardingClose = false
    public private(set) var subtitle: Subtitle?
    public private(set) var cardID: Int64?
    public private(set) var saveFailure: SaveFailure?
    public private(set) var error: String?
    public private(set) var discardConfirmation: UUID?
    @ObservationIgnored private weak var owner: ReviewCollection?
    @ObservationIgnored private weak var session: ReviewCoordinator?
    @ObservationIgnored private var epoch = UUID()
    @ObservationIgnored private var saveTask: Task<Bool, Never>?
    @ObservationIgnored private var closeTask: Task<Bool, Never>?
    @ObservationIgnored private var discardAnswer: CheckedContinuation<Bool, Never>?
    // W1: own patches are matched by op seq; events wait while one is in flight.
    @ObservationIgnored private var patchInFlight = false
    @ObservationIgnored private var ownPatches: Set<UInt64> = []
    @ObservationIgnored private var heldChanges: [(seq: UInt64, changes: ChangeSet)] = []
    @ObservationIgnored private var reloadTask: Task<Void, Never>?
    @ObservationIgnored private var reloadPending = false
    // Hold the actual durable reply, never substitute a mutation or a writer.
    var afterPatching: (@MainActor (NoteMutation) async -> Void)?
    var reloadCompletion: Task<Void, Never>? { reloadTask }

    init(owner: ReviewCollection) { self.owner = owner }

    /// The pencil, Card › Edit Note, the context menu and E: a card is
    /// presented, on either side, and nothing else owns the screen.
    public var canOpen: Bool {
        guard let owner, let review = owner.review else { return false }
        return !isPresented && !opening && closeTask == nil && review.canEditNote &&
            owner.canBeginAddAction && owner.navigation.sheet == nil && owner.reviewIsActive
    }

    /// Done: enabled once the page is editable; with no change it only closes.
    public var canSave: Bool {
        isPresented && runtime != nil && !opening && !saving && !guardingClose
    }

    /// E1: reads the current card's note, its type and deck, holds the review
    /// (E2) and mounts the field page. Opening writes nothing.
    @discardableResult public func open(makeRuntime: @escaping RuntimeFactory) async -> Bool {
        guard canOpen, let owner, let review = owner.review, let item = review.item else { return false }
        opening = true; error = nil; saveFailure = nil
        defer { opening = false }
        let generation = owner.generation
        guard review.beginEditingNote() else { return false }
        let expected = UUID(); epoch = expected
        var host: (any AddNoteFieldRuntime)?
        do {
            let notes = CoreNoteService()
            let note = try await notes.readNote(id: item.noteId)
            let notetype = try await CoreNotetypeService().readNotetype(id: note.notetypeId)
            let deckID = try await notes.cardDeck(cardId: item.cardId)
            let names = try await CoreDeckService().allDeckNames()
            guard epoch == expected, names.generation == generation, owner.ownsAddGeneration(generation),
                  owner.review === review, review.editingNote else { throw Self.ownerChanged }
            let deck = names.decks.first { $0.id == deckID } ?? DeckNameEntry(id: deckID, fullName: "", filtered: false)
            let draft = try NoteEditorDraft(editing: note, notetype: notetype, deck: deck, generation: generation)
            let current: @MainActor () -> Bool = { [weak self, weak draft] in
                guard let self, let draft else { return false }
                return self.epoch == expected && self.draft === draft
            }
            self.draft = draft; session = review; cardID = item.cardId
            subtitle = Subtitle(deck: Self.lastComponent(deck.fullName), notetype: notetype.name)
            let made = try makeRuntime(draft, current)
            host = made; runtime = made
            // The form shows first: its page loads only once the view holds it (as Add's does).
            isPresented = true
            try await made.start()
            guard current(), runtime === made else { throw Self.ownerChanged }
            return true
        } catch {
            if epoch == expected {
                host?.shutDown()
                reset()
                self.error = String(describing: error)
                // The form never showed: the review says why (A1's error path).
                review.showNotice(String(describing: error))
            }
            // Whatever ended the attempt, the review gets its input back.
            if review.editingNote, !isPresented { review.endEditingNote(saved: false) }
            return false
        }
    }

    /// Done (E2): flush, then one patch of the changed fields. It closes once
    /// the patch commits; with no change it closes at once. A failed patch
    /// keeps the form and its text.
    @discardableResult public func save() async -> Bool {
        if let saveTask { return await saveTask.value }
        guard canSave, let draft, let host = runtime else { return false }
        saving = true; saveFailure = nil; error = nil
        let expected = epoch
        let work = Task { () -> Bool in
            let current = { self.epoch == expected && self.draft === draft && self.runtime === host }
            do {
                await reloadTask?.value
                guard current() else { throw Self.ownerChanged }
                try await host.flush(hold: true)
                guard current(), draft.pendingFields.isEmpty else { throw Self.ownerChanged }
                guard let patch = draft.patch() else { finish(saved: false); return true }
                patchInFlight = true
                let result: NoteMutation
                // E2: the save is a step of the review session, so the review's Undo covers it.
                do { result = try await CoreNoteService().patchNote(patch, sessionID: session?.sessionID) }
                catch { patchInFlight = false; throw error }
                ownPatches.insert(result.summary.seq); patchInFlight = false
                await afterPatching?(result)
                guard current() else { return result.written }
                finish(saved: result.written)
                return true
            } catch {
                guard current() else { return false }
                if case .noteNotFound = error as? BridgeError {
                    // W5.4: the note was deleted; the form closes.
                    noteDeleted(droppingChanges: true)
                    return false
                }
                saveFailure = Self.failure(error)
                try? await host.release()
                handleHeldChanges()
                return false
            }
        }
        saveTask = work
        let result = await work.value
        saveTask = nil; saving = false
        return result
    }

    /// Cancel, Esc, ⌘W and the swipe down (E7): close and drop the draft,
    /// asking "Discard changes?" first when it holds a change. Returns
    /// whether the form closed.
    @discardableResult public func requestCancel() async -> Bool {
        if let closeTask { return await closeTask.value }
        guard isPresented else { return true }
        // Still opening: the page never held an edit, so nothing is asked.
        if opening { finish(saved: false); return true }
        guardingClose = true
        let work = Task { () -> Bool in
            _ = await saveTask?.value
            await reloadTask?.value
            guard isPresented, let draft, let host = runtime else { return true }
            let expected = epoch
            // The page's pending input counts as a change; a failed flush keeps it unknown.
            let flushed = (try? await host.flush(hold: true)) != nil
            guard epoch == expected else { return !isPresented }
            if !flushed || draft.hasChanges {
                let discard = await withCheckedContinuation { continuation in
                    discardConfirmation = UUID(); discardAnswer = continuation
                }
                guard epoch == expected else { return !isPresented }
                guard discard else { try? await host.release(); return false }
            }
            finish(saved: false)
            return true
        }
        closeTask = work
        let result = await work.value
        closeTask = nil; guardingClose = false
        return result
    }

    public func answerDiscard(_ confirmation: UUID, discard: Bool) {
        guard discardConfirmation == confirmation else { return }
        discardConfirmation = nil
        let answer = discardAnswer; discardAnswer = nil; answer?.resume(returning: discard)
    }

    /// ReviewCollection forwards every committed change of this generation.
    /// A foreign change to the note follows E2's merge rule; one that removes
    /// it closes the form (W5.4).
    func receive(_ changes: ChangeSet, seq: UInt64, generation: UInt64) {
        guard isPresented, let draft, draft.generation == generation, let note = draft.noteID else { return }
        guard changes.wholeCollection || changes.notes.contains(where: { $0.id == note }) ||
                changes.notetypes.contains(where: { $0.id == draft.notetype.id }) else { return }
        heldChanges.append((seq, changes))
        if !patchInFlight { handleHeldChanges() }
    }

    /// The collection is replaced or closed: the form ends without a write.
    func generationEnded() {
        if let confirmation = discardConfirmation { answerDiscard(confirmation, discard: false) }
        let review = session
        epoch = UUID(); runtime?.shutDown(); reset()
        // A review that outlives the collection change gets its input back.
        review?.endEditingNote(saved: false)
    }

    private func handleHeldChanges() {
        let events = heldChanges.sorted { $0.seq < $1.seq }
        heldChanges.removeAll()
        let foreign = events.filter { !ownPatches.contains($0.seq) }
        guard !foreign.isEmpty, let draft, let note = draft.noteID else { return }
        if foreign.contains(where: { event in event.changes.notes.contains { $0.id == note && $0.change == .removed } }) {
            noteDeleted(droppingChanges: draft.hasChanges)
            return
        }
        reloadPending = true
        startReload()
    }

    /// E2 with `editor/edit-flows` W5.2: flush, read the note again, then
    /// load every field without an unsaved edit; the changed ones keep the
    /// draft. Several changes that arrive meanwhile give one more reload.
    private func startReload() {
        guard reloadTask == nil, reloadPending else { return }
        reloadTask = Task {
            while reloadPending, isPresented, !saving, let draft, let host = runtime, let note = draft.noteID {
                reloadPending = false
                let expected = epoch
                do {
                    try await host.flush(hold: false)
                    guard epoch == expected else { break }
                    let stored = try await CoreNoteService().readNote(id: note)
                    guard epoch == expected, self.draft === draft else { break }
                    // A changed layout is Done's Stale refusal to report; no field is reinterpreted here.
                    guard stored.notetypeId == draft.notetype.id, stored.fields.count == draft.fields.count else { break }
                    let ords = draft.reloadUnchanged(stored: stored.fields, tags: stored.tags)
                    if !ords.isEmpty { try await host.reload(fields: ords) }
                } catch {
                    guard epoch == expected else { break }
                    if case .noteNotFound = error as? BridgeError { noteDeleted(droppingChanges: draft.hasChanges); break }
                    self.error = String(describing: error)
                }
            }
            reloadTask = nil
        }
    }

    private func noteDeleted(droppingChanges: Bool) {
        let review = session
        if let confirmation = discardConfirmation { answerDiscard(confirmation, discard: true) }
        finish(saved: false)
        if droppingChanges { review?.showNotice("This note was deleted, so your last changes couldn’t be saved.") }
    }

    private func finish(saved: Bool) {
        let review = session
        epoch = UUID(); runtime?.shutDown()
        reset()
        review?.endEditingNote(saved: saved)
    }

    private func reset() {
        runtime = nil; draft = nil; isPresented = false; subtitle = nil; cardID = nil; session = nil
        saveFailure = nil; heldChanges.removeAll(); ownPatches.removeAll(); patchInFlight = false
        reloadPending = false
    }

    nonisolated static func lastComponent(_ fullName: String) -> String {
        fullName.components(separatedBy: "::").last.flatMap { $0.isEmpty ? nil : $0 } ?? fullName
    }
    private static func failure(_ error: any Error) -> SaveFailure {
        if case .invalidArgument = error as? BridgeError { return .checkDatabase }
        return .notSaved
    }
    private static var ownerChanged: BridgeError {
        .unavailable(reason: "The editor changed. Your changes have been kept; nothing was saved.")
    }
}
