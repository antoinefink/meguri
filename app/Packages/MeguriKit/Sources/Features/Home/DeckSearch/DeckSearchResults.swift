import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// Find…, ⇧⌘O and "/" (N20.1, N20.4): the focus goes to the sidebar's field,
/// or, in the tab layout, the deck list's Search is presented.
@MainActor public enum DeckSearchFocus {
    public static func available(_ collection: ReviewCollection, layout: ShellLayout) -> Bool {
        guard case .ready = collection.state, collection.review == nil, collection.navigation.review == nil,
              collection.navigation.sheet == nil else { return false }
        // The tab layout's Search belongs to its deck list (N14.2).
        return layout == .sidebar || !collection.decks.isEmpty
    }

    public static func request(_ collection: ReviewCollection, layout: ShellLayout) {
        guard available(collection, layout: layout) else { return }
        let search = search(collection.navigation)
        if layout == .compact {
            collection.navigation.openHome()
            search.presented = true
        } else {
            // A hidden sidebar comes back first.
            collection.navigation.columnVisibility = .all
            search.requestFocus()
        }
    }

    static func search(_ navigation: AppNavigation) -> DeckListSearch {
        navigation.featureState(DeckListSearch.self) { DeckListSearch() }
    }
}

/// A result: the deck's own name over its parents, joined with " › " (N20.5).
struct DeckSearchRow: View {
    let match: DeckSearch.Match
    var body: some View {
        VStack(alignment: .leading) {
            Text(verbatim: match.name)
            if !match.ancestors.isEmpty {
                Text(verbatim: match.ancestors.joined(separator: " › "))
                    .font(.caption)
                    .quietText()
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }
}

/// What follows the results: the create row once D18's read found no deck with
/// the typed name, its failure, or H6.3's one-line message when nothing is listed.
struct DeckSearchTrailingRows: View {
    let collection: ReviewCollection
    let search: DeckListSearch
    let listed: Bool
    var body: some View {
        let generation = collection.generation
        if search.offersCreate(generation: generation) {
            Button { DeckSearchActivation.create(collection, search: search) } label: {
                Label { Text("Create deck named \(search.text)", bundle: .module) } icon: { Image(systemName: "plus") }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(.tint)
            .disabled(!collection.deckActions.canPresent(.create))
            .tag(DeckSearchTarget.create)
            .accessibilityIdentifier("home.deckSearch.create")
        } else if search.nameCheck(generation: generation) == .failed {
            HStack {
                Text("Couldn’t check this deck name. Try again.", bundle: .module)
                    .foregroundStyle(.secondary)
                Spacer()
                Button { search.retryNameCheck() } label: { Text("Retry", bundle: .module) }
                    .accessibilityIdentifier("home.deckSearch.retry")
            }
            .accessibilityIdentifier("home.deckSearch.checkFailed")
        } else if !listed, search.nameCheck(generation: generation) == .exists {
            Text("No Matching Decks", bundle: .module)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("home.deckSearch.empty")
        }
    }
}

/// Opening a result and the create row, the same from a click, a tap and Return.
@MainActor enum DeckSearchActivation {
    static func open(_ id: Int64, collection: ReviewCollection, layout: ShellLayout) {
        // Every layout opens the deck page, never a review (N20.5).
        if layout == .compact {
            collection.navigation.select(.deck(DeckID(id)))
        } else {
            collection.openDeckPage(deckID: id)
        }
    }
    static func create(_ collection: ReviewCollection, search: DeckListSearch) {
        // D18: the New Deck form with the raw query as its Name, Parent Deck None.
        collection.deckActions.present(.create, name: search.text)
    }
    static func activate(_ target: DeckSearchTarget, collection: ReviewCollection, search: DeckListSearch, layout: ShellLayout) {
        switch target {
        case .deck(let id): open(id, collection: collection, layout: layout)
        case .create: create(collection, search: search)
        }
    }
}

/// Runs D18's name read for the current text and generation while results show.
private struct DeckNameCheckTask: ViewModifier {
    let collection: ReviewCollection
    let search: DeckListSearch
    func body(content: Content) -> some View {
        content.task(id: search.nameCheckKey(generation: collection.generation)) {
            await search.checkName(search.nameCheckKey(generation: collection.generation)) {
                try await CoreDeckService().matchDeckName(name: $0)
            }
        }
    }
}

/// The tab layout's results in place of the tree (N14.2): a tap opens the deck page.
struct HomeDeckSearchResults: View {
    let collection: ReviewCollection
    let search: DeckListSearch
    let query: String

    var body: some View {
        let results = DeckSearch(nodes: collection.decks, query: query)
        List {
            ForEach(results.matches) { match in
                Button { DeckSearchActivation.open(match.id, collection: collection, layout: .compact) } label: {
                    DeckSearchRow(match: match)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text(verbatim: match.spokenPath))
                .accessibilityIdentifier("home.deckSearch.deck.\(match.id)")
            }
            DeckSearchTrailingRows(collection: collection, search: search, listed: !results.matches.isEmpty)
        }
        .modifier(DeckNameCheckTask(collection: collection, search: search))
        .accessibilityIdentifier("home.deckSearch.list")
    }
}

/// N20.1: the sidebar's search field. While it holds text, the results take the
/// whole sidebar, Today row included; Esc clears it and the tree returns.
public struct HomeSidebarDeckSearch<Content: View>: View {
    private let collection: ReviewCollection
    @Bindable private var search: DeckListSearch
    @ViewBuilder private let content: () -> Content
    @FocusState private var focused: Bool
    #if os(macOS)
    @State private var arrows: Any?
    #endif

    public init(collection: ReviewCollection, @ViewBuilder content: @escaping () -> Content) {
        self.collection = collection
        search = DeckSearchFocus.search(collection.navigation)
        self.content = content
    }

    public var body: some View {
        Group {
            if let query = search.query {
                results(query)
            } else {
                content()
            }
        }
        .searchable(text: $search.text, placement: .sidebar, prompt: Text("Search or Create a Deck", bundle: .module))
        .searchFocused($focused)
        .onSubmit(of: .search) {
            guard let query = search.query else { return }
            let targets = targets(query)
            #if os(macOS)
            // Return opens the highlighted row.
            let target = DeckSearch.highlight(search.chosen, in: targets)
            #else
            // iPad: Return in the field opens the first row.
            let target = targets.first
            #endif
            if let target { DeckSearchActivation.activate(target, collection: collection, search: search, layout: .sidebar) }
        }
        .onChange(of: search.focusRequest) { _, _ in focused = true }
        .onChange(of: collection.navigation.homeDeckPage) { _, deck in
            // The opened deck shows highlighted among the results.
            if let deck, search.query != nil { search.chosen = .deck(deck.rawValue) }
        }
        #if os(macOS)
        .onAppear { installArrows() }
        .onDisappear { removeArrows() }
        #endif
    }

    private func targets(_ query: String) -> [DeckSearchTarget] {
        DeckSearch(nodes: collection.decks, query: query)
            .targets(offeringCreate: search.offersCreate(generation: collection.generation))
    }

    private func results(_ query: String) -> some View {
        let results = DeckSearch(nodes: collection.decks, query: query)
        return List(selection: selection(results)) {
            ForEach(results.matches) { match in
                Button { DeckSearchActivation.open(match.id, collection: collection, layout: .sidebar) } label: {
                    DeckSearchRow(match: match)
                }
                .buttonStyle(.plain)
                .tag(DeckSearchTarget.deck(match.id))
                .accessibilityLabel(Text(verbatim: match.spokenPath))
                .accessibilityIdentifier("home.deckSearch.deck.\(match.id)")
            }
            DeckSearchTrailingRows(collection: collection, search: search, listed: !results.matches.isEmpty)
        }
        .modifier(DeckNameCheckTask(collection: collection, search: search))
        .accessibilityIdentifier("home.deckSearch.list")
    }

    /// Mac: the selection is the highlight, which the arrows move without
    /// opening anything. iPad: the opened deck, with no highlight.
    private func selection(_ results: DeckSearch) -> Binding<DeckSearchTarget?> {
        Binding(get: {
            #if os(macOS)
            DeckSearch.highlight(search.chosen, in: results.targets(offeringCreate: search.offersCreate(generation: collection.generation)))
            #else
            collection.navigation.homeDeckPage.map { .deck($0.rawValue) }
            #endif
        }, set: { target in
            guard let target else { return }
            search.chosen = target
            DeckSearchActivation.activate(target, collection: collection, search: search, layout: .sidebar)
        })
    }

    #if os(macOS)
    /// ↑ and ↓ in the field move the highlight (H6.3); the field keeps the focus.
    /// ⇧⌘O is Find…'s second key, with no menu item of its own (N20.4): as a
    /// menu key equivalent it also acts while a text field has the focus.
    private func installArrows() {
        guard arrows == nil else { return }
        arrows = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [search, collection] event in
            let flags = event.modifierFlags.intersection([.command, .option, .control, .shift])
            if flags == [.command, .shift], event.charactersIgnoringModifiers?.lowercased() == "o",
               let window = event.window, window.isKeyWindow, window.attachedSheet == nil {
                guard !event.isARepeat, DeckSearchFocus.available(collection, layout: .sidebar) else { return event }
                DeckSearchFocus.request(collection, layout: .sidebar)
                return nil
            }
            guard event.keyCode == 125 || event.keyCode == 126,
                  event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty,
                  let window = event.window, window.isKeyWindow, window.attachedSheet == nil,
                  let editor = window.firstResponder as? NSTextView, editor.isFieldEditor,
                  editor.delegate is NSSearchField, let query = search.query else { return event }
            let targets = DeckSearch(nodes: collection.decks, query: query)
                .targets(offeringCreate: search.offersCreate(generation: collection.generation))
            let current = DeckSearch.highlight(search.chosen, in: targets)
            search.chosen = DeckSearch.move(current, by: event.keyCode == 125 ? 1 : -1, in: targets)
            return nil
        }
    }
    private func removeArrows() {
        if let arrows { NSEvent.removeMonitor(arrows) }
        arrows = nil
    }
    #endif
}
