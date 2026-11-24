import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The sidebar while Browse is selected (`browser/workspace` V6, N21.1):
/// the Today and Browse rows the shell gives (`header`), then the filters
/// in folding sections, each value row with its count (T12.3). The rows
/// are a view of the query: the system's selection shows the values of
/// its **is** tokens, and a click, ⌘-click or ⌥-click changes the query as
/// `search/filter-tokens` T9 says. The sidebar's field filters the rows
/// (`browser/sidebar` S6). Since 2026-11-24 (N21.1) the list's own selection
/// is the Browse row alone: a filter row the query selects draws a tinted
/// background and a checkmark itself, and each row leads with a symbol.
public struct BrowseSidebar<Header: View>: View {
    @Bindable private var model: BrowseModel
    private let decks: [CountedDeckNode]
    private let deckExpanded: (Int64) -> Binding<Bool>
    private let navigate: (AppSection) -> Void
    private let header: Header

    /// `decks` is the deck tree in Today's order and `deckExpanded` its
    /// expansion, the same as Today's (V6); `navigate` selects a section row.
    public init(model: BrowseModel, decks: [CountedDeckNode], deckExpanded: @escaping (Int64) -> Binding<Bool>,
                navigate: @escaping (AppSection) -> Void, @ViewBuilder header: () -> Header) {
        self.model = model
        self.decks = decks
        self.deckExpanded = deckExpanded
        self.navigate = navigate
        self.header = header()
    }

    private var filter: BrowseSidebarFilter { BrowseSidebarFilter(model.filter) }
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    @Environment(\.appClock) private var clock

    public var body: some View {
        List(selection: $selection) {
            Section { header }
            if filter.isOn && !anyMatch {
                Text("browse.sidebar.noMatches", bundle: .module)
                    .foregroundStyle(.secondary)
                    .selectionDisabled()
                    .accessibilityIdentifier("browse.sidebar.noMatches")
            }
            savedSearches
            cardStates
            flags
            deckSection
            notetypes
            tags
        }
        // The list's own row actions: the row menus, and its primary action, which
        // macOS runs on Return (and a double click). T9: Return on the one selected
        // filter row is a second click on it. A double click is two clicks already.
        #if os(macOS)
        .contextMenu(forSelectionType: BrowseSidebarTag.self) { rows in
            rowMenu(rows)
        } primaryAction: { _ in
            guard NSApp.currentEvent?.type == .keyDown else { return }
            activateByKey()
        }
        #else
        // On touch a tap is the selection's (a primary action would take every tap).
        .contextMenu(forSelectionType: BrowseSidebarTag.self) { rows in rowMenu(rows) }
        #endif
        // T9: Space on the one selected filter row, as Return.
        .onKeyPress(.space) { activateByKey() ? .handled : .ignored }
        // N20.6 (2026-11-22): the system's own prompt, "Search", as with Today selected.
        .searchable(text: $model.filter, placement: .sidebar)
        .accessibilityIdentifier("browse.sidebar.list")
        .modifier(RenameSavedSearch(model: model))
        .onAppear {
            selection = model.listSelection
            model.useFormatter(locale: locale, timeZone: timeZone, clock: clock)
            model.appear()
        }
        .onDisappear { model.disappear() }
        .onChange(of: selection) { _, rows in selectionChanged(rows) }
        // The rows shown changed: Return and Space never act on a row out of sight.
        .onChange(of: model.filter) { forgetKeyRow() }
        .onChange(of: model.folded) { forgetKeyRow() }
    }

    /// V6: a user saved search's Rename… and Delete (T13; not the built-ins),
    /// a note type's Edit Note Type… (W14). Other rows have no menu.
    @ViewBuilder private func rowMenu(_ rows: Set<BrowseSidebarTag>) -> some View {
        if rows.count == 1, let row = rows.first {
            switch row {
            case .saved(let name):
                Button { model.startRenaming(name) } label: { Text("browse.savedSearch.rename", bundle: .module) }
                    .accessibilityIdentifier("browse.sidebar.saved.rename")
                Button { model.deleteSavedSearch(name) } label: { Text("browse.savedSearch.delete", bundle: .module) }
                    .accessibilityIdentifier("browse.sidebar.saved.delete")
            case .notetype(let id):
                Button { model.editNoteType(id) } label: { Text("browse.columns.editNoteType", bundle: .module) }
                    .accessibilityIdentifier("browse.sidebar.notetype.edit")
            default:
                EmptyView()
            }
        }
    }

