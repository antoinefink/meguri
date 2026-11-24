import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The Cards tab (`notetypes/workspace` W5 to W9; N22.3, N22.7): the card
/// types list over the Fields list, then the template editor with Front |
/// Back | Styling and the Insert menu in its bar and the live issues under
/// it, then the live preview. Nothing here writes before Save.
struct NoteTypeCardsTab: View {
    @Bindable var model: NoteTypesModel
    let id: Int64

    var body: some View {
        #if os(macOS)
        // N22.3: the lists belong to the editor's side of the window, on the
        // same background as its bar and issues, not a second sidebar; only
        // the app's navigation sidebar uses sidebar material. The preview is
        // the third column, resizable; it hides by itself while the tab is too
        // narrow for a usable editor beside it, and its toolbar toggle is kept
        // on the device.
        GeometryReader { geometry in
            let shows = NoteTypePreviewLayout.macShows(requested: model.preview.requested, width: geometry.size.width)
            HSplitView {
                NoteTypeCardsListColumn(model: model, id: id)
                    .frame(minWidth: Self.columnMinimum, idealWidth: Self.columnIdeal, maxWidth: Self.columnMaximum)
                NoteTypeTemplateEditorColumn(model: model, id: id)
                    // The split's own minimum stays the editor's narrowest; the preview
                    // asks for a wider one (NoteTypePreviewLayout) before it shows.
                    .frame(minWidth: ContentWidth.primaryAction, maxWidth: .infinity, maxHeight: .infinity)
                if shows {
                    NoteTypePreviewPane(model: model, id: id)
                        .frame(minWidth: NoteTypePreviewLayout.previewMinimum, idealWidth: NoteTypePreviewLayout.previewIdeal,
                               maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .onChange(of: geometry.size.width, initial: true) { _, width in
                model.preview.fits = NoteTypePreviewLayout.macShows(requested: true, width: width)
            }
        }
        #else
        // iPad (N22.7): with the preview shown, the lists step aside and the
        // code and the preview take half each, the card type a menu in the
        // editor's bar; without it, the Mac's columns with touch sizes.
        VStack(spacing: .zero) {
            // The navigation bar's edge, across both columns.
            Divider()
            HStack(spacing: .zero) {
                if model.preview.requested {
                    NoteTypeTemplateEditorColumn(model: model, id: id, cardTypeMenu: true)
                        .frame(maxWidth: .infinity)
                    Divider()
                        .ignoresSafeArea(.container, edges: .bottom)
                    NoteTypePreviewPane(model: model, id: id)
                        .frame(maxWidth: .infinity)
                        // The card runs to the bottom edge, under the home indicator.
                        .ignoresSafeArea(.container, edges: .bottom)
                } else {
                    NoteTypeCardsListColumn(model: model, id: id)
                        .frame(width: Self.columnIdeal)
                    // The columns run to the bottom edge, under the home indicator.
                    Divider()
                        .ignoresSafeArea(.container, edges: .bottom)
                    NoteTypeTemplateEditorColumn(model: model, id: id)
                }
            }
        }
        #endif
    }

    private static let columnMinimum: CGFloat = NoteTypePreviewLayout.listMinimum
    #if os(macOS)
    private static let columnIdeal: CGFloat = NoteTypePreviewLayout.listWidth
    #else
    private static let columnIdeal: CGFloat = 280
    #endif
    #if os(macOS)
    /// Narrow enough that the editor and the preview share the rest.
    private static let columnMaximum: CGFloat = 260
    #else
    private static let columnMaximum: CGFloat = 320
    #endif
}

/// The bar at the top of each column of the Cards tab: the card types
/// column's title and the editor's Front | Back | Styling share one height,
/// so their hairlines make one line across the tab.
struct CardsColumnBar: ViewModifier {
    #if os(macOS)
    @ScaledMetric(relativeTo: .body) private var height: CGFloat = ControlMetrics.minimumTarget + 2 * Spacing.xSmall
    #else
    @ScaledMetric(relativeTo: .body) private var height: CGFloat = ControlMetrics.minimumTarget + Spacing.xSmall
    #endif

    func body(content: Content) -> some View {
        content
            .padding(.horizontal, Spacing.small)
            .frame(maxWidth: .infinity, minHeight: height, alignment: .leading)
    }
}

/// The card types column: its bar, with + at its trailing end, then one
/// list with the card types and the Fields section (W5).
struct NoteTypeCardsListColumn: View {
    @Bindable var model: NoteTypesModel
    let id: Int64

    var body: some View {
        VStack(spacing: .zero) {
            HStack(spacing: Spacing.xSmall) {
                SectionTitle(title: "noteTypes.cards.cardTypes")
                    .accessibilityIdentifier("noteTypes.cards.cardTypesHeader")
                Spacer(minLength: Spacing.xxSmall)
                CardTypeAddButton(model: model, id: id)
            }
            .modifier(CardsColumnBar())
            Divider()
            NoteTypeCardTypesList(model: model, id: id)
        }
    }
}

/// + in the column's bar: duplicates the shown card type (W5). Its help
/// names that card type, or says why there is nothing to add.
private struct CardTypeAddButton: View {
    let model: NoteTypesModel
    let id: Int64
    @Environment(\.locale) private var locale

    private var draft: NoteTypeDraft? { model.draft(id) }

    var body: some View {
        Button { model.addCardType(id) } label: {
            Label { Text("noteTypes.cards.add", bundle: .module) } icon: { Image(systemName: "plus") }
                .labelStyle(.iconOnly)
                .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .disabled(!model.editable(id) || draft?.addCardTypeBlock != nil)
        .help(help)
        .accessibilityIdentifier("noteTypes.cards.add")
    }

    private var help: String {
        if let block = draft?.addCardTypeBlock { return NoteTypesStrings.cardTypeBlock(block, locale: locale) }
        guard let row = model.shownCard(id).flatMap({ draft?.cardRow($0) }), !row.deleted else { return "" }
        return NoteTypesStrings.duplicateNamed(row.name, locale: locale)
    }
}

// MARK: The card types list (W5)

/// One row of the card types list as it is drawn.
struct CardTypeLine: Identifiable, Hashable {
    let id: Int
    let name: String
    let deleted: Bool
    let cards: UInt64?
    let severity: NoteTypeLiveIssue.Severity?
}

extension NoteTypesModel {
    /// The card types list's rows: name, cards, and the worst issue naming it.
    func cardTypeLines(_ id: Int64) -> [CardTypeLine] {
        guard let draft = draft(id) else { return [] }
        let issues = liveIssues(id)
        return draft.cardRows.map { row in
            let named = issues.filter { $0.card == row.id }
            let severity: NoteTypeLiveIssue.Severity? = named.contains { $0.severity == .red } ? .red : (named.isEmpty ? nil : .yellow)
            return CardTypeLine(id: row.id, name: row.name, deleted: row.deleted, cards: cardCount(id, row: row), severity: severity)
        }
    }
}

struct NoteTypeCardTypesList: View {
    @Bindable var model: NoteTypesModel
    let id: Int64
    /// W5's keys act on the list only while it has focus: in the editor
    /// ⌘⌫ and ⌥⌘↑ stay the text view's.
    @FocusState private var listFocused: Bool

    private var draft: NoteTypeDraft? { model.draft(id) }
    private var editable: Bool { model.editable(id) }
    private var selection: Binding<Int?> {
        Binding(get: { model.shownCard(id) }, set: { model.selectedCard[id] = $0 })
    }

    var body: some View {
        // One list, two sections: the card types, then the fields. Plain
        // rows on the editor side's background, with the system selection.
        // A row's More button and its context menu hold the same commands.
        List(selection: selection) {
            Section {
                ForEach(model.cardTypeLines(id)) { line in
                    CardTypeRowView(model: model, id: id, line: line) { menu(line, shortcuts: false) }
                        .tag(line.id)
                        .contextMenu { menu(line, shortcuts: true) }
                }
                .onMove { offsets, destination in model.moveCardTypes(id, fromOffsets: offsets, toOffset: destination) }
                .disabled(!editable)
            }
            Section {
                NoteTypeCardFieldRows(model: model, id: id)
            } header: {
                SectionTitle(title: "noteTypes.cards.fields")
                    .accessibilityIdentifier("noteTypes.cards.fieldsHeader")
            }
        }
        #if os(macOS)
        .listStyle(.inset)
        // The window's background, as the editor's bar and issues have.
        .scrollContentBackground(.hidden)
        .onDeleteCommand { deleteSelected() }
        #else
        // The plain list's own background: the page's, as the editor's.
        .listStyle(.plain)
        #endif
        .focused($listFocused)
        // Return renames the selected card type in place (W5).
        .onKeyPress(.return) {
            guard listFocused, model.renamingCard == nil, let row = shownRow, !row.deleted else { return .ignored }
            startRename(row.id)
            return .handled
        }
        .accessibilityIdentifier("noteTypes.cards.list")
        .background { keys }
    }

    /// Duplicate ⌘D, Move Up ⌥⌘↑, Move Down ⌥⌘↓, Delete ⌘⌫ (W5).
    private var keys: some View {
        Group {
            Button("") { model.addCardType(id) }
                .keyboardShortcut("d", modifiers: .command)
                .disabled(!listFocused || draft?.addCardTypeBlock != nil)
            Button("") { if let row = shownRow { model.moveCardType(id, row: row.id, by: -1) } }
                .keyboardShortcut(.upArrow, modifiers: [.option, .command])
                .disabled(!listFocused)
            Button("") { if let row = shownRow { model.moveCardType(id, row: row.id, by: 1) } }
                .keyboardShortcut(.downArrow, modifiers: [.option, .command])
                .disabled(!listFocused)
            Button("") { deleteSelected() }
                .keyboardShortcut(.delete, modifiers: .command)
                .disabled(!listFocused)
        }
        .opacity(0)
        .accessibilityHidden(true)
    }

    /// A card type's commands: the context menu's, with W5's keys beside
    /// them, and the row's More menu's, without. The More menus stay in the
    /// tree, one a row, so keys on them would act on a row nobody chose; the
    /// list's own keys (`keys`) act on the selected one.
    @ViewBuilder private func menu(_ line: CardTypeLine, shortcuts: Bool) -> some View {
        if line.deleted {
            Button { model.restoreCardType(id, row: line.id) } label: { Text("noteTypes.cards.restore", bundle: .module) }
        } else {
            Button { startRename(line.id) } label: { Text("noteTypes.cards.rename", bundle: .module) }
            Button { model.selectedCard[id] = line.id; model.addCardType(id) } label: { Text("noteTypes.cards.duplicate", bundle: .module) }
                .keyboardShortcut(shortcuts ? KeyboardShortcut("d", modifiers: .command) : nil)
                .disabled(draft?.addCardTypeBlock != nil)
            Divider()
            Button { model.moveCardType(id, row: line.id, by: -1) } label: { Text("noteTypes.cards.moveUp", bundle: .module) }
                .keyboardShortcut(shortcuts ? KeyboardShortcut(.upArrow, modifiers: [.option, .command]) : nil)
                .disabled(draft?.canMoveCardType(line.id, by: -1) != true)
            Button { model.moveCardType(id, row: line.id, by: 1) } label: { Text("noteTypes.cards.moveDown", bundle: .module) }
                .keyboardShortcut(shortcuts ? KeyboardShortcut(.downArrow, modifiers: [.option, .command]) : nil)
                .disabled(draft?.canMoveCardType(line.id, by: 1) != true)
            Divider()
            Button(role: .destructive) { model.deleteCardType(id, row: line.id) } label: { Text("noteTypes.cards.delete", bundle: .module) }
                .keyboardShortcut(shortcuts ? KeyboardShortcut(.delete, modifiers: .command) : nil)
                .disabled(draft?.cardDeleteBlock(line.id) != nil)
        }
    }

    private var shownRow: NoteTypeCardTypeRow? { model.shownCard(id).flatMap { draft?.cardRow($0) } }
    private var deleteBlocked: Bool {
        guard let row = shownRow else { return true }
        return row.deleted || draft?.cardDeleteBlock(row.id) != nil
    }

    private func deleteSelected() {
        guard let row = shownRow, !deleteBlocked else { return }
        model.deleteCardType(id, row: row.id)
    }

    private func startRename(_ row: Int) {
        guard editable else { return }
        model.selectedCard[id] = row
        model.renamingCard = row
    }
}

/// A card type's row: its name (crossed out when deleted) or the rename
/// field, a More button with the row's commands, a dot when an issue names
/// it, and its cards.
private struct CardTypeRowView<Actions: View>: View {
    let model: NoteTypesModel
    let id: Int64
    let line: CardTypeLine
    /// The row's commands, as the More menu's and VoiceOver's actions.
    @ViewBuilder let actions: () -> Actions
    @State private var text = ""
    @FocusState private var focused: Bool
    #if os(macOS)
    @State private var hovered = false
    #endif

    private var renaming: Bool { model.renamingCard == line.id }

    /// The pointer shows More on the row under it and on the selected one;
    /// touch has no pointer, so every row shows it.
    private var showsMore: Bool {
        guard !renaming else { return false }
        #if os(macOS)
        return hovered || model.shownCard(id) == line.id
        #else
        return true
        #endif
    }

    var body: some View {
        HStack(spacing: Spacing.xSmall) {
            if renaming {
                TextField(text: $text) { Text("noteTypes.cards.name", bundle: .module) }
                    .labelsHidden()
                    .focused($focused)
                    .autocorrectionDisabled()
                    .onSubmit { model.renameCardType(id, row: line.id, typed: text) }
                    #if os(macOS)
                    .onExitCommand { model.renamingCard = nil }
                    #endif
                    .onChange(of: focused) { _, now in
                        if !now, model.renamingCard == line.id { model.renameCardType(id, row: line.id, typed: text) }
                    }
                    .onAppear { text = line.name; focused = true }
                    // Save commits the typed name first, as it does a field's.
                    .onChange(of: text) { _, typed in
                        let model = model, id = id, row = line.id
                        guard typed != line.name else { model.pendingEdits["cardRename"] = nil; return }
                        model.pendingEdits["cardRename"] = {
                            model.renameCardType(id, row: row, typed: typed)
                            return true
                        }
                    }
                    .onDisappear { model.pendingEdits["cardRename"] = nil }
                    .accessibilityIdentifier("noteTypes.cards.renameField")
            } else {
                Text(verbatim: line.name)
                    .strikethrough(line.deleted)
                    .foregroundStyle(line.deleted ? .secondary : .primary)
                    .lineLimit(1)
                    #if os(macOS)
                    .onTapGesture(count: 2) { if !line.deleted { model.renamingCard = line.id } }
                    #endif
            }
            Spacer(minLength: Spacing.xxSmall)
            more
            if let severity = line.severity, !line.deleted {
                Image(systemName: "circle.fill")
                    .imageScale(.small)
                    .foregroundStyle(severity == .red ? Color.red : Color.yellow)
                    .accessibilityLabel(Text("noteTypes.cards.issueDot", bundle: .module))
            }
            if let cards = line.cards {
                Text(cards, format: .number)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .fixedSize()
            }
        }
        #if os(macOS)
        .onHover { hovered = $0 }
        #endif
        .accessibilityElement(children: renaming ? .contain : .combine)
        .accessibilityValue(line.deleted ? Text("noteTypes.cards.deleted", bundle: .module) : Text(verbatim: ""))
        // The More menu's commands, as the row's own actions.
        .accessibilityActions { actions() }
        .accessibilityIdentifier("noteTypes.cards.row.\(line.id)")
    }

    /// It keeps its place while hidden, so the cards never move under the
    /// pointer. Out of the accessibility tree: the row carries its commands
    /// as actions, and a button inside a combined row would become the
    /// row's own press.
    private var more: some View {
        Menu { actions() } label: {
            Label { Text("noteTypes.cards.more", bundle: .module) } icon: { Image(systemName: "ellipsis") }
                .labelStyle(.iconOnly)
        }
        #if os(macOS)
        .menuStyle(.borderlessButton)
        #else
        .menuStyle(.button)
        .buttonStyle(.borderless)
        #endif
        .menuIndicator(.hidden)
        .fixedSize()
        .foregroundStyle(.secondary)
        .opacity(showsMore ? 1 : 0)
        .allowsHitTesting(showsMore)
        .accessibilityHidden(true)
    }
}

/// The title of a section of the card types column: the column's bar and
/// the Fields section header read alike.
private struct SectionTitle: View {
    let title: LocalizedStringKey

    var body: some View {
        Text(title, bundle: .module)
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(.secondary)
            .accessibilityAddTraits(.isHeader)
    }
}

// MARK: The Fields section (W5)

/// Every draft field; the ones the shown card type does not use are grey.
/// Double click inserts `{{<name>}}` at the insertion point; a drag drops it.
struct NoteTypeCardFieldRows: View {
    let model: NoteTypesModel
    let id: Int64

    private var used: Set<Int>? {
        guard let draft = model.draft(id), let card = model.shownCard(id),
              let position = draft.keptCardRows.firstIndex(where: { $0.id == card }), let usedOn = model.usedOn(id) else { return nil }
        return Set(usedOn.filter { $0.value.contains(position) }.map(\.key))
    }

    var body: some View {
        // Their own identities: the card types above share the list, and a field's
        // row id must never be taken for a card type's.
        ForEach(model.draft(id)?.keptRows ?? [], id: \.fieldListID) { row in
            let unused = used.map { !$0.contains(row.id) } ?? false
            Text(verbatim: row.name)
                .foregroundStyle(unused ? .secondary : .primary)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { insert(row.name) }
                .draggable(reference(row.name))
                .accessibilityAddTraits(.isButton)
                .accessibilityAction { insert(row.name) }
                // Fields are never the list's selection: the card types are.
                .selectionDisabled()
                .accessibilityIdentifier("noteTypes.cards.field.\(row.id)")
        }
    }

    /// The reference a drag drops, in the shown part's delimiters (A14).
    private func reference(_ name: String) -> String {
        let text = model.shownCard(id).flatMap { model.draft(id)?.text($0, model.cardPart == .styling ? .front : model.cardPart) } ?? ""
        return NoteTypeTemplateEditing.insert(.field(name), in: text, selection: 0..<0).text
    }

    private func insert(_ name: String) {
        guard let card = model.shownCard(id), model.cardPart != .styling else { return }
        model.request(id, card: card, part: model.cardPart, .insert(.field(name)))
    }
}

extension NoteTypeFieldRow {
    /// A field's identity in the card types column's list.
    nonisolated fileprivate var fieldListID: String { "field.\(id)" }
}

// MARK: The editor column (W6, W7)

struct NoteTypeTemplateEditorColumn: View {
    @Bindable var model: NoteTypesModel
    let id: Int64
    /// iPad with the preview shown: the lists step aside and the card type is a menu here (N22.7).
    var cardTypeMenu = false
    @Environment(\.locale) private var locale

    private var card: Int? { model.shownCard(id) }
    private var part: NoteTypeTemplatePart { model.cardPart }
    private var draft: NoteTypeDraft? { model.draft(id) }

    var body: some View {
        VStack(spacing: .zero) {
            bar
            Divider()
            // W10, N22.4: the Card Text row above the styling, when it has one plain `.card` rule.
            if part == .styling, let card, let css = draft?.css {
                NoteTypeCardTextRow(model: model, id: id, card: card, css: css)
            }
            editor
            Divider()
            NoteTypeIssuesList(model: model, id: id)
        }
        .alert(Text("noteTypes.editor.flipRefused.title", bundle: .module), isPresented: $model.flipRefused) {
            Button(role: .cancel) { model.flipRefused = false } label: { Text("noteTypes.failed.ok", bundle: .module) }
        } message: {
            Text("noteTypes.editor.flipRefused.message", bundle: .module)
        }
    }

    private var fieldNames: [String] { draft?.keptRows.map(\.name) ?? [] }

    private var bar: some View {
        HStack(spacing: Spacing.small) {
            if cardTypeMenu { cardTypePicker }
            Picker(selection: $model.cardPart) {
                Text("noteTypes.editor.front", bundle: .module).tag(NoteTypeTemplatePart.front)
                    .accessibilityIdentifier("noteTypes.cards.part.front")
                Text("noteTypes.editor.back", bundle: .module).tag(NoteTypeTemplatePart.back)
                    .accessibilityIdentifier("noteTypes.cards.part.back")
                Text("noteTypes.editor.styling", bundle: .module).tag(NoteTypeTemplatePart.styling)
                    .accessibilityIdentifier("noteTypes.cards.part.styling")
            } label: {
                Text("noteTypes.editor.part", bundle: .module)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .accessibilityIdentifier("noteTypes.cards.part")
            #if os(macOS)
            .help(partHelp)
            .accessibilityHint(partHelp)
            #endif
            #if !os(macOS)
            if part == .styling {
                Text("noteTypes.editor.shared", bundle: .module)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .accessibilityIdentifier("noteTypes.cards.shared")
            }
            #endif
            Spacer(minLength: Spacing.xSmall)
            insertMenu
        }
        .padding(.vertical, Spacing.xxSmall)
        .modifier(CardsColumnBar())
        .background {
            // ⌘1, ⌘2, ⌘3 choose the part (R929).
            Group {
                Button("") { model.cardPart = .front }.keyboardShortcut("1", modifiers: .command)
                Button("") { model.cardPart = .back }.keyboardShortcut("2", modifiers: .command)
                Button("") { model.cardPart = .styling }.keyboardShortcut("3", modifiers: .command)
            }
            .opacity(0)
            .accessibilityHidden(true)
        }
    }

    #if os(macOS)
    private var partHelp: Text {
        part == .styling ? Text("noteTypes.editor.shared", bundle: .module) : Text(verbatim: "")
    }
    #endif

    /// N22.7: the card type as a menu in the bar while the lists step aside.
    private var cardTypePicker: some View {
        Menu {
            Picker(selection: Binding(get: { model.shownCard(id) }, set: { model.selectedCard[id] = $0 })) {
                ForEach(draft?.keptCardRows ?? [], id: \.id) { row in
                    Text(verbatim: row.name).tag(Optional(row.id))
                }
            } label: {
                Text("noteTypes.cards.cardType", bundle: .module)
            }
            .pickerStyle(.inline)
        } label: {
            Text(verbatim: card.flatMap { draft?.cardRow($0)?.name } ?? "")
                .lineLimit(1)
                .truncationMode(.tail)
        }
        .menuStyle(.button)
        .buttonStyle(.borderless)
        // The name gives way first: the parts and Insert keep their size.
        .layoutPriority(-1)
        .accessibilityLabel(Text("noteTypes.cards.cardType", bundle: .module))
        .accessibilityIdentifier("noteTypes.cards.cardTypeMenu")
    }

    private var insertEntries: [InsertMenuEntry] {
        guard let card, let text = draft?.text(card, part) else { return [] }
        return InsertMenuEntry.menu(part: part, text: text, fields: fieldNames, locale: locale)
    }

    private var insertMenu: some View {
        Menu {
            InsertMenuContent(entries: insertEntries) { item in
                if let card { model.request(id, card: card, part: part, .insert(item)) }
            }
        } label: {
            Text("noteTypes.insert", bundle: .module)
        }
        #if os(iOS)
        // One button in the tree, as the Mac's pull-down is.
        .menuStyle(.button)
        .buttonStyle(.borderless)
        #endif
        .fixedSize()
        .disabled(!model.editable(id) || card.flatMap { draft?.cardRow($0)?.deleted } == true)
        .accessibilityIdentifier("noteTypes.cards.insert")
    }

    @ViewBuilder private var editor: some View {
        if let card, let text = draft?.text(card, part) {
            let deleted = draft?.cardRow(card)?.deleted == true
            TemplateCodeEditor(
                key: TemplateEditorKey(notetype: id, card: card, part: part), text: text,
                editable: model.editable(id) && !deleted, fields: fieldNames, request: model.editorRequest, insertMenu: insertEntries,
                accessibilityLabel: NoteTypesStrings.part(part, locale: locale),
                onChange: { model.setText(id, card: card, part: part, $0) },
                onConsume: { model.consumeEditorRequest($0) },
                onFlip: draft?.isCloze == true ? nil : { model.flipped(id, card: card) },
                onReplaceFront: { model.replaceText(id, card: card, part: .front, expected: $0, with: $1) },
                syntax: { text, styling in await model.syntax(text, styling: styling) })
        } else {
            Color.clear
        }
    }
}

/// The Insert menu's items as SwiftUI menu content.
struct InsertMenuContent: View {
    let entries: [InsertMenuEntry]
    let choose: (NoteTypeTemplateEditing.Item) -> Void

    var body: some View {
        ForEach(entries, id: \.self) { entry in
            switch entry {
            case .item(let title, let item, let enabled):
                Button(title) { choose(item) }.disabled(!enabled)
            case .submenu(let title, let children):
                Menu(title) { InsertMenuContent(entries: children, choose: choose) }
            }
        }
    }
}

/// W7: the live issues under the editor, each with its fix; the bottom line
/// says how many red issues hold Save.
struct NoteTypeIssuesList: View {
    let model: NoteTypesModel
    let id: Int64
    @Environment(\.locale) private var locale

    var body: some View {
        let issues = model.liveIssues(id)
        VStack(alignment: .leading, spacing: .zero) {
            if issues.isEmpty {
                // Until the reads answer, the row keeps its place without
                // claiming there is nothing to fix.
                let known = model.issuesKnown(id)
                Label { Text("noteTypes.issues.none", bundle: .module) } icon: { Image(systemName: "checkmark.circle") }
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, Spacing.small)
                    .padding(.vertical, Spacing.xSmall)
                    .opacity(known ? 1 : 0)
                    .accessibilityHidden(!known)
                    .accessibilityIdentifier("noteTypes.cards.issues.none")
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                        ForEach(Array(issues.enumerated()), id: \.element.id) { index, issue in
                            IssueRow(model: model, id: id, issue: issue, index: index)
                        }
                    }
                    .padding(.horizontal, Spacing.small)
                    .padding(.vertical, Spacing.xSmall)
                }
                .frame(maxHeight: Self.maximumHeight)
                .fixedSize(horizontal: false, vertical: true)
                if model.issuesBlockSave(id) {
                    Divider()
                    Text(verbatim: NoteTypesStrings.fixToSave(model.redIssueCount(id), locale: locale))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, Spacing.small)
                        .padding(.vertical, Spacing.xSmall)
                        .accessibilityIdentifier("noteTypes.cards.issues.fixToSave")
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.cards.issues")
    }

    private static let maximumHeight: CGFloat = 180
}

private struct IssueRow: View {
    let model: NoteTypesModel
    let id: Int64
    let issue: NoteTypeLiveIssue
    let index: Int
    @Environment(\.locale) private var locale

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.xSmall) {
            Image(systemName: "circle.fill")
                .imageScale(.small)
                .foregroundStyle(issue.severity == .red ? Color.red : Color.yellow)
                .accessibilityHidden(true)
            if let fix = issue.fix, isOffered(fix) {
                // The fix sits after the message while both fit on one line;
                // otherwise it takes its own line under the message, so it
                // never narrows the text into a thin column.
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
                        message
                        fixButton(fix)
                    }
                    VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                        message
                        fixButton(fix)
                    }
                }
            } else {
                message
            }
        }
    }

    private var message: some View {
        let words = NoteTypesStrings.issue(issue, locale: locale)
        return Button { model.showIssue(id, issue) } label: {
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(verbatim: words.message.isEmpty ? words.location : words.location + " " + words.message)
                    .fixedSize(horizontal: false, vertical: true)
                if let reason = words.reason {
                    Text(verbatim: reason).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("noteTypes.cards.issue.\(index)")
    }

    private func fixButton(_ fix: NoteTypeLiveIssue.Fix) -> some View {
        Button { model.applyFix(id, issue) } label: { Text(verbatim: NoteTypesStrings.fix(fix, locale: locale)) }
            .fixedSize()
            .disabled(!model.editable(id) && !isShowNotes(fix))
            .accessibilityIdentifier("noteTypes.cards.fix.\(index)")
    }

    /// Show Notes waits for Browse to take note ids (U287).
    private func isOffered(_ fix: NoteTypeLiveIssue.Fix) -> Bool { !isShowNotes(fix) || model.showNotes != nil }
    private func isShowNotes(_ fix: NoteTypeLiveIssue.Fix) -> Bool { if case .showNotes = fix { true } else { false } }
}
