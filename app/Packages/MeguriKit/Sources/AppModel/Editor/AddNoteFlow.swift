import CoreBridge
import Foundation
import Observation

/// AppModel owns the operation; Editor supplies the actual retained WebKit
/// host. This boundary has no persistence implementation or substitute store.
@MainActor public protocol AddNoteFieldRuntime: AnyObject {
    var draft: NoteEditorDraft { get }
    func start() async throws
    func flush(hold: Bool) async throws
    func release() async throws
    func setFocus(_ ord: Int) async throws
    func shutDown()
    /// N17.1: a runtime that can load the next draft into the page it already
    /// shows hands that page on, so the keyboard stays up after an add. The
    /// receiver ends as `shutDown()` ends it, except that the page lives on.
    func successor(for draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) -> (any AddNoteFieldRuntime)?
    /// Edit mode: show again the fields the draft reloaded from the stored note.
    func reload(fields: [Int]) async throws
}
extension AddNoteFieldRuntime {
    public func successor(for draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) -> (any AddNoteFieldRuntime)? { nil }
    public func reload(fields: [Int]) async throws {}
}

/// Collection-owned A5/A7 action model. Production presentation remains gated
/// on *all* replacement consumers acquiring pre-operation editor admission.
@Observable @MainActor
public final class AddNoteFlow {
    /// A picker action, never a stored note type id. Core ensures it on demand.
    public static let imageOcclusionChoice: Int64 = -206
    public typealias RuntimeFactory = @MainActor (NoteEditorDraft, @escaping @MainActor () -> Bool) throws -> any AddNoteFieldRuntime
    public let preparation: AddNoteDraft
    public var draft: NoteEditorDraft? { preparation.draft }
    public private(set) var runtime: (any AddNoteFieldRuntime)?
    public private(set) var isPresented = false
    public private(set) var opening = false
    public private(set) var saving = false
    public private(set) var guardingClose = false
    public private(set) var error: String?
    public private(set) var failure: BridgeError?
    public private(set) var refusal: FieldState?
    public private(set) var lastAdded: NoteMutation?
    public private(set) var history: [Int64] = []
    public private(set) var missingClozeConfirmation: UUID?
    public private(set) var discardConfirmation: UUID?
    public private(set) var exclusiveOperation: String?
    /// N17.1: the deck of the newest add, until its sheet closes or the selection changes.
    public struct Confirmation: Equatable, Sendable {
        public let note: Int64
        public let deck: String
    }
    public private(set) var confirmation: Confirmation?
    /// A2's lists for the Deck and Type rows, read on open and after each relevant change.
    public private(set) var deckChoices: [DeckNameEntry] = []
    public private(set) var notetypeChoices: [NotetypeName] = []
    @ObservationIgnored private var choicesTask: Task<Void, Never>?
    @ObservationIgnored private var choicesPending = false
    /// Chosen by each opener: after an add the next draft loads into the page
    /// already showing, instead of a page from the factory.
    @ObservationIgnored private var continuesPage = false
    @ObservationIgnored private weak var owner: ReviewCollection?
    @ObservationIgnored private var factory: RuntimeFactory?
    @ObservationIgnored private var epoch = UUID()
    @ObservationIgnored private var startedEpoch: UUID?
    @ObservationIgnored private weak var failedStartupDraft: NoteEditorDraft?
    @ObservationIgnored private var attempt: UUID?
    @ObservationIgnored private var saveTask: Task<NoteMutation?, Never>?
    @ObservationIgnored private var closeTask: Task<Bool, Never>?
    @ObservationIgnored private var closeDecision: ReviewCollection.EditorReplacementDecision?
    @ObservationIgnored private var backgroundTask: Task<Bool, Never>?
    @ObservationIgnored private var clozeAnswer: CheckedContinuation<Bool, Never>?
    @ObservationIgnored private var discardAnswer: CheckedContinuation<Bool, Never>?
    @ObservationIgnored private var replacement: UUID?
    private var noNotetype = false
    @ObservationIgnored private var showingNoNotetype = false
    @ObservationIgnored private var selectionObservation = UUID()
    @ObservationIgnored private var pendingSelectionGeneration: UInt64?
    @ObservationIgnored private var selectionTask: Task<Void, Never>?
    @ObservationIgnored private var resumingInput = false
    @ObservationIgnored private var resumeTask: Task<Void, Never>?
    // Suspend only real collection snapshots for ownership regression coverage.
    var afterReadingSelection: (@MainActor () async -> Void)?
    var selectionRefreshCompletion: Task<Void, Never>? { selectionTask }
    // Suspend an actual fresh Core check for owner-race regression coverage.
    var afterCheckingFields: (@MainActor () async -> Void)?
    // Hold the actual durable reply, never substitute a mutation or a writer.
    var afterAddingNote: (@MainActor (NoteMutation) async -> Void)?

