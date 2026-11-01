import AppModel
import CoreBridge
import Observation
import SwiftUI

@Observable @MainActor
public final class GoToDeckModel {
    public var query = "" {
        didSet {
            guard query != oldValue else { return }
            if hasSavedResult { query = savedQuery ?? oldValue; return }
            invalidateLookup(); message = nil; reconcileSelection(); scheduleLookup()
        }
    }
    public nonisolated enum Selection: Hashable, Sendable { case deck(Int64), create }
    public var selection: Selection?
    public var selectedID: Int64? {
        get { if case .deck(let id) = selection { id } else { nil } }
        set { selection = newValue.map(Selection.deck) }
    }
    public private(set) var snapshot: DeckNameSnapshot?
    public private(set) var loading = false
    public private(set) var busy = false
    public private(set) var checkingName = false
    public private(set) var hasSavedResult = false
    public var liveLayout: ShellLayout = .sidebar
    public private(set) var message: Message?
    public enum Message: Equatable { case loadFailed, lookupFailed, activationFailed, unavailable, filtered, filteredParent, tooDeep, saved }
    private var presentation: UInt64?
    private var generation: UInt64 = 0
    private var revision: UInt64 = 0
    private var activation: Task<Void, Never>?
    private var lookup: Task<Void, Never>?
    private var lookupRevision: UInt64 = 0
    private var canonicalMatch: DeckNameMatch?
    private var savedQuery: String?
    private var savedMatch: DeckNameMatch?
    private let read: @MainActor () async throws -> DeckNameSnapshot
    private let isCurrent: @MainActor (UInt64, UInt64) -> Bool
    private let activate: @MainActor (DeckNameEntry, UInt64, UInt64, ShellLayout) -> Task<ReviewCollection.GoToDeckResult, Never>
    private let dismiss: @MainActor (UInt64) -> Task<Bool, Never>
    private let matchName: (@MainActor (String) async throws -> DeckNameMatch)?
    private let create: @MainActor (String, DeckNameMatch, UInt64, [DeckNameEntry], @escaping @MainActor () -> ShellLayout) -> Task<ReviewCollection.GoToDeckResult, Never>

    init(read: @escaping @MainActor () async throws -> DeckNameSnapshot,
         isCurrent: @escaping @MainActor (UInt64, UInt64) -> Bool,
         activate: @escaping @MainActor (DeckNameEntry, UInt64, UInt64, ShellLayout) -> Task<ReviewCollection.GoToDeckResult, Never>,
         dismiss: @escaping @MainActor (UInt64) -> Task<Bool, Never>,
         matchName: (@MainActor (String) async throws -> DeckNameMatch)? = nil,
         create: @escaping @MainActor (String, DeckNameMatch, UInt64, [DeckNameEntry], @escaping @MainActor () -> ShellLayout) -> Task<ReviewCollection.GoToDeckResult, Never> = { _, _, _, _, _ in Task { .unavailable } }) {
        self.read = read; self.isCurrent = isCurrent; self.activate = activate; self.dismiss = dismiss
        self.matchName = matchName; self.create = create
    }

    public convenience init(collection: ReviewCollection) {
        self.init(read: { try await CoreDeckService().allDeckNames() },
            isCurrent: { [weak collection] in collection?.goToDeckIsCurrent($0, generation: $1) == true },
            activate: { [weak collection] entry, generation, token, layout in
                collection?.activateGoToDeck(entry, generation: generation, presentation: token, layout: layout)
                    ?? Task { .unavailable }
            }, dismiss: { [weak collection] in collection?.cancelGoToDeck($0) ?? Task { false } },
            matchName: { try await CoreDeckService().matchDeckName(name: $0) },
            create: { [weak collection] query, match, token, decks, layout in
                collection?.createGoToDeck(name: query, match: match, presentation: token, knownDecks: decks, layout: layout)
                    ?? Task { .unavailable }
            })
    }

    public var matches: [DeckNameEntry] {
        let filter = GoToDeckFilter(query: query)
        return snapshot?.decks.filter { filter.matches(fullName: $0.fullName) } ?? []
    }
    private var current: Bool {
        presentation.map { isCurrent($0, generation) } == true
    }
    public var canSubmit: Bool {
        guard current, !loading, !busy, snapshot?.generation == generation else { return false }
        if hasSavedResult { return true }
        switch selection {
        case .deck(let id): return matches.contains { $0.id == id }
        case .create: return canCreate
        case nil: return false
        }
    }
    public var canCreate: Bool {
        current && !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
            snapshot?.generation == generation && canonicalMatch?.generation == generation &&
            canonicalMatch?.deck == nil && canonicalMatch != nil && !checkingName
    }
    private func reconcileSelection() {
        let visible = matches
        if selection == .create, canCreate { return }
        if !visible.contains(where: { $0.id == selectedID }) {
            selection = visible.first.map { .deck($0.id) } ?? (canCreate ? .create : nil)
        }
    }

