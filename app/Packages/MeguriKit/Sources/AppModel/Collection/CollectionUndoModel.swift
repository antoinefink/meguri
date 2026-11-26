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
    public private(set) var failed = false
    public func dismissFailure() { failed = false }
    private weak var owner: ReviewCollection?
    private var revision: UInt64 = 0
    private var epoch: UInt64 = 0
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
    @ObservationIgnored private var orderedSteps: Set<UInt64> = []
    @ObservationIgnored private var titles: [UInt64: String] = [:]

    /// The step `id` (an op's seq) shows `title` in Edit › Undo and Redo.
    public func name(step id: UInt64, _ title: String, ordered: Bool = false) {
        titles[id] = title
        if ordered { orderedSteps.insert(id) }
        // The oldest go first: the history holds far fewer steps than this.
        if titles.count > 200 { for key in titles.keys.sorted().prefix(100) { titles[key] = nil } }
        orderedSteps.formIntersection(titles.keys)
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
        guard let owner, !busy, !owner.browseMutationBusy else { return false }
        return owner.deckNamesAllowed(host: .main, route: nil)
    }

    private var dispatchAllowed: Bool {
        guard let owner, !owner.browseMutationBusy else { return false }
        return owner.deckNamesAllowed(host: .main, route: nil, ignoringUndoReservation: true)
    }

    public func undo() { run(redo: false) }
    public func redo() { run(redo: true) }

    private func run(redo: Bool) {
        guard redo ? canRedo : canUndo, let owner else { return }
        // Reserved before yielding, so a second ⌘Z admits nothing.
        busy = true
        failed = false
        let expected = owner.generation, epoch = epoch, intent = owner.navigation.intentRevision
        // The title goes with the step: the redo (or undo) step it leaves.
        let step: ReviewStepInfo? = if case .available(let step) = redo ? redoStatus : undoStatus { step } else { nil }
        task = Task {
            defer { busy = false; task = nil }
            var done = false
            do {
                guard expected == owner.generation, epoch == self.epoch, dispatchAllowed else { return }
                let current: ReviewUndoStatus
                if redo { current = try await service.peekRedo(scope: .global) }
                else { current = try await service.peekUndo(scope: .global) }
                guard expected == owner.generation, epoch == self.epoch, dispatchAllowed else { return }
                guard current == (redo ? redoStatus : undoStatus),
                      case .available(let currentStep) = current, currentStep.id == step?.id else {
                    await refreshNow()
                    return
                }
                let start: CoreUndoStart
                if redo { start = try await service.startRedo(scope: .global) }
                else if let step, orderedSteps.contains(step.id) {
                    // The writer checks the named tag step atomically. A newer
                    // edit must never be undone by an old Tags toolbar action.
                    start = try await service.startUndo(step: step.id)
                } else { start = try await service.startUndo(scope: .global) }
                let result = try await start.value()
                if case .global(let summary, _) = result, summary.generation != expected { return }
                guard expected == owner.generation, epoch == self.epoch, owner.browseReadable else { return }
                done = true
            } catch {
                if expected == owner.generation, epoch == self.epoch { failed = true; owner.collectionUndoFailed() }
            }
            guard expected == owner.generation, epoch == self.epoch, owner.browseReadable else { return }
            await owner.collectionUndoFinished(intent: intent)
            guard expected == owner.generation, epoch == self.epoch, owner.browseReadable else { return }
            // The tree's refresh scheduled a peek; this one replaces it.
            refreshTask?.cancel(); refreshTask = nil
            await refreshNow()
            guard expected == owner.generation, epoch == self.epoch, owner.browseReadable else { return }
            // Only a step that was undone or redone hands its title on.
            if done, let step, let title = titles[step.id],
               case .available(let left) = redo ? undoStatus : redoStatus, left.name == step.name, left.id != step.id {
                name(step: left.id, title, ordered: orderedSteps.contains(step.id))
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
        let current = revision, expected = owner?.generation
        let undo = try? await service.peekUndo(scope: .global)
        let redo = try? await service.peekRedo(scope: .global)
        guard current == revision, expected == owner?.generation, owner?.browseReadable == true, !Task.isCancelled else { return }
        undoStatus = undo ?? .unavailable(reason: .none)
        redoStatus = redo ?? .unavailable(reason: .none)
    }

    func generationEnded() {
        revision &+= 1
        epoch &+= 1
        titles = [:]; orderedSteps = []
        failed = false
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