    init(owner: ReviewCollection) {
        self.owner = owner; preparation = AddNoteDraft(owner: owner)
    }

    public var canSave: Bool {
        isPresented && runtime != nil && !noNotetype && !resumingInput && draft?.submissionAcknowledged == false && !opening && !saving && !guardingClose &&
            backgroundTask == nil && exclusiveOperation == nil && owner?.canBeginAddAction == true
    }

    /// N17.1: Add also waits for content in the first field.
    public var canAdd: Bool { canSave && (draft?.occlusionRoles != nil || draft?.firstFieldHasContent == true) }
    private var canChangeSelection: Bool {
        canSave && runtime?.draft === draft && startedEpoch == epoch
    }

    /// A2: every normal deck in the snapshot's order. The default deck is left
    /// out when the deck tree hides it, unless it is selected or nothing else is listed.
    nonisolated public static func deckChoices(names: [DeckNameEntry], selected: Int64, defaultShown: Bool) -> [DeckNameEntry] {
        let normal = names.filter { !$0.filtered }
        let listed = normal.filter { $0.id != 1 || defaultShown || selected == 1 }
        return listed.isEmpty ? normal : listed
    }

    public func refreshChoices() {
        guard isPresented, let draft else { return }
        // A request made while one is in flight runs once that one ends.
        guard choicesTask == nil else { choicesPending = true; return }
        choicesTask = Task {
            defer {
                choicesTask = nil
                if choicesPending { choicesPending = false; refreshChoices() }
            }
            guard let decks = try? await CoreDeckService().allDeckNames(),
                  var types = try? await CoreNotetypeService().notetypeNames(),
                  isPresented, let current = self.draft, current.generation == draft.generation,
                  decks.generation == current.generation else { return }
            var hasIO = false
            for type in types {
                guard let record = try? await CoreNotetypeService().readNotetype(id: type.id) else { return }
                if record.originalStockKind == 6 { hasIO = true; break }
            }
            guard isPresented, self.draft === current,
                  owner?.ownsAddGeneration(current.generation) == true else { return }
            if !hasIO {
                var name = "Image Occlusion"
                while types.contains(where: { $0.name == name }) { name += "+" }
                types.append(.init(id: Self.imageOcclusionChoice, name: name))
                types.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
            }
            deckChoices = Self.deckChoices(names: decks.decks, selected: current.deck.id,
                                           defaultShown: owner?.deck(1) != nil)
            notetypeChoices = types
        }
    }

    /// A2: choosing another deck selects it and changes nothing else.
    public func selectDeck(_ id: Int64) async {
        guard canChangeSelection, id != draft?.deck.id else { return }
        if await applyEntryDeck(id) { confirmation = nil }
        refreshChoices()
    }

