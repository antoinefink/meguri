import CoreBridge
import Foundation
import Observation

/// A collection-owned draft and operation, independent of the adaptive shell.
@Observable @MainActor
public final class DeckActionsModel {
    public nonisolated enum Action: Hashable, Sendable {
        case create
        case rename(Int64)
        case subdeck(Int64)
    }
    public private(set) var action: Action?
    public var name = ""
    public private(set) var busy = false
    public private(set) var cancelling = false
    public private(set) var message: String?
    public private(set) var finalName: String?
    public private(set) var progress: CoreProgress?
    private weak var owner: ReviewCollection?
    private var generation: UInt64 = 0
    private var revision: UInt64 = 0
    private var task: Task<Void, Never>?
    private var cancellationTask: Task<Void, Never>?
    private var rename: CoreDeckRenameJob?
    private var pendingResult: Result<DeckMutation, BridgeError>?
    private var pendingPage: ReviewCollection.DeckPageDetail?
    // Admission tests can hold the call; production always uses U28 services.
    @ObservationIgnored var createDeck: @MainActor (String) async throws -> DeckMutation
    @ObservationIgnored var startRename: @MainActor (Int64, String) async throws -> CoreDeckRenameJob

    init(owner: ReviewCollection) {
        self.owner = owner
        createDeck = { @MainActor in try await CoreDeckService().addDeck(name: $0) }
        startRename = { @MainActor in try await CoreDeckService().startRenameDeck(id: $0, name: $1) }
    }

    public func canPresent(_ action: Action, host: AppNavigation.SheetHost = .main) -> Bool {
        guard let owner, !busy, cancellationTask == nil,
              owner.deckNamesAllowed(host: host, route: nil) else { return false }
        return targetExists(action, owner: owner)
    }

    public func present(_ action: Action, host: AppNavigation.SheetHost = .main) {
        guard canPresent(action, host: host), let owner else { return }
        revision &+= 1
        self.action = action; generation = owner.generation
        message = nil; finalName = nil; pendingResult = nil; pendingPage = nil; cancelling = false
        switch action {
        case .create: name = ""
        case .rename(let id): name = owner.fullDeckName(id) ?? ""
        case .subdeck(let id): name = (owner.fullDeckName(id) ?? "") + "::"
        }
        owner.navigation.present(.deckName(action))
        owner.navigation.sheetHost = .main
    }

    public var canSubmit: Bool {
        guard !busy, cancellationTask == nil, finalName == nil, pendingResult == nil,
              !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              let action, let owner, generation == owner.generation else { return false }
        return owner.deckNamesAllowed(host: .main, route: .deckName(action)) && targetExists(action, owner: owner)
    }

    public func submit() {
        guard canSubmit, let action, let owner else { return }
        // Reserve synchronously, before the task can yield to another submit.
        busy = true; cancelling = false; message = nil
        let expected = generation, current = revision, draft = name
        task = Task {
            defer { rename = nil; progress = nil; busy = false; task = nil }
            if cancelling, current == revision, expected == owner.generation {
                pendingResult = .failure(.cancelled)
                await publishResult(action: action, revision: current)
                return
            }
            guard current == revision, expected == owner.generation,
                  owner.deckNamesAllowed(host: .main, route: .deckName(action)),
                  targetExists(action, owner: owner), !cancelling else { return }
            let result: Result<DeckMutation, BridgeError>
            do {
                let mutation: DeckMutation
                switch action {
                case .create, .subdeck: mutation = try await createNamedDeck(draft)
                case .rename(let id):
                    let job = try await startRename(id, draft)
                    rename = job
                    if cancelling { try? await job.job.cancel() }
                    let updates = Task {
                        for await value in job.job.progress {
                            if current == revision, expected == owner.generation { progress = value }
                        }
                    }
                    let outcome: Result<DeckMutation, BridgeError>
                    do throws(BridgeError) { outcome = .success(try await job.value()) }
                    catch { outcome = .failure(error) }
                    updates.cancel()
                    await updates.value
                    mutation = try outcome.get()
                }
                result = .success(mutation)
            } catch {
                result = .failure((error as? BridgeError) ?? .internal("Deck name operation failed"))
            }
            guard current == revision, expected == owner.generation else { return }
            pendingResult = result
            await publishResult(action: action, revision: current)
        }
    }

    /// Both name entry surfaces use this collection-owned writer. The picker
    /// retains its reservation and committed result through its continuation.
    func createNamedDeck(_ draft: String) async throws -> DeckMutation {
        try await createDeck(draft)
    }