    private func invalidateLookup() {
        lookupRevision &+= 1; lookup?.cancel(); lookup = nil
        canonicalMatch = nil; checkingName = false
    }
    private func scheduleLookup() {
        guard let matchName, current, snapshot?.generation == generation,
              !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, !hasSavedResult else { return }
        checkingName = true
        let request = lookupRevision, draft = query, token = presentation, expected = generation
        lookup = Task {
            defer { if request == lookupRevision { checkingName = false; lookup = nil } }
            do {
                let result = try await matchName(draft)
                guard request == lookupRevision, draft == query, token == presentation,
                      expected == generation, current, !Task.isCancelled else { return }
                guard result.generation == expected else { message = .unavailable; return }
                canonicalMatch = result; checkingName = false; reconcileSelection()
            } catch {
                guard request == lookupRevision, draft == query, token == presentation,
                      expected == generation, current, !Task.isCancelled else { return }
                message = .lookupFailed
            }
        }
    }
    public func retryLookup() {
        invalidateLookup(); message = nil; scheduleLookup()
    }

    /// The view's task owns cancellation. A replacement layout can start a newer read.
    public func load(presentation token: UInt64, generation expected: UInt64) async {
        if presentation == token, generation == expected, busy || hasSavedResult { return }
        if presentation != token || generation != expected {
            hasSavedResult = false; savedQuery = nil; savedMatch = nil
            query = ""; selectedID = nil; snapshot = nil
        }
        invalidateLookup()
        presentation = token; generation = expected
        revision &+= 1
        let request = revision
        guard current else { loading = false; return }
        loading = true; message = nil
        defer { if request == revision { loading = false } }
        do {
            let result = try await read()
            guard request == revision, current, !Task.isCancelled else { return }
            guard result.generation == expected else { snapshot = nil; message = .unavailable; return }
            snapshot = result
            reconcileSelection()
            scheduleLookup()
        } catch {
            guard request == revision, current, !Task.isCancelled else { return }
            snapshot = nil; message = .loadFailed
        }
    }

    public func submit(layout: ShellLayout) {
        guard canSubmit, let token = presentation else { return }
        liveLayout = layout
        busy = true; message = nil
        let request = revision, draft = query
        // Owner admission and reservation happen before creating the waiting task.
        let result: Task<ReviewCollection.GoToDeckResult, Never>
        if hasSavedResult || selection == .create {
            guard let match = savedMatch ?? canonicalMatch else { busy = false; return }
            savedQuery = savedQuery ?? query; savedMatch = match
            result = create(savedQuery ?? query, match, token, snapshot?.decks ?? [], { [weak self] in self?.liveLayout ?? layout })
        } else if let entry = matches.first(where: { $0.id == selectedID }) {
            result = activate(entry, generation, token, layout)
        } else { busy = false; return }
        activation = Task {
            let outcome = await result.value
            defer {
                busy = false; activation = nil
                if !hasSavedResult { savedQuery = nil; savedMatch = nil }
            }
            guard request == revision, current else { return }
            if outcome == .saved {
                hasSavedResult = true
                query = savedQuery ?? draft
                invalidateLookup(); selection = .create; message = .saved
                return
            }
            guard draft == query else { return }
            switch outcome {
            case .opened: break // The owner dismisses only its originating presentation.
            case .filtered: message = .filtered
            case .filteredParent: message = .filteredParent
            case .tooDeep: message = .tooDeep
            case .unavailable: message = .unavailable
            case .failed: message = .activationFailed
            case .saved: hasSavedResult = true; message = .saved
            }
        }
    }
    public func cancel() {
        guard !busy, let presentation else { return }
        revision &+= 1; loading = false; busy = true
        invalidateLookup()
        let result = dismiss(presentation)
        activation = Task {
            let cancelled = await result.value
            busy = false; activation = nil
            if !cancelled, current { message = .activationFailed }
        }
    }
    func waitForActivation() async { await activation?.value }
    func waitForLookup() async { await lookup?.value }
}

public struct GoToDeckView: View {
    private let collection: ReviewCollection
    @Bindable private var model: GoToDeckModel
    @Environment(\.shellLayout) private var layout
    #if os(macOS)
    private enum InputFocus: Hashable { case query, results }
    @FocusState private var inputFocus: InputFocus?
    #endif