    /// A3: held flush, A3.1's mapping, the deck kept, the last add forgotten,
    /// then the new draft loaded with focus at the same position.
    public func selectNotetype(_ id: Int64) async {
        guard canChangeSelection, let draft, let page = runtime, id != draft.notetype.id else { return }
        opening = true
        defer { opening = false; startSelectionRefresh(); refreshChoices() }
        await selectionTask?.value
        let expected = epoch
        do {
            try await page.flush(hold: true)
            guard owns(draft, epoch: expected), runtime === page, draft.pendingFields.isEmpty else { throw Self.ownerChanged }
            let revision = draft.preparationRevision
            let selectedID: Int64
            if id == Self.imageOcclusionChoice {
                selectedID = try await CoreNotetypeService().ensureImageOcclusionNotetype(expectedGeneration: draft.generation).id
            } else { selectedID = id }
            let notetype = try await CoreNotetypeService().readNotetype(id: selectedID)
            let lifecycle = try await CoreCollectionService().lifecycle()
            guard owns(draft, epoch: expected), runtime === page, draft.preparationRevision == revision,
                  lifecycle == .ready(generation: draft.generation) else { throw Self.ownerChanged }
            let next = try draft.replacingSelection(notetype: notetype, deck: draft.deck)
            confirmation = nil
            // The new draft gets its own host.
            let host = page
            host.shutDown()
            epoch = UUID()
            preparation.replace(with: next)
            do { try await mount(next) }
            catch {
                if preparation.draft === next, runtime?.draft === next,
                   owner?.ownsAddGeneration(next.generation) == true { record(error) }
            }
        } catch {
            if epoch == expected, preparation.draft === draft {
                record(error); await release(page, draft: draft, epoch: expected)
            }
        }
    }

    /// A factory is supplied by Editor once, never by a SwiftUI view remount.
    /// An invalid explicit deck refuses the entry rather than using defaults.
    @discardableResult
    public func open(presetDeck: Int64? = nil, continuingPage: Bool = false, makeRuntime: @escaping RuntimeFactory) async -> Bool {
        guard owner?.canBeginAddAction == true, exclusiveOperation == nil,
              draft?.submissionAcknowledged != true, !resumingInput, !opening, !saving, !guardingClose, backgroundTask == nil else { return false }
        if isPresented {
            guard let draft, runtime?.draft === draft, startedEpoch == epoch,
                  owns(draft, epoch: epoch) else { return false }
            guard let presetDeck else { return true }
            return await applyEntryDeck(presetDeck)
        }
        opening = true
        defer { opening = false; startSelectionRefresh() }
        await selectionTask?.value
        let retryingStartup: Bool
        if let draft, draft === failedStartupDraft, presetDeck == nil,
           owner?.ownsAddGeneration(draft.generation) == true {
            retryingStartup = true
        } else { retryingStartup = false }
        if !retryingStartup { await preparation.prepare(presetDeck: presetDeck) }
        guard preparation.error == nil, let draft, owner?.canBeginAddAction == true,
              owner?.ownsAddGeneration(draft.generation) == true, exclusiveOperation == nil else {
            error = preparation.error; return false
        }
        factory = makeRuntime; continuesPage = continuingPage; isPresented = true
        error = nil; failure = nil; refusal = nil
        do {
            try await mount(draft)
            if retryingStartup || noNotetype { selectionChanged(generation: draft.generation) }
            refreshChoices()
            return true
        }
        catch { return false }
    }

    private func applyEntryDeck(_ id: Int64) async -> Bool {
        opening = true
        defer { opening = false; startSelectionRefresh() }
        await selectionTask?.value
        guard let draft, let host = runtime else { return false }
        let expected = epoch
        do {
            try await host.flush(hold: true)
            guard owns(draft, epoch: expected), runtime === host else { throw Self.ownerChanged }
            let revision = draft.preparationRevision
            let names = try await CoreDeckService().allDeckNames()
            guard owns(draft, epoch: expected), runtime === host,
                  draft.preparationRevision == revision, names.generation == draft.generation else { throw Self.ownerChanged }
            guard let selected = names.decks.first(where: { $0.id == id && !$0.filtered }) else {
                throw BridgeError.unavailable(reason: "The selected deck is no longer a normal deck. Choose a deck again.")
            }
            try draft.selectDeck(selected)
            await release(host, draft: draft, epoch: expected)
            return owns(draft, epoch: expected) && runtime === host
        } catch {
            if epoch == expected, preparation.draft === draft {
                record(error); await release(host, draft: draft, epoch: expected)
            }
            return false
        }
    }