    // MARK: Selection (T9)

    /// The list's own selection: the Browse row, or the row the arrow keys
    /// stand on. A click changes it, then the query (T9), then it is set
    /// back to the Browse row after that event: the list keeps a selection
    /// it made itself, and the rows draw the query's values (`active`).
    @State private var selection: Set<BrowseSidebarTag> = []
    /// The filter row the arrow keys moved to: Return and Space are a click on it.
    @State private var keyRow: BrowseSidebarTag?
    /// The filter row clicked last: Return and Space are a second click on it (T9).
    @State private var clickedRow: BrowseSidebarTag?

    private func selectionChanged(_ rows: Set<BrowseSidebarTag>) {
        guard rows != model.listSelection else { return keyRow = nil }
        if let keyRow, rows == [keyRow] { return }
        // A ⌘-click on the row the keys stand on empties the list's selection.
        if let keyRow, rows.isEmpty, !Self.byKey() { return clicked(keyRow) }
        let filters = rows.filter { if case .section = $0 { false } else { true } }
        // The arrow keys move through the rows without changing the query.
        if Self.byKey(), filters.count == 1, filters == rows, let row = filters.first {
            keyRow = row
            return
        }
        // The row the keys stood on is still in the list's selection after a
        // ⌘-click on another row; it was not clicked.
        var rows = rows
        if let keyRow, rows.count > 1 { rows.remove(keyRow) }
        let clicked = rows.filter { if case .section = $0 { false } else { true } }
        keyRow = nil
        clickedRow = clicked.count == 1 ? clicked.first : nil
        if let section = model.listSelectionChanged(to: rows, click: Self.click()) {
            navigate(section)
            return
        }
        Task { @MainActor in
            if keyRow == nil, selection != model.listSelection { selection = model.listSelection }
        }
    }

    /// A click on the row the arrow keys stand on. A plain or ⌥-click leaves
    /// the list's selection as it is, so the list reports nothing and the
    /// row's own tap does (`row`); it is T9's click on that row all the same.
    private func clicked(_ row: BrowseSidebarTag) {
        guard keyRow == row else { return }
        keyRow = nil
        clickedRow = row
        model.activate(row, click: Self.click())
        if selection != model.listSelection { selection = model.listSelection }
    }

    private func forgetKeyRow() {
        keyRow = nil
        clickedRow = nil
        if selection != model.listSelection { selection = model.listSelection }
    }

    /// T9: Return or Space on the row the keys stand on is a click on it;
    /// after a click, a second click on that row where it removes its filter.
    @discardableResult private func activateByKey() -> Bool {
        if let keyRow {
            model.activate(keyRow)
            return true
        }
        guard let clickedRow else { return false }
        return model.activateByKey([clickedRow])
    }

    /// Whether a key, not a click, changed the selection.
    private static func byKey() -> Bool {
        #if os(macOS)
        NSApp.currentEvent?.type == .keyDown
        #else
        false
        #endif
    }

    /// The modifier keys of the click that changed the selection.
    private static func click() -> BrowseModel.Click {
        #if os(macOS)
        let flags = NSEvent.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if flags.contains(.command) { return .command }
        if flags.contains(.option) { return .option }
        #endif
        return .plain
    }

    // MARK: Sections

    /// A folding section (V6), unfolded while the filter shows its matches;
    /// absent while the filter matches nothing in it.
    @ViewBuilder
    private func section<Content: View>(_ kind: BrowseSidebarSection, shown: Bool,
                                        @ViewBuilder content: () -> Content) -> some View {
        if shown {
            Section(isExpanded: Binding(get: { filter.isOn || !model.folded.contains(kind) }, set: { expanded in
                guard !filter.isOn else { return }
                if expanded { model.folded.remove(kind) } else { model.folded.insert(kind) }
            })) {
                content()
            } header: {
                Text(verbatim: BrowseStrings.section(kind))
                    .accessibilityIdentifier("browse.sidebar.\(kind.rawValue)")
                    .contextMenu { sectionMenu(kind) }
            }
        }
    }