    public init(collection: ReviewCollection) {
        self.collection = collection
        model = collection.navigation.featureState(GoToDeckModel.self) { GoToDeckModel(collection: collection) }
    }
    public var body: some View {
        SheetContainer {
            VStack {
                #if os(macOS)
                TextField(text: $model.query, prompt: Text("Search Decks", bundle: .module)) {
                    Text("Search Decks", bundle: .module)
                }
                .accessibilityIdentifier("home.goToDeck.search")
                .disabled(model.hasSavedResult)
                .focused($inputFocus, equals: .query)
                .onAppear { inputFocus = .query }
                .onSubmit(of: .text) { model.submit(layout: layout) }
                #endif
                if model.loading {
                    ProgressView { Text("Loading decks…", bundle: .module) }
                        .accessibilityIdentifier("home.goToDeck.loading")
                } else if model.snapshot == nil {
                    ContentUnavailableView {
                        Label { Text("Couldn’t Load Decks", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
                    } actions: {
                        Button { Task { await reload() } } label: { Text("Retry", bundle: .module) }
                            .accessibilityIdentifier("home.goToDeck.retry")
                    }
                } else {
                    List(selection: $model.selection) {
                        ForEach(model.matches, id: \.id) { entry in
                            Text(verbatim: entry.fullName)
                                .tag(GoToDeckModel.Selection.deck(entry.id))
                                .accessibilityIdentifier("home.goToDeck.deck.\(entry.id)")
                        }
                        if model.canCreate {
                            Text("Create deck named \(model.query)", bundle: .module)
                                .tag(GoToDeckModel.Selection.create)
                                .accessibilityIdentifier("home.goToDeck.create")
                        }
                    }
                    .disabled(model.hasSavedResult)
                    #if os(macOS)
                    .focused($inputFocus, equals: .results)
                    .simultaneousGesture(TapGesture().onEnded {
                        inputFocus = .results
                    })
                    #endif
                    .accessibilityIdentifier("home.goToDeck.list")
                    .overlay {
                        if model.matches.isEmpty && !model.canCreate {
                            ContentUnavailableView {
                                Label { Text("No Matching Decks", bundle: .module) } icon: { Image(systemName: "magnifyingglass") }
                            }
                        }
                    }
                }
                if let message = model.message {
                    messageText(message)
                        .accessibilityIdentifier("home.goToDeck.message")
                }
                if model.checkingName { ProgressView().accessibilityIdentifier("home.goToDeck.checkingName") }
                if model.message == .lookupFailed {
                    Button { model.retryLookup() } label: { Text("Retry", bundle: .module) }
                        .accessibilityIdentifier("home.goToDeck.retryLookup")
                }
                if model.busy { ProgressView().accessibilityIdentifier("home.goToDeck.progress") }
            }
            #if !os(macOS)
            .searchable(text: $model.query, prompt: Text("Search Decks", bundle: .module))
            #endif
            .disabled(model.busy)
            #if os(macOS)
            .padding()
            #else
            .onSubmit(of: .search) { model.submit(layout: layout) }
            #endif
            #if !os(macOS)
            .navigationTitle(Text("Go to Deck", bundle: .module))
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button { model.cancel() } label: { Text("Cancel", bundle: .module) }
                        .keyboardShortcut(.cancelAction)
                        .disabled(model.busy)
                        .accessibilityIdentifier("home.goToDeck.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button { model.submit(layout: layout) } label: { Text("Go", bundle: .module) }
                        .keyboardShortcut(.defaultAction)
                        .disabled(!model.canSubmit)
                        .accessibilityIdentifier("home.goToDeck.go")
                }
            }
        }
        #if os(macOS)
        .frame(width: 480, height: 420)
        #endif
        .interactiveDismissDisabled(model.busy || collection.goToDeckBusy)
        #if os(macOS)
        // Without a navigation stack, the sheet's identifier needs its own container.
        .accessibilityElement(children: .contain)
        #endif
        .accessibilityIdentifier("home.goToDeck.sheet")
        .onChange(of: layout, initial: true) { _, value in model.liveLayout = value }
        .task(id: collection.goToDeckPresentation) { await reload() }
    }
    private func reload() async {
        guard let token = collection.goToDeckPresentation else { return }
        await model.load(presentation: token, generation: collection.generation)
    }
    private func messageText(_ message: GoToDeckModel.Message) -> Text {
        switch message {
        case .loadFailed: Text("Couldn’t read the decks. Try again.", bundle: .module)
        case .lookupFailed: Text("Couldn’t check this deck name. Try again.", bundle: .module)
        case .activationFailed: Text("Couldn’t open this deck. Try Go again, or cancel.", bundle: .module)
        case .unavailable: Text("The collection is busy or has changed. Cancel and open Go to Deck again.", bundle: .module)
        case .filtered: Text("Studying filtered decks isn’t supported yet. Choose a regular deck.", bundle: .module)
        case .filteredParent: Text("A filtered deck can’t contain subdecks. Choose another path.", bundle: .module)
        case .tooDeep: Text("This path has too many new parent decks. Choose a shorter path.", bundle: .module)
        case .saved: Text("The deck was saved. Couldn’t open it. Try Go again, or cancel.", bundle: .module)
        }
    }
}

/// A Mac sheet has no navigation bar; its toolbar actions sit along the bottom.
private struct SheetContainer<Content: View>: View {
    @ViewBuilder let content: () -> Content
    var body: some View {
        #if os(macOS)
        content()
        #else
        NavigationStack { content() }
        #endif
    }
}
