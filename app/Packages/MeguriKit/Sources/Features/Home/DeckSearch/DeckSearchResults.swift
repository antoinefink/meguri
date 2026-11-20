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
        collection.deckSearchAvailable(layout: layout)
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

    /// "/" is a single key (H9.2, R197): never while a text input, a sheet or
    /// an open menu owns the keys, and never on a repeat.
    static func slashAdmitted() -> Bool {
        #if os(macOS)
        guard NSApp.isActive, let window = NSApp.keyWindow, window.attachedSheet == nil,
              !(window.firstResponder is NSTextInputClient),
              RunLoop.current.currentMode != .eventTracking,
              let event = NSApp.currentEvent, event.type == .keyDown, !event.isARepeat else { return false }
        #endif
        return true
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

/// The results of the current text over the D17 snapshot (H6.1), with the
/// layout's way to open one.
struct DeckSearchRows: View {
    let collection: ReviewCollection
    let search: DeckListSearch
    let results: DeckSearch?
    let layout: ShellLayout

    var body: some View {
        if let results {
            ForEach(results.matches) { match in
                Button { DeckSearchActivation.open(match.id, collection: collection, layout: layout) } label: {
                    DeckSearchRow(match: match)
                }
                .buttonStyle(.plain)
                .tag(DeckSearchTarget.deck(match.id))
                .accessibilityLabel(Text(verbatim: match.spokenPath))
                .accessibilityIdentifier("home.deckSearch.deck.\(match.id)")
            }
            DeckSearchTrailingRows(collection: collection, search: search, listed: !results.matches.isEmpty)
        }
        if search.snapshotFailed {
            // With the results of an earlier read, if any: Retry replaces them.
            HStack {
                Text("Couldn’t read the decks. Try again.", bundle: .module)
                    .foregroundStyle(.secondary)
                Spacer()
                Button { search.retrySnapshot() } label: { Text("Retry", bundle: .module) }
                    .accessibilityIdentifier("home.deckSearch.retryRead")
            }
            .accessibilityIdentifier("home.deckSearch.readFailed")
        }
    }

    /// The text's results, once a snapshot of this generation is read.
    static func results(_ collection: ReviewCollection, _ search: DeckListSearch, query: String) -> DeckSearch? {
        search.entries(generation: collection.generation)
            .map { DeckSearch(entries: $0, listsDefault: search.listsDefault, query: query) }
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
            }
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

/// Opening a result and the create row: one path for a click, a tap and Return.
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

/// Reads the snapshot (D17) and D18's name match for the current text while
/// results show.
private struct DeckSearchReads: ViewModifier {
    let collection: ReviewCollection
    let search: DeckListSearch
    func body(content: Content) -> some View {
        content
            .task(id: search.snapshotKey(generation: collection.generation, tree: collection.decks.hashValue)) {
                await search.loadSnapshot(search.snapshotKey(generation: collection.generation, tree: collection.decks.hashValue),
                    read: { try await CoreDeckService().allDeckNames() },
                    defaultHasCards: { !(try await CoreDeckService().studyState(deckID: 1).empty) })
            }
            .task(id: search.nameCheckKey(generation: collection.generation)) {
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
        List {
            DeckSearchRows(collection: collection, search: search,
                results: DeckSearchRows.results(collection, search, query: query), layout: .compact)
        }
        .modifier(DeckSearchReads(collection: collection, search: search))
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
    @State private var window = HostingWindow()
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
                    // H9.2: "/" from the sidebar's deck tree (`decks.list`), as from the deck page.
                    .onKeyPress(keys: [KeyEquivalent("/")], phases: .down) { press in
                        guard press.modifiers.isEmpty, DeckSearchFocus.available(collection, layout: .sidebar),
                              DeckSearchFocus.slashAdmitted() else { return .ignored }
                        DeckSearchFocus.request(collection, layout: .sidebar)
                        return .handled
                    }
            }
        }
        .searchable(text: $search.text, placement: .sidebar, prompt: Text("Search or Create a Deck", bundle: .module))
        .searchFocused($focused)
        #if !os(macOS)
        .onSubmit(of: .search) {
            // iPad: Return in the field opens the first row. (The Mac's Return is the monitor's.)
            guard let query = search.query, let target = targets(query).first else { return }
            DeckSearchActivation.activate(target, collection: collection, search: search, layout: .sidebar)
        }
        #endif
        .onChange(of: search.focusRequest, initial: true) { _, _ in
            guard search.focusPending else { return }
            focused = true
            search.focusTaken()
        }
        .onChange(of: collection.navigation.homeDeckPage) { _, deck in
            // The opened deck shows highlighted among the results.
            if let deck, search.query != nil { search.chosen = .deck(deck.rawValue) }
        }
        #if os(macOS)
        .background(HostingWindowReader(window: window))
        .onAppear { installArrows() }
        .onDisappear { removeArrows() }
        #endif
    }

    private func targets(_ query: String) -> [DeckSearchTarget] {
        DeckSearchRows.results(collection, search, query: query)?
            .targets(offeringCreate: search.offersCreate(generation: collection.generation)) ?? []
    }

    private func results(_ query: String) -> some View {
        List(selection: selection(query)) {
            DeckSearchRows(collection: collection, search: search,
                results: DeckSearchRows.results(collection, search, query: query), layout: .sidebar)
        }
        .modifier(DeckSearchReads(collection: collection, search: search))
        .accessibilityIdentifier("home.deckSearch.list")
    }

    /// Mac: the selection is the highlight; iPad: the opened deck. Only a row's
    /// own button or Return opens anything: selecting changes nothing else (N20.1).
    private func selection(_ query: String) -> Binding<DeckSearchTarget?> {
        Binding(get: {
            #if os(macOS)
            DeckSearch.highlight(search.chosen, in: targets(query))
            #else
            collection.navigation.homeDeckPage.map { .deck($0.rawValue) }
            #endif
        }, set: { target in
            #if os(macOS)
            if let target { search.chosen = target }
            #endif
        })
    }

    #if os(macOS)
    /// ↑ and ↓ in this sidebar's field move the highlight (H6.3), and Return opens
    /// the highlighted row; the field keeps the focus. In the Mac VM run, Return
    /// in the sidebar's field opened nothing through `onSubmit(of: .search)`, so
    /// the field's own keys are read here. Only this window's field: never Settings.
    private func installArrows() {
        guard arrows == nil else { return }
        arrows = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [search, collection, window] event in
            guard let host = window.value, let eventWindow = event.window, eventWindow === host, host.isKeyWindow,
                  host.attachedSheet == nil,
                  let editor = host.firstResponder as? NSTextView, editor.isFieldEditor,
                  let field = editor.delegate as? NSSearchField, field.window === host,
                  let key = DeckSearchFieldKey(keyCode: event.keyCode,
                      modifiers: event.modifierFlags.intersection([.command, .option, .control, .shift]),
                      repeated: event.isARepeat, composing: editor.hasMarkedText()),
                  let query = search.query else { return event }
            let targets = DeckSearchRows.results(collection, search, query: query)?
                .targets(offeringCreate: search.offersCreate(generation: collection.generation)) ?? []
            let current = DeckSearch.highlight(search.chosen, in: targets)
            if key == .open {
                // With no row at all, Return does nothing (H6.3).
                if let current { DeckSearchActivation.activate(current, collection: collection, search: search, layout: .sidebar) }
            } else {
                search.chosen = DeckSearch.move(current, by: key == .down ? 1 : -1, in: targets)
            }
            return nil
        }
    }
    private func removeArrows() {
        if let arrows { NSEvent.removeMonitor(arrows) }
        arrows = nil
    }
    #endif
}

#if os(macOS)
/// The sidebar field's own keys on the Mac (H6.3): ↑ and ↓ move the highlight,
/// Return opens it. Any modifier and a repeated Return leave the key alone, and
/// while an input method composes text (Japanese, Chinese) every key is its own.
enum DeckSearchFieldKey: Equatable {
    case up, down, open
    init?(keyCode: UInt16, modifiers: NSEvent.ModifierFlags, repeated: Bool, composing: Bool) {
        guard modifiers.isEmpty, !composing else { return nil }
        switch keyCode {
        case 125: self = .down
        case 126: self = .up
        case 36 where !repeated, 76 where !repeated: self = .open
        default: return nil
        }
    }
}

/// A weak reference to the hosting NSWindow.
final class HostingWindow {
    weak var value: NSWindow?
}

private struct HostingWindowReader: NSViewRepresentable {
    let window: HostingWindow
    func makeNSView(context: Context) -> ReaderView {
        let view = ReaderView()
        view.holder = window
        return view
    }
    func updateNSView(_ view: ReaderView, context: Context) { view.holder = window; window.value = view.window }
    final class ReaderView: NSView {
        var holder: HostingWindow?
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            holder?.value = window
        }
    }
}
#endif
