import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The Cards tab (`notetypes/workspace` W5 to W8; N22.3): the card types
/// list over the Fields list, then the template editor with Front | Back |
/// Styling and the Insert menu in its bar and the live issues under it.
/// The live preview beside them is U283's. Nothing here writes before Save.
struct NoteTypeCardsTab: View {
    @Bindable var model: NoteTypesModel
    let id: Int64

    var body: some View {
        #if os(macOS)
        // N22.3: the lists belong to the editor's side of the window, on the
        // same background as its bar and issues, not a second sidebar; only
        // the app's navigation sidebar uses sidebar material.
        HSplitView {
            NoteTypeCardsListColumn(model: model, id: id)
                .frame(minWidth: Self.columnMinimum, idealWidth: Self.columnIdeal, maxWidth: Self.columnMaximum)
            NoteTypeTemplateEditorColumn(model: model, id: id)
                .frame(minWidth: ContentWidth.primaryAction, maxWidth: .infinity, maxHeight: .infinity)
        }
        #else
        // iPad (N22.7): the Mac's columns with touch sizes. Hiding the lists
        // while a card type is open waits for the preview that shares the
        // space with the code (U283).
        VStack(spacing: .zero) {
            // The navigation bar's edge, across both columns.
            Divider()
            HStack(spacing: .zero) {
                NoteTypeCardsListColumn(model: model, id: id)
                    .frame(width: Self.columnIdeal)
                // The columns run to the bottom edge, under the home indicator.
                Divider()
                    .ignoresSafeArea(.container, edges: .bottom)
                NoteTypeTemplateEditorColumn(model: model, id: id)
            }
        }
        #endif
    }

    private static let columnMinimum: CGFloat = 200
    #if os(macOS)
    private static let columnIdeal: CGFloat = 240
    #else
    private static let columnIdeal: CGFloat = 280
    #endif
    private static let columnMaximum: CGFloat = 320
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

/// The card types column: its bar, then one list with the card types (+ and
/// − under them) and the Fields section (W5).
struct NoteTypeCardsListColumn: View {
    @Bindable var model: NoteTypesModel
    let id: Int64

