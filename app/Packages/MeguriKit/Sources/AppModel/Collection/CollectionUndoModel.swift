import CoreBridge
import Foundation
import Observation

/// Edit › Undo and Redo outside review (R173, N20.4): the collection's own
/// history (`engine/undo`, global scope), titled after its newest step.
/// A text input that owns undo keeps it (R174); the menu decides that.
@Observable @MainActor
public final class CollectionUndoModel {
    public private(set) var undoStatus: ReviewUndoStatus = .unavailable(reason: .none)
    public private(set) var redoStatus: ReviewUndoStatus = .unavailable(reason: .none)
    public private(set) var busy = false
    private weak var owner: ReviewCollection?
    private var revision: UInt64 = 0
    private var refreshTask: Task<Void, Never>?
    private var task: Task<Void, Never>?
    // Tests replace the history; production always uses the core's.
    @ObservationIgnored var service: any UndoService = CoreUndoService()

    init(owner: ReviewCollection) { self.owner = owner }

    /// The step's name while one can be undone ("Rename Deck"), or the
    /// display title of the command that recorded it ("Suspend Cards",
    /// `browser/workspace` V8.3).
    public var undoName: String? { title(undoStatus) }
    public var redoName: String? { title(redoStatus) }

    /// Display titles by step id, for the steps whose command the app holds
    /// (V8.3: a step does not carry the command that recorded it). An undo
    /// or redo hands its title on to the step it leaves.
    @ObservationIgnored private var titles: [UInt64: String] = [:]

    /// The step `id` (an op's seq) shows `title` in Edit › Undo and Redo.
    public func name(step id: UInt64, _ title: String) {
        titles[id] = title
        // The oldest go first: the history holds far fewer steps than this.
        if titles.count > 200 { for key in titles.keys.sorted().prefix(100) { titles[key] = nil } }
        titleRevision &+= 1
    }

    /// Raised when a title changes, so the menus read it again.
    private(set) var titleRevision: UInt64 = 0

    private func title(_ status: ReviewUndoStatus) -> String? {
        _ = titleRevision
        guard case .available(let step) = status else { return nil }
        return titles[step.id] ?? step.name
    }
    public var canUndo: Bool { undoName != nil && admitted }
    public var canRedo: Bool { redoName != nil && admitted }

    /// The same admission as the deck name operations: a ready collection,
    /// no review, no sheet and no other collection work.
    private var admitted: Bool {
        guard let owner, !busy else { return false }
        return owner.deckNamesAllowed(host: .main, route: nil)
    }

    public func undo() { run(redo: false) }
    public func redo() { run(redo: true) }

    private func run(redo: Bool) {
        guard redo ? canRedo : canUndo, let owner else { return }
        // Reserved before yielding, so a second ⌘Z admits nothing.
        busy = true
        let expected = owner.generation, intent = owner.navigation.intentRevision
        // The title goes with the step: the redo (or undo) step it leaves.
        let step: ReviewStepInfo? = if case .available(let step) = redo ? redoStatus : undoStatus { step } else { nil }
        task = Task {
            defer { busy = false; task = nil }
            do {
                let start: CoreUndoStart
                if redo { start = try await service.startRedo(scope: .global) }
                else { start = try await service.startUndo(scope: .global) }
                _ = try await start.value()
            } catch {
                if expected == owner.generation { owner.collectionUndoFailed() }
            }
            guard expected == owner.generation else { return }
            await owner.collectionUndoFinished(intent: intent)
            // The tree's refresh scheduled a peek; this one replaces it.
            refreshTask?.cancel(); refreshTask = nil
            await refreshNow()
            if let step, let title = titles[step.id],
               case .available(let left) = redo ? undoStatus : redoStatus, left.name == step.name, left.id != step.id {
                name(step: left.id, title)
            }
        }
    }

    /// Peeks again after the collection changed; coalesced.
    func scheduleRefresh() {
        refreshTask?.cancel()
        refreshTask = Task { await refreshNow() }
    }

    func refreshNow() async {
        // A refresh cancelled before it started leaves the newer one alone.
        guard !Task.isCancelled else { return }
        revision &+= 1
        let current = revision
        let undo = try? await service.peekUndo(scope: .global)
        let redo = try? await service.peekRedo(scope: .global)
        guard current == revision, !Task.isCancelled else { return }
        undoStatus = undo ?? .unavailable(reason: .none)
        redoStatus = redo ?? .unavailable(reason: .none)
    }

    func generationEnded() {
        revision &+= 1
        titles = [:]
        refreshTask?.cancel()
        undoStatus = .unavailable(reason: .none); redoStatus = .unavailable(reason: .none)
    }

    public func waitForCompletion() async { await task?.value }
    func waitForRefresh() async { await refreshTask?.value }

    /// The step's name while one is available.
    public nonisolated static func stepName(_ status: ReviewUndoStatus) -> String? {
        if case .available(let step) = status { return step.name }
        return nil
    }
}