    private func owns(_ draft: NoteEditorDraft, epoch expected: UUID) -> Bool {
        epoch == expected && isPresented && preparation.draft === draft &&
            owner?.ownsAddGeneration(draft.generation) == true
    }

    /// Only ReviewCollection's owned flow receives relevant committed changes.
    /// Keep the latest observation while save/close/background owns the input.
    func selectionChanged(generation: UInt64) {
        guard isPresented, draft?.generation == generation,
              owner?.ownsAddGeneration(generation) == true else { return }
        selectionObservation = UUID(); pendingSelectionGeneration = generation
        startSelectionRefresh()
        refreshChoices()
    }

    private var canRefreshSelection: Bool {
        isPresented && draft?.submissionAcknowledged == false && !resumingInput && !opening && !saving && !guardingClose &&
            backgroundTask == nil && exclusiveOperation == nil && owner?.canBeginAddAction == true
    }

    private func startSelectionRefresh() {
        guard selectionTask == nil, pendingSelectionGeneration != nil, canRefreshSelection else { return }
        selectionTask = Task {
            while let generation = pendingSelectionGeneration, canRefreshSelection,
                  let draft, let host = runtime, draft.generation == generation {
                let observation = selectionObservation
                await refreshSelection(draft, host: host, observation: observation)
                if selectionObservation == observation { pendingSelectionGeneration = nil }
            }
            selectionTask = nil
        }
    }

    private func refreshSelection(_ draft: NoteEditorDraft, host: any AddNoteFieldRuntime, observation: UUID) async {
        let expected = epoch
        let current = {
            self.owns(draft, epoch: expected) && self.runtime === host && self.attempt == nil &&
                self.canRefreshSelection && self.selectionObservation == observation
        }
        var revision = draft.preparationRevision
        var held = false
        do {
            // A valid selection does not flush, rebuild or reset last-add state.
            // Once repair is needed, query again after the real held flush.
            while current() {
                let selection = try await CoreEditorServices().addNotetypeSelection(
                    expectedGeneration: draft.generation, selectedId: draft.notetype.id)
                guard current(), selection.generation == draft.generation else { break }
                let decks = try await CoreDeckService().allDeckNames()
                guard current(), decks.generation == draft.generation else { break }
                let lifecycle = try await CoreCollectionService().lifecycle()
                await afterReadingSelection?()
                guard current(), lifecycle == .ready(generation: draft.generation) else { break }
                // Input accepted during a query is newer than its observation.
                // Re-read against that revision; never map the old field bytes.
                guard draft.preparationRevision == revision else {
                    revision = draft.preparationRevision
                    continue
                }
                guard let stored = selection.notetype else {
                    recordMissingNotetype(); break
                }
                guard let deck = decks.decks.first(where: { $0.id == draft.deck.id && !$0.filtered }) ??
                    decks.decks.first(where: { $0.id == 1 && !$0.filtered }) else {
                    record(BridgeError.notFound(table: .decks, key: String(draft.deck.id))); break
                }
                if stored.id == draft.notetype.id && deck.id == draft.deck.id {
                    clearMissingNotetype(); break
                }
                if !held {
                    held = true
                    do { try await host.flush(hold: true) }
                    catch {
                        if current() { record(error) }
                        break
                    }
                    guard current(), draft.pendingFields.isEmpty else { break }
                    revision = draft.preparationRevision
                    continue
                }
                guard draft.pendingFields.isEmpty else { throw Self.ownerChanged }
                let repaired = try draft.replacingSelection(notetype: stored, deck: deck)
                guard current(), draft.preparationRevision == revision else { break }
                clearMissingNotetype()
                host.shutDown(); epoch = UUID(); preparation.replace(with: repaired)
                opening = true
                do { try await mount(repaired) }
                catch {
                    if isPresented, preparation.draft === repaired, attempt == nil,
                       owner?.ownsAddGeneration(repaired.generation) == true { record(error) }
                    opening = false
                    return
                }
                opening = false
                if preparation.draft === repaired, runtime?.draft === repaired,
                   canRefreshSelection, selectionObservation == observation {
                    error = "The Add selection changed. Check the selection and press Add again."
                }
                return
            }
        } catch {
            if current(), draft.preparationRevision == revision { record(error) }
        }
        // Other actions join this task before touching the host. A superseded
        // observation may release its own hold, never another runtime's input.
        if held { await release(host, draft: draft, epoch: expected) }
        if owns(draft, epoch: expected), runtime === host,
           selectionObservation == observation, !canRefreshSelection {
            // An accepted action will restart the latest useful observation.
            selectionObservation = UUID()
        }
    }