    /// T13: the Saved Searches section's Save Search… (the query's, while
    /// it is not empty). The other sections have no menu in this version.
    @ViewBuilder private func sectionMenu(_ kind: BrowseSidebarSection) -> some View {
        if kind == .savedSearches {
            Button { model.startSavingSearch() } label: { Text("browse.search.saveSearch", bundle: .module) }
                .disabled(!model.canSaveSearch)
                .accessibilityIdentifier("browse.sidebar.savedSearches.save")
        }
    }

    private func rootMatched(_ kind: BrowseSidebarSection) -> Bool { filter.matches(BrowseStrings.section(kind)) }

    private var anyMatch: Bool {
        !savedRows.isEmpty || !stateRows.isEmpty || !flagRows.isEmpty || !deckNodes.isEmpty || !notetypeRows.isEmpty
            || !tagNodes.isEmpty
    }

    private func shows<Item>(_ rows: [Item]) -> Bool { !filter.isOn || !rows.isEmpty }

    // Saved Searches: T13's built-ins, then the user's (S3.1's order).
    private enum Saved: Hashable { case builtIn(FilterBuiltIn), user(BrowseModel.SavedSearch) }
    private var savedRows: [Saved] {
        filter.rows(FilterBuiltIn.allCases.map(Saved.builtIn) + model.savedSearches.map(Saved.user), label: { saved in
            switch saved {
            case .builtIn(let search): BrowseStrings.builtIn(search)
            case .user(let search): search.name
            }
        }, rootMatched: rootMatched(.savedSearches))
    }

    private var savedSearches: some View {
        section(.savedSearches, shown: shows(savedRows)) {
            ForEach(savedRows, id: \.self) { saved in
                switch saved {
                case .builtIn(let search):
                    row(.builtIn(search), label: BrowseStrings.builtIn(search), identifier: "saved.\(Self.identifier(search))",
                        icon: .symbol("magnifyingglass"))
                case .user(let search):
                    // V6, T13: its Rename… and Delete are in the list's row menu (`rowMenu`).
                    row(.saved(search.name), label: search.name,
                        identifier: "user.\(model.savedSearches.firstIndex(of: search) ?? 0)", icon: .symbol("magnifyingglass"))
                }
            }
        }
    }

    private static func identifier(_ search: FilterBuiltIn) -> String {
        switch search {
        case .dueToday: "dueToday"
        case .addedInLast7Days: "addedInLast7Days"
        case .leeches: "leeches"
        }
    }

    // Card State: the five states (Leech is the Leeches saved search).
    private var stateRows: [FilterState] {
        filter.rows(BrowseModel.states, label: { BrowseStrings.state($0) }, rootMatched: rootMatched(.cardState))
    }

    private var cardStates: some View {
        section(.cardState, shown: shows(stateRows)) {
            ForEach(stateRows, id: \.self) { state in
                row(.state(state), label: BrowseStrings.state(state), identifier: "state.\(state)", icon: .state(state))
            }
        }
    }

    // Flags: No Flag, then flags 1 to 7 by their shown names, each in its colour (T3).
    private var flagRows: [UInt8] {
        guard model.flagNames.count == 7 else { return [] }
        return filter.rows(Array(UInt8(0)...7), label: flagName, rootMatched: rootMatched(.flags))
    }

    private func flagName(_ flag: UInt8) -> String {
        flag == 0 ? BrowseStrings.text("browse.flag.none") : model.flagNames[Int(flag) - 1]
    }

    private var flags: some View {
        section(.flags, shown: shows(flagRows)) {
            ForEach(flagRows, id: \.self) { flag in
                row(.flag(flag), label: flagName(flag), identifier: "flag.\(flag)", icon: .flag(flag))
            }
        }
    }

    // Decks: the deck tree, with Today's expansion (V6).
    private var deckNodes: [BrowseSidebarFilter.Node<CountedDeckNode>] {
        guard filter.isOn else { return [] }
        return filter.tree(decks, label: Self.deckName, children: \.children, rootMatched: rootMatched(.decks))
    }