    var body: some View {
        VStack(spacing: .zero) {
            SectionTitle(title: "noteTypes.cards.cardTypes")
                .modifier(CardsColumnBar())
                .accessibilityIdentifier("noteTypes.cards.cardTypesHeader")
            Divider()
            NoteTypeCardTypesList(model: model, id: id)
        }
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
    @Environment(\.locale) private var locale
    /// W5's keys act on the list only while it has focus: in the editor
    /// ⌘⌫ and ⌥⌘↑ stay the text view's.
    @FocusState private var listFocused: Bool

    private var draft: NoteTypeDraft? { model.draft(id) }
    private var editable: Bool { model.editable(id) }
    private var selection: Binding<Int?> {
        Binding(get: { model.shownCard(id) }, set: { model.selectedCard[id] = $0 })
    }

    var body: some View {
        // One list, two sections: the card types with their + and − under
        // them, then the fields. Plain rows on the editor side's background,
        // with the system selection.
        List(selection: selection) {
            Section {
                ForEach(model.cardTypeLines(id)) { line in
                    CardTypeRowView(model: model, id: id, line: line)
                        .tag(line.id)
                        .contextMenu { menu(line) }
                }
                .onMove { offsets, destination in model.moveCardTypes(id, fromOffsets: offsets, toOffset: destination) }
                .disabled(!editable)
            } footer: {
                bottomBar
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
        .onDeleteCommand { deleteSelected() }
        #else
        .listStyle(.plain)
        #endif
        .scrollContentBackground(.hidden)
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

    @ViewBuilder private func menu(_ line: CardTypeLine) -> some View {
        if line.deleted {
            Button { model.restoreCardType(id, row: line.id) } label: { Text("noteTypes.cards.restore", bundle: .module) }
        } else {
            Button { startRename(line.id) } label: { Text("noteTypes.cards.rename", bundle: .module) }
            Button { model.selectedCard[id] = line.id; model.addCardType(id) } label: { Text("noteTypes.cards.duplicate", bundle: .module) }
                .keyboardShortcut("d", modifiers: .command)
                .disabled(draft?.addCardTypeBlock != nil)
            Divider()
            Button { model.moveCardType(id, row: line.id, by: -1) } label: { Text("noteTypes.cards.moveUp", bundle: .module) }
                .keyboardShortcut(.upArrow, modifiers: [.option, .command])
                .disabled(draft?.canMoveCardType(line.id, by: -1) != true)
            Button { model.moveCardType(id, row: line.id, by: 1) } label: { Text("noteTypes.cards.moveDown", bundle: .module) }
                .keyboardShortcut(.downArrow, modifiers: [.option, .command])
                .disabled(draft?.canMoveCardType(line.id, by: 1) != true)
            Divider()
            Button(role: .destructive) { model.deleteCardType(id, row: line.id) } label: { Text("noteTypes.cards.delete", bundle: .module) }
                .keyboardShortcut(.delete, modifiers: .command)
                .disabled(draft?.cardDeleteBlock(line.id) != nil)
        }
    }

    private var bottomBar: some View {
        HStack(spacing: Spacing.xxSmall) {
            Button { model.addCardType(id) } label: {
                Label { Text("noteTypes.cards.add", bundle: .module) } icon: { Image(systemName: "plus") }
                    .labelStyle(.iconOnly)
                    .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                    .contentShape(Rectangle())
            }
            .disabled(draft?.addCardTypeBlock != nil)
            .help(draft?.addCardTypeBlock.map { NoteTypesStrings.cardTypeBlock($0, locale: locale) } ?? "")
            .accessibilityIdentifier("noteTypes.cards.add")
            Button { deleteSelected() } label: {
                Label { Text("noteTypes.cards.deleteSelected", bundle: .module) } icon: { Image(systemName: "minus") }
                    .labelStyle(.iconOnly)
                    .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                    .contentShape(Rectangle())
            }
            .disabled(deleteBlocked)
            .help(deleteHelp)
            .accessibilityIdentifier("noteTypes.cards.delete")
            Spacer()
        }
        .buttonStyle(.borderless)
        .disabled(!editable)
    }

    private var shownRow: NoteTypeCardTypeRow? { model.shownCard(id).flatMap { draft?.cardRow($0) } }
    private var deleteBlocked: Bool {
        guard let row = shownRow else { return true }
        return row.deleted || draft?.cardDeleteBlock(row.id) != nil
    }
    private var deleteHelp: String {
        guard let row = shownRow, let block = draft?.cardDeleteBlock(row.id) else { return "" }
        return NoteTypesStrings.cardTypeBlock(block, locale: locale)
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
/// field, its cards, and a dot when an issue names it.
private struct CardTypeRowView: View {
    let model: NoteTypesModel
    let id: Int64
    let line: CardTypeLine
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: Spacing.xSmall) {
            if model.renamingCard == line.id {
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
        .accessibilityElement(children: model.renamingCard == line.id ? .contain : .combine)
        .accessibilityValue(line.deleted ? Text("noteTypes.cards.deleted", bundle: .module) : Text(verbatim: ""))
        .accessibilityIdentifier("noteTypes.cards.row.\(line.id)")
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
    @Environment(\.locale) private var locale

    private var card: Int? { model.shownCard(id) }
    private var part: NoteTypeTemplatePart { model.cardPart }
    private var draft: NoteTypeDraft? { model.draft(id) }

    var body: some View {
        VStack(spacing: .zero) {
            bar
            Divider()
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
            if part == .styling {
                Text("noteTypes.editor.shared", bundle: .module)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .accessibilityIdentifier("noteTypes.cards.shared")
            }
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