    private func recordMissingNotetype() {
        noNotetype = true
        record(BridgeError.invalidArgument("This collection has no note type. Your draft has been kept."))
        showingNoNotetype = true
    }

    private func clearMissingNotetype() {
        noNotetype = false
        if showingNoNotetype { error = nil; failure = nil; showingNoNotetype = false }
    }

    private func mount(_ draft: NoteEditorDraft, after previous: (any AddNoteFieldRuntime)? = nil) async throws {
        epoch = UUID(); runtime = nil
        let expected = epoch
        var startingHost: (any AddNoteFieldRuntime)?
        do {
            let current: @MainActor () -> Bool = { [weak self, weak draft] in
                guard let self, let draft else { return false }
                return self.owns(draft, epoch: expected)
            }
            // After an add the same page takes the next draft when it can.
            let adopted = continuesPage ? previous?.successor(for: draft, isCurrent: current) : nil
            if adopted == nil { previous?.shutDown() }
            guard let factory else { throw BridgeError.unavailable(reason: "The field editor is unavailable.") }
            let host = try adopted ?? factory(draft, current)
            startingHost = host; runtime = host
            try await host.start()
            guard owns(draft, epoch: expected), runtime === host else {
                host.shutDown(); throw BridgeError.unavailable(reason: "The editor owner changed. Your draft has been kept.")
            }
            startedEpoch = expected
            failedStartupDraft = nil
        } catch {
            // A revoked start can settle after another owner has taken over.
            // Only the exact original failure may retire presentation or report its error.
            if epoch == expected, preparation.draft === draft, runtime === startingHost {
                epoch = UUID(); startingHost?.shutDown(); runtime = nil; isPresented = false
                failedStartupDraft = draft
                record(error)
            }
            throw error
        }
    }

    /// One task owns the held input, check, optional consent, write and advance.
    /// Repeated activations do nothing, including during delayed confirmation.
    @discardableResult public func save() async -> NoteMutation? {
        guard canSave, let draft, let host = runtime else { return nil }
        let token = UUID(), expected = epoch
        attempt = token; saving = true; error = nil; failure = nil; refusal = nil
        let priorSelection = selectionTask
        let work = Task { () -> NoteMutation? in
            await priorSelection?.value
            guard attempt == token, owns(draft, epoch: expected), runtime === host else { return nil }
            return await performSave(draft, host: host, epoch: expected, attempt: token)
        }
        saveTask = work
        let result = await work.value
        if attempt == token { attempt = nil; saving = false; saveTask = nil }
        startSelectionRefresh()
        return result
    }