    private static func deckName(_ node: CountedDeckNode) -> String {
        node.name.components(separatedBy: "::").last ?? node.name
    }

    private var deckSection: some View {
        section(.decks, shown: !filter.isOn || !deckNodes.isEmpty) {
            if filter.isOn {
                ForEach(deckNodes, id: \.item.id) { node in FilteredDeckBranch(sidebar: self, node: node, ancestors: []) }
            } else {
                ForEach(decks, id: \.id) { node in
                    DeckBranch(sidebar: self, node: node, ancestors: [])
                        .tag(BrowseSidebarTag.deck(node.id))
                }
            }
        }
    }

    // Note Types: by name, A2's order; no children. With more than 12, the 8 with
    // the most rows and the query's own, then Show All (the filter searches them all).
    private var notetypeRows: [NotetypeName] {
        filter.rows(filter.isOn ? model.notetypes : model.shownNotetypes.rows, label: \.name, rootMatched: rootMatched(.notetypes))
    }

    private var notetypes: some View {
        section(.notetypes, shown: shows(notetypeRows)) {
            ForEach(notetypeRows, id: \.id) { notetype in
                // V6, W14: Edit Note Type… is in the list's row menu (`rowMenu`).
                row(.notetype(notetype.id), label: notetype.name, identifier: "notetype.\(notetype.id)", icon: .symbol("doc.text"))
            }
            if !filter.isOn, let all = model.shownNotetypes.showAll {
                Text(verbatim: BrowseStrings.showAll(all))
                    .foregroundStyle(.secondary)
                    .tag(BrowseSidebarTag.showAllNotetypes)
                    .accessibilityIdentifier("browse.sidebar.notetypes.showAll")
            }
        }
    }

    // Tags: the tag tree; with more than 12 top-level tags, the 8 with the most notes, then Show All.
    private var tagNodes: [BrowseSidebarFilter.Node<TagNode>] {
        model.filteredTags(filter, rootMatched: rootMatched(.tags))
    }

    private var tags: some View {
        section(.tags, shown: !filter.isOn || !tagNodes.isEmpty) {
            if filter.isOn {
                ForEach(tagNodes, id: \.item.fullName) { node in FilteredTagBranch(sidebar: self, node: node) }
            } else {
                let shown = model.shownTopLevelTags
                ForEach(shown.tags, id: \.fullName) { node in
                    TagBranch(sidebar: self, node: node)
                        .tag(BrowseSidebarTag.tag(node.fullName))
                }
                if let all = shown.showAll {
                    Text(verbatim: BrowseStrings.showAll(all))
                        .foregroundStyle(.secondary)
                        .tag(BrowseSidebarTag.showAllTags)
                        .accessibilityIdentifier("browse.sidebar.tags.showAll")
                }
            }
        }
    }

    // MARK: Rows

    /// A row's leading symbol: its section's, a card state's or a flag's own.
    fileprivate enum RowIcon {
        case symbol(String), state(FilterState), flag(UInt8)
    }

    /// One value row: its symbol, its label (the match highlighted) and its
    /// count as the row's badge once known. A row the query selects (T9)
    /// reads in the emphasized weight on a tinted background, with a checkmark.
    fileprivate func row(_ tag: BrowseSidebarTag, label: String, identifier: String, icon: RowIcon,
                         help: String? = nil) -> some View {
        let active = model.selectedRows.contains(tag)
        return HStack(spacing: Spacing.xxSmall) {
            Label {
                highlighted(label).fontWeight(active ? .semibold : nil)
            } icon: {
                rowIcon(icon).accessibilityHidden(true)
            }
            .lineLimit(1)
            Spacer(minLength: 0)
            if active {
                Image(systemName: "checkmark")
                    .imageScale(.small)
                    .fontWeight(.semibold)
                    .foregroundStyle(.tint)
                    .accessibilityHidden(true)
            }
        }
        // The whole row takes the click below, not the label alone.
        .contentShape(Rectangle())
        #if os(macOS)
        .simultaneousGesture(TapGesture().onEnded { clicked(tag) })
        #endif
        .help(help.map { Text(verbatim: $0) } ?? Text(verbatim: label))
        .badge(model.counts[tag].map { Text($0, format: .number) })
        .listRowBackground(active ? Radius.rounded(Radius.small).fill(.tint.quaternary).padding(.horizontal, Spacing.xSmall) : nil)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(active ? .isSelected : [])
        .accessibilityValue(model.counts[tag].map { Text(verbatim: BrowseStrings.count($0, mode: model.mode)) } ?? Text(verbatim: ""))
        .accessibilityHint(model.activationRemoves(tag) ? Text("browse.sidebar.row.removeHint", bundle: .module) : Text(verbatim: ""))
        .accessibilityIdentifier("browse.sidebar.\(identifier)")
        .tag(tag)
    }

