import AppModel
import CoreBridge
import Observation
import SwiftUI

@Observable @MainActor
public final class GoToDeckModel {
    public var query = "" { didSet { reconcileSelection(); message = nil } }
    public var selectedID: Int64?
    public private(set) var snapshot: DeckNameSnapshot?
    public private(set) var loading = false
    public private(set) var busy = false
    public private(set) var message: Message?
    public enum Message: Equatable { case loadFailed, activationFailed, unavailable, filtered }
    private var presentation: UInt64?
    private var generation: UInt64 = 0
    private var revision: UInt64 = 0
    private var activation: Task<Void, Never>?
    private let read: @MainActor () async throws -> DeckNameSnapshot
    private let isCurrent: @MainActor (UInt64, UInt64) -> Bool
    private let activate: @MainActor (DeckNameEntry, UInt64, UInt64, ShellLayout) -> Task<ReviewCollection.GoToDeckResult, Never>
    private let dismiss: @MainActor (UInt64) -> Task<Bool, Never>

    init(read: @escaping @MainActor () async throws -> DeckNameSnapshot,
         isCurrent: @escaping @MainActor (UInt64, UInt64) -> Bool,
         activate: @escaping @MainActor (DeckNameEntry, UInt64, UInt64, ShellLayout) -> Task<ReviewCollection.GoToDeckResult, Never>,
         dismiss: @escaping @MainActor (UInt64) -> Task<Bool, Never>) {
        self.read = read; self.isCurrent = isCurrent; self.activate = activate; self.dismiss = dismiss
    }

    public convenience init(collection: ReviewCollection) {
        self.init(read: { try await CoreDeckService().allDeckNames() },
            isCurrent: { [weak collection] in collection?.goToDeckIsCurrent($0, generation: $1) == true },
            activate: { [weak collection] entry, generation, token, layout in
                collection?.activateGoToDeck(entry, generation: generation, presentation: token, layout: layout)
                    ?? Task { .unavailable }
            }, dismiss: { [weak collection] in collection?.cancelGoToDeck($0) ?? Task { false } })
    }

    public var matches: [DeckNameEntry] {
        let filter = GoToDeckFilter(query: query)
        return snapshot?.decks.filter { filter.matches(fullName: $0.fullName) } ?? []
    }
    private var current: Bool {
        presentation.map { isCurrent($0, generation) } == true
    }
    public var canSubmit: Bool {
        current && !loading && !busy && snapshot?.generation == generation &&
            matches.contains { $0.id == selectedID }
    }
    private func reconcileSelection() {
        let visible = matches
        if !visible.contains(where: { $0.id == selectedID }) { selectedID = visible.first?.id }
    }

    /// The view's task owns cancellation. A replacement layout can start a newer read.
    public func load(presentation token: UInt64, generation expected: UInt64) async {
        if presentation != token || generation != expected {
            query = ""; selectedID = nil; snapshot = nil
        }
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
        } catch {
            guard request == revision, current, !Task.isCancelled else { return }
            snapshot = nil; message = .loadFailed
        }
    }

    public func submit(layout: ShellLayout) {
        guard canSubmit, let token = presentation, let entry = matches.first(where: { $0.id == selectedID }) else { return }
        busy = true; message = nil
        let request = revision, draft = query
        // Owner admission and reservation happen before creating the waiting task.
        let result = activate(entry, generation, token, layout)
        activation = Task {
            let outcome = await result.value
            defer { busy = false; activation = nil }
            guard request == revision, current, draft == query else { return }
            switch outcome {
            case .opened: break // The owner dismisses only its originating presentation.
            case .filtered: message = .filtered
            case .unavailable: message = .unavailable
            case .failed: message = .activationFailed
            }
        }
    }
    public func cancel() {
        guard !busy, let presentation else { return }
        revision &+= 1; loading = false; busy = true
        let result = dismiss(presentation)
        activation = Task {
            let cancelled = await result.value
            busy = false; activation = nil
            if !cancelled, current { message = .activationFailed }
        }
    }
    func waitForActivation() async { await activation?.value }
}

public struct GoToDeckView: View {
    private let collection: ReviewCollection
    @Bindable private var model: GoToDeckModel
    @Environment(\.shellLayout) private var layout

    public init(collection: ReviewCollection) {
        self.collection = collection
        model = collection.navigation.featureState(GoToDeckModel.self) { GoToDeckModel(collection: collection) }
    }
    public var body: some View {
        NavigationStack {
            VStack {
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
                    List(selection: $model.selectedID) {
                        ForEach(model.matches, id: \.id) { entry in
                            Text(verbatim: entry.fullName)
                                .tag(entry.id)
                                .accessibilityIdentifier("home.goToDeck.deck.\(entry.id)")
                        }
                    }
                    .accessibilityIdentifier("home.goToDeck.list")
                    .overlay {
                        if model.matches.isEmpty {
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
                if model.busy { ProgressView().accessibilityIdentifier("home.goToDeck.progress") }
            }
            .searchable(text: $model.query, prompt: Text("Search Decks", bundle: .module))
            .disabled(model.busy)
            .onSubmit(of: .search) { model.submit(layout: layout) }
            #if os(macOS)
            .padding()
            #endif
            .navigationTitle(Text("Go to Deck", bundle: .module))
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
        .presentationSizing(.page)
        #endif
        .interactiveDismissDisabled(model.busy || collection.goToDeckBusy)
        .accessibilityIdentifier("home.goToDeck.sheet")
        .task(id: collection.goToDeckPresentation) { await reload() }
    }
    private func reload() async {
        guard let token = collection.goToDeckPresentation else { return }
        await model.load(presentation: token, generation: collection.generation)
    }
    private func messageText(_ message: GoToDeckModel.Message) -> Text {
        switch message {
        case .loadFailed: Text("Couldn’t read the decks. Try again.", bundle: .module)
        case .activationFailed: Text("Couldn’t open this deck. Try Go again, or cancel.", bundle: .module)
        case .unavailable: Text("The collection is busy or has changed. Cancel and open Go to Deck again.", bundle: .module)
        case .filtered: Text("Studying filtered decks isn’t supported yet. Choose a regular deck.", bundle: .module)
        }
    }
}