    private func performSave(_ draft: NoteEditorDraft, host: any AddNoteFieldRuntime,
                             epoch expected: UUID, attempt token: UUID) async -> NoteMutation? {
        let current = { self.owns(draft, epoch: expected) && self.runtime === host && self.attempt == token }
        do {
            try await host.flush(hold: true)
            guard current(), draft.pendingFields.isEmpty else { throw Self.ownerChanged }
            let revision = draft.preparationRevision
            let exact = { current() && draft.preparationRevision == revision }
            // D5 reads real stored identities after the held flush. Missing
            // selections are shown before another explicit Add is permitted.
            let selection = try await CoreEditorServices().addNotetypeSelection(
                expectedGeneration: draft.generation, selectedId: draft.notetype.id)
            guard exact(), selection.generation == draft.generation else { throw Self.ownerChanged }
            guard let stored = selection.notetype else {
                recordMissingNotetype()
                await release(host, draft: draft, epoch: expected)
                return nil
            }
            let decks = try await CoreDeckService().allDeckNames()
            guard exact(), decks.generation == draft.generation else { throw Self.ownerChanged }
            guard let deck = decks.decks.first(where: { $0.id == draft.deck.id && !$0.filtered }) ??
                decks.decks.first(where: { $0.id == 1 && !$0.filtered }) else {
                throw BridgeError.notFound(table: .decks, key: String(draft.deck.id))
            }
            let lifecycle = try await CoreCollectionService().lifecycle()
            guard exact(), lifecycle == .ready(generation: draft.generation) else { throw Self.ownerChanged }
            guard stored.id != draft.notetype.id || stored == draft.notetype else {
                throw BridgeError.stale(table: .notetypes, key: String(draft.notetype.id))
            }
            clearMissingNotetype()
            if stored.id != draft.notetype.id || deck.id != draft.deck.id {
                let repaired = try draft.replacingSelection(notetype: stored, deck: deck)
                guard exact() else { throw Self.ownerChanged }
                host.shutDown(); epoch = UUID(); preparation.replace(with: repaired)
                do {
                    try await mount(repaired)
                    error = "The Add selection changed. Check the selection and press Add again."
                } catch { record(error) }
                return nil
            }
            let request = AddScreenNoteRequest(expectedGeneration: draft.generation, notetype: draft.notetype,
                fields: draft.values, tags: draft.tags, deckId: draft.deck.id)
            if let roles = draft.occlusionRoles,
               (try? occlusionHasNumberedMask(draft.values[roles.occlusions])) != true {
                draft.occlusionAddRefused = true
                draft.showsMaskEditor = true
                await release(host, draft: draft, epoch: expected)
                return nil
            }
            let check = try await CoreNoteService().checkNoteFields(notetypeId: request.notetype.id, fields: request.fields, ownNote: nil)
            await afterCheckingFields?()
            guard exact() else { throw Self.ownerChanged }
            switch check.state {
            case .empty, .notetypeNotCloze, .fieldNotCloze:
                refusal = check.state
                await release(host, draft: draft, epoch: expected)
                if current() { try await host.setFocus(0) }
                return nil
            case .missingCloze:
                guard !guardingClose, backgroundTask == nil, owner?.foreground == true else {
                    await release(host, draft: draft, epoch: expected); return nil
                }
                let accepted = await withCheckedContinuation { continuation in
                    missingClozeConfirmation = token; clozeAnswer = continuation
                }
                guard exact() else { throw Self.ownerChanged }
                guard accepted else { await release(host, draft: draft, epoch: expected); return nil }
            case .normal, .duplicate: break
            }
            guard exact() else { throw Self.ownerChanged }
            // This is the only write. Core also checks generation, exact stored
            // type base/layout and normal deck at writer admission. No retry.
            let next = try draft.nextAfterAcknowledgedAdd()
            let added = try await CoreNoteService().addNoteForScreen(request)
            guard added.written, added.summary.generation == request.expectedGeneration else {
                throw BridgeError.internal("The Add operation did not acknowledge a durable note.")
            }
            await afterAddingNote?(added)
            // Durability belongs to this submitted identity/revision, even if
            // its presentation or collection owner ended while Core replied.
            draft.acknowledgeSubmission(revision: revision)
            guard current() else {
                if !isPresented, runtime == nil, preparation.draft === draft, draft.preparationRevision == revision {
                    preparation.replace(with: nil); error = nil; failure = nil
                }
                return added
            }
            lastAdded = added; history.insert(added.id, at: 0)
            confirmation = Confirmation(note: added.id, deck: draft.deck.fullName)
            // Newer input in this same owner survives a delayed reply too.
            guard exact() else {
                await release(host, draft: draft, epoch: expected)
                return added
            }
            epoch = UUID(); preparation.replace(with: next)
            do { try await mount(next, after: host) }
            catch {
                if preparation.draft === next, runtime?.draft === next,
                   owner?.ownsAddGeneration(next.generation) == true { record(error) }
            }
            if preparation.draft === next, runtime?.draft === next, isPresented,
               owner?.ownsAddGeneration(next.generation) == true {
                await owner?.refreshAfterAdding(generation: request.expectedGeneration)
            }
            return added
        } catch {
            if epoch == expected, preparation.draft === draft {
                record(error); await release(host, draft: draft, epoch: expected)
            }
            return nil
        }
    }