    private func publishResult(action: Action, revision current: UInt64) async {
        guard let owner, let result = pendingResult, current == revision,
              generation == owner.generation,
              owner.deckNamesAllowed(host: .main, route: .deckName(action)) else { return }
        switch result {
        case .success(let mutation):
            guard mutation.summary.generation == generation else {
                invalidate(); return
            }
            finalName = mutation.name
            do {
                if pendingPage == nil {
                    guard try await owner.refreshDeckNames(generation: generation), current == revision,
                          owner.deckNamesAllowed(host: .main, route: .deckName(action)) else { return }
                    if case .rename = action {
                        try await owner.refreshRenamedDeckPage(mutation)
                    } else {
                        let detail = try await owner.resolveDeckPage(mutation)
                        guard current == revision, generation == owner.generation,
                              owner.deckNamesAllowed(host: .main, route: .deckName(action)) else { return }
                        pendingPage = detail
                    }
                }
                let opened: Bool
                if case .rename = action {
                    opened = true
                } else if let pendingPage {
                    opened = await owner.openDeckPage(pendingPage).value
                } else {
                    return
                }
                guard current == revision, generation == owner.generation,
                      owner.deckNamesAllowed(host: .main, route: .deckName(action)) else { return }
                guard opened else {
                    message = "The deck was saved. Couldn’t open it. Try again."
                    cancelling = false
                    return
                }
                pendingResult = nil; pendingPage = nil; message = nil; cancelling = false
            } catch {
                // The write succeeded. Retry only the authoritative read, never the write.
                if current == revision, generation == owner.generation,
                   owner.deckNamesAllowed(host: .main, route: .deckName(action)) {
                    message = "The name was saved. Couldn’t refresh decks. Try again."
                }
            }
        case .failure(let error):
            message = Self.explanation(error); pendingResult = nil; cancelling = false
        }
    }

    /// Retry post-commit reads or selection without repeating the mutation.
    public func resumeResult() {
        guard !busy, pendingResult != nil, let action else { return }
        busy = true; message = nil
        let current = revision
        task = Task {
            await publishResult(action: action, revision: current)
            busy = false; task = nil
        }
    }

    public var needsRefresh: Bool { pendingResult != nil && !busy }
    public var needsOpen: Bool { pendingPage != nil }

    public func cancel() {
        if busy { requestCancellation(); return }
        // A pending successful result still belongs to this collection. Dismissing
        // acknowledges it; it never implies the write was rolled back.
        if let owner, let action, owner.navigation.sheet == .deckName(action), owner.navigation.sheetHost == .main {
            owner.navigation.dismissSheet()
        }
        action = nil; pendingResult = nil; pendingPage = nil
    }

    private func requestCancellation() {
        cancelling = true
        guard cancellationTask == nil else { return }
        let job = rename
        cancellationTask = Task {
            try? await job?.job.cancel()
            cancellationTask = nil
        }
    }

    func cancelAndDrain() async {
        if busy { requestCancellation() }
        await cancellationTask?.value
        await task?.value
        await cancellationTask?.value
    }

    func invalidate() {
        revision &+= 1
        if busy { requestCancellation() }
        if let owner, let action, owner.navigation.sheet == .deckName(action), owner.navigation.sheetHost == .main {
            owner.navigation.dismissSheet()
        }
        action = nil; name = ""; finalName = nil; message = nil; pendingResult = nil; pendingPage = nil
    }

    public func waitForCompletion() async { await task?.value }

    private func targetExists(_ action: Action, owner: ReviewCollection) -> Bool {
        switch action {
        case .create: true
        case .rename(let id): owner.fullDeckName(id) != nil
        case .subdeck(let id): owner.deck(id).map { !$0.filtered } ?? (owner.fullDeckName(id) != nil)
        }
    }

    static func explanation(_ error: BridgeError) -> String {
        switch error {
        case .deckRefused(.filteredParent): "A filtered deck can’t contain subdecks. Choose another path."
        case .deckRefused(.underDescendant): "A deck can’t be moved inside its own subdeck. Choose another path."
        case .deckRefused(.tooDeep): "This path has too many new parent decks. Choose a shorter path."
        case .cancelled: "The deck name operation was cancelled."
        case .notFound: "This deck is no longer available. Close this sheet and choose another deck."
        case .busy, .sessionActive: "The collection is busy. Finish the current operation, then try again."
        case .staleGeneration: "The collection changed. Close this sheet and try again."
        default: "Couldn’t save the deck name. Try again."
        }
    }
}