    @ViewBuilder
    private func rowIcon(_ icon: RowIcon) -> some View {
        switch icon {
        case .symbol(let name):
            Image(systemName: name).foregroundStyle(.secondary)
        case .state(.new):
            Image(systemName: "circle.fill").imageScale(.small).foregroundStyle(.token(.countNew))
        case .state(.learning):
            Image(systemName: "circle.fill").imageScale(.small).foregroundStyle(.token(.countLearn))
        case .state(.review):
            Image(systemName: "circle.fill").imageScale(.small).foregroundStyle(.token(.countDue))
        // The table's state column draws these two the same way (V2.5).
        case .state(.suspended):
            Image(systemName: "pause.circle").foregroundStyle(.secondary)
        case .state:
            Image(systemName: "moon").foregroundStyle(.secondary)
        case .flag(0):
            Image(systemName: "flag.slash").foregroundStyle(.secondary)
        case .flag(let flag):
            Image(systemName: "flag.fill").foregroundStyle(BrowseStrings.flagColor(flag))
        }
    }

    /// S6: a matching row shows its match in bold.
    private func highlighted(_ label: String) -> Text {
        guard let range = filter.match(in: label) else { return Text(verbatim: label) }
        var text = AttributedString(label)
        if let lower = AttributedString.Index(range.lowerBound, within: text),
           let upper = AttributedString.Index(range.upperBound, within: text) {
            text[lower..<upper].inlinePresentationIntent = .stronglyEmphasized
        }
        return Text(text)
    }

    fileprivate func deckRow(_ node: CountedDeckNode, ancestors: [String]) -> some View {
        let name = Self.deckName(node)
        return row(.deck(node.id), label: name, identifier: "deck.\(node.id)", icon: .symbol("rectangle.stack"),
                   help: (ancestors + [name]).joined(separator: " › "))
    }

    fileprivate func tagRow(_ node: TagNode) -> some View {
        row(.tag(node.fullName), label: node.name, identifier: "tag.\(Self.identifier(node.fullName))", icon: .symbol("tag"),
            help: node.fullName.replacingOccurrences(of: "::", with: " › "))
    }

    /// A tag's name as an identifier's last part (letters, digits, `_` and `-`).
    private static func identifier(_ name: String) -> String {
        String(name.unicodeScalars.map { $0.properties.isAlphabetic || ("0"..."9").contains($0) || $0 == "-" ? Character($0) : "_" })
    }

    fileprivate var deckExpansion: (Int64) -> Binding<Bool> { deckExpanded }
    fileprivate var browse: BrowseModel { model }
}

// MARK: Trees

/// A deck and its subdecks, with Today's expansion (V6).
private struct DeckBranch<Header: View>: View {
    let sidebar: BrowseSidebar<Header>
    let node: CountedDeckNode
    let ancestors: [String]

    var body: some View {
        if node.children.isEmpty {
            sidebar.deckRow(node, ancestors: ancestors)
        } else {
            DisclosureGroup(isExpanded: sidebar.deckExpansion(node.id)) {
                ForEach(node.children, id: \.id) { child in
                    // AnyView hides the row's own tag, so the group's would reach it and select it too.
                    AnyView(DeckBranch(sidebar: sidebar, node: child,
                                       ancestors: ancestors + [node.name.components(separatedBy: "::").last ?? node.name]))
                        .tag(BrowseSidebarTag.deck(child.id))
                }
            } label: {
                sidebar.deckRow(node, ancestors: ancestors)
            }
            .tag(BrowseSidebarTag.deck(node.id))
        }
    }
}