    public func answerMissingCloze(_ confirmation: UUID, add: Bool) {
        guard missingClozeConfirmation == confirmation, attempt == confirmation else { return }
        missingClozeConfirmation = nil
        let answer = clozeAnswer; clozeAnswer = nil; answer?.resume(returning: add)
    }

    public func answerDiscard(_ confirmation: UUID, discard: Bool) {
        guard discardConfirmation == confirmation else { return }
        discardConfirmation = nil
        let answer = discardAnswer; discardAnswer = nil; answer?.resume(returning: discard)
    }

    private func cancelUnconfirmedAdd() {
        if let token = missingClozeConfirmation { answerMissingCloze(token, add: false) }
    }

    /// Close buttons, sheet dismissal, window close and quit must await this
    /// same guard. A failed flush never authorizes disposal or Core close.
    @discardableResult public func requestClose() async -> Bool {
        await requestClose(decision: nil)
    }
    private func requestClose(decision: ReviewCollection.EditorReplacementDecision?) async -> Bool {
        if let closeTask { return await closeTask.value }
        closeDecision = decision
        guardingClose = true
        cancelUnconfirmedAdd()
        let priorBackground = backgroundTask
        let work = Task { () -> Bool in
            _ = await saveTask?.value
            _ = await priorBackground?.value
            await selectionTask?.value
            await resumeTask?.value
            guard decision?.cancelled != true else { return false }
            guard isPresented else { return true }
            guard !opening, let draft, let host = runtime else { return false }
            let expected = epoch
            do {
                try await host.flush(hold: true)
                guard owns(draft, epoch: expected), runtime === host else { throw Self.ownerChanged }
                guard decision?.cancelled != true else {
                    await release(host, draft: draft, epoch: expected); return false
                }
                let revision = draft.preparationRevision
                var discard = false
                if !draft.unchangedSinceLastAdd {
                    discard = await withCheckedContinuation { continuation in
                        discardConfirmation = UUID(); discardAnswer = continuation
                    }
                    guard owns(draft, epoch: expected), runtime === host,
                          draft.preparationRevision == revision else { throw Self.ownerChanged }
                    guard discard, decision?.cancelled != true else {
                        await release(host, draft: draft, epoch: expected); return false
                    }
                }
                guard decision?.cancelled != true else {
                    await release(host, draft: draft, epoch: expected); return false
                }
                epoch = UUID(); host.shutDown(); runtime = nil; isPresented = false
                confirmation = nil
                if discard { preparation.replace(with: nil) }
                return true
            } catch {
                if epoch == expected, preparation.draft === draft {
                    record(error); await release(host, draft: draft, epoch: expected)
                }
                return false
            }
        }
        closeTask = work
        let result = await work.value
        closeTask = nil; closeDecision = nil; guardingClose = false
        startSelectionRefresh()
        return result
    }