/// A deck while the filter is on: expanded exactly when a subdeck matches (S6).
private struct FilteredDeckBranch<Header: View>: View {
    let sidebar: BrowseSidebar<Header>
    let node: BrowseSidebarFilter.Node<CountedDeckNode>
    let ancestors: [String]

    var body: some View {
        let name = node.item.name.components(separatedBy: "::").last ?? node.item.name
        if node.children.isEmpty {
            sidebar.deckRow(node.item, ancestors: ancestors)
        } else {
            DisclosureGroup(isExpanded: .constant(true)) {
                ForEach(node.children, id: \.item.id) { child in
                    AnyView(FilteredDeckBranch(sidebar: sidebar, node: child, ancestors: ancestors + [name]))
                        .tag(BrowseSidebarTag.deck(child.item.id))
                }
            } label: {
                sidebar.deckRow(node.item, ancestors: ancestors)
            }
            .tag(BrowseSidebarTag.deck(node.item.id))
        }
    }
}

/// A tag and the tags below it, with the tag's own expansion.
private struct TagBranch<Header: View>: View {
    let sidebar: BrowseSidebar<Header>
    let node: TagNode

    var body: some View {
        if node.children.isEmpty {
            sidebar.tagRow(node)
        } else {
            DisclosureGroup(isExpanded: Binding(get: { sidebar.browse.tagExpanded(node) },
                                                set: { sidebar.browse.setTagExpanded(node, $0) })) {
                ForEach(node.children, id: \.fullName) { child in
                    AnyView(TagBranch(sidebar: sidebar, node: child))
                        .tag(BrowseSidebarTag.tag(child.fullName))
                }
            } label: {
                sidebar.tagRow(node)
            }
            .tag(BrowseSidebarTag.tag(node.fullName))
        }
    }
}

private struct FilteredTagBranch<Header: View>: View {
    let sidebar: BrowseSidebar<Header>
    let node: BrowseSidebarFilter.Node<TagNode>

    var body: some View {
        if node.children.isEmpty {
            sidebar.tagRow(node.item)
        } else {
            DisclosureGroup(isExpanded: .constant(true)) {
                ForEach(node.children, id: \.item.fullName) { child in
                    AnyView(FilteredTagBranch(sidebar: sidebar, node: child))
                        .tag(BrowseSidebarTag.tag(child.item.fullName))
                }
            } label: {
                sidebar.tagRow(node.item)
            }
            .tag(BrowseSidebarTag.tag(node.item.fullName))
        }
    }
}

/// T13's Rename…: the saved search's new name, Cancel and Rename; a name
/// another saved search has asks whether to replace it (Replace, Cancel).
private struct RenameSavedSearch: ViewModifier {
    let model: BrowseModel
    @State private var name = ""

    func body(content: Content) -> some View {
        let renaming = model.renamingSearch
        content
            .alert(Text("browse.renameSearch.title", bundle: .module), isPresented: Binding(
                get: { model.renamingSearch != nil && model.renamingSearch?.replacing == nil },
                set: { if !$0, model.renamingSearch?.replacing == nil { model.cancelRenaming() } }), presenting: renaming) { renaming in
                TextField(text: $name) { Text("browse.saveSearch.name", bundle: .module) }
                    .onAppear { name = renaming.old }
                    .accessibilityIdentifier("browse.renameSearch.name")
                Button(role: .cancel) { model.cancelRenaming() } label: { Text("browse.saveSearch.cancel", bundle: .module) }
                Button { model.rename(to: name) } label: { Text("browse.renameSearch.rename", bundle: .module) }
                    .keyboardShortcut(.defaultAction)
            }
            .alert(Text(verbatim: BrowseStrings.replaceSavedSearch(renaming?.replacing ?? "")), isPresented: Binding(
                get: { model.renamingSearch?.replacing != nil }, set: { if !$0 { model.keepRenaming() } }),
                   presenting: renaming?.replacing) { replacing in
                Button(role: .cancel) { model.keepRenaming() } label: { Text("browse.saveSearch.cancel", bundle: .module) }
                Button { model.rename(to: replacing) } label: { Text("browse.saveSearch.replace", bundle: .module) }
            }
    }
}