    /// Background joins accepted work, then holds all field input before the
    /// collection flush/suspend. Foreground explicitly releases the same host.
    func flushForBackground() async -> Bool {
        if let backgroundTask { return await backgroundTask.value }
        cancelUnconfirmedAdd()
        let priorClose = closeTask
        let work = Task { () -> Bool in
            _ = await saveTask?.value
            if let priorClose, !(await priorClose.value) { return false }
            await selectionTask?.value
            await resumeTask?.value
            guard isPresented else { return true }
            guard !opening, let draft, let host = runtime else { return false }
            let expected = epoch
            do {
                try await host.flush(hold: true)
                guard owns(draft, epoch: expected), runtime === host else { throw Self.ownerChanged }
                return true
            } catch {
                if epoch == expected, preparation.draft === draft { record(error) }
                return false
            }
        }
        backgroundTask = work
        let result = await work.value
        backgroundTask = nil
        return result
    }

    func resumeInput() async {
        guard isPresented, !resumingInput, !saving, !guardingClose, backgroundTask == nil, exclusiveOperation == nil else { return }
        resumingInput = true
        let work = Task {
            await selectionTask?.value
            if let draft, let host = runtime, !saving, !guardingClose, backgroundTask == nil, exclusiveOperation == nil {
                await release(host, draft: draft, epoch: epoch)
            }
        }
        resumeTask = work
        await work.value
        resumeTask = nil; resumingInput = false
        // Collection foreground admission completes just after this returns.
        Task { [weak self] in
            await self?.owner?.waitForLifecycle()
            self?.startSelectionRefresh()
        }
    }

    /// Future replacement consumers must acquire this *before* Core admission,
    /// and release only after terminal completion, even without a replacement.
    func prepareForReplacement(operation: String, decision: ReviewCollection.EditorReplacementDecision) async -> UUID? {
        // A separately initiated user close owns its question independently.
        guard replacement == nil, closeTask == nil, !opening, !decision.cancelled else { return nil }
        let token = decision.id; replacement = token; exclusiveOperation = operation
        decision.revokeQuestion = { [weak self, weak decision] in
            guard let self, let decision, self.replacement == token, self.closeDecision === decision else { return }
            if let confirmation = self.discardConfirmation { self.answerDiscard(confirmation, discard: false) }
        }
        let accepted = await withTaskCancellationHandler {
            await requestClose(decision: decision)
        } onCancel: {
            Task { @MainActor in decision.cancel() }
        }
        // The collection reservation stays held through candidate disposal.
        return accepted && !decision.cancelled && replacement == token ? token : nil
    }
    public func finishReplacement(_ token: UUID) {
        guard replacement == token else { return }
        replacement = nil; exclusiveOperation = nil
        startSelectionRefresh()
    }

    /// Defensive revocation is not pre-operation admission. Unexpected changes
    /// retain nonblank input for recovery instead of silently discarding it.
    func generationEnded() {
        selectionObservation = UUID(); pendingSelectionGeneration = nil
        closeDecision?.cancel()
        epoch = UUID(); runtime?.shutDown(); runtime = nil; isPresented = false
        cancelUnconfirmedAdd()
        if let confirmation = discardConfirmation { answerDiscard(confirmation, discard: false) }
        history.removeAll(); lastAdded = nil; confirmation = nil
        deckChoices = []; notetypeChoices = []
        if draft?.unchangedSinceLastAdd == true { preparation.replace(with: nil) }
        else if draft != nil { error = "The collection changed. Your draft has been kept." }
    }

    private func release(_ host: any AddNoteFieldRuntime, draft: NoteEditorDraft, epoch expected: UUID) async {
        guard owns(draft, epoch: expected), runtime === host else { return }
        do { try await host.release() }
        catch { if owns(draft, epoch: expected) { record(error) } }
    }
    private func record(_ error: any Error) {
        showingNoNotetype = false
        failure = error as? BridgeError; self.error = String(describing: error)
    }
    private static var ownerChanged: BridgeError {
        .unavailable(reason: "The editor changed. Your draft has been kept; nothing was submitted.")
    }
}
