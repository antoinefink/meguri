import AppModel
import CoreBridge
import SwiftUI

/// Browse's search field (U271; N21.2, `search/filter-tokens` T8, T10,
/// T12, T13): the system's search field in the window toolbar with its
/// standard prompt, the query's tokens as the system's search tokens, each
/// with its menu, and the system's suggestions list in sections.
struct BrowseSearchField: ViewModifier {
    let model: BrowseModel
    /// The toolbar's. On the Mac the principal placement would put the field
    /// after the title and Cards / Notes, wide, but shows no search
    /// suggestions there (VM, 2026-11-23), and T12 needs them: macOS then
    /// draws the field at the toolbar's trailing end.
    static let placement: SearchFieldPlacement = .toolbar
    @FocusState private var focused: Bool
    @Environment(\.locale) private var locale

    func body(content: Content) -> some View {
        content
            .searchable(text: Binding(get: { model.query.text }, set: { model.editText($0) }),
                        editableTokens: Binding(get: { model.fieldTokens }, set: { model.setFieldTokens($0) }),
                        placement: Self.placement) { $token in
                BrowseTokenView(model: model, token: token)
            }
            .searchSuggestions { BrowseSuggestionList(model: model) }
            .searchFocused($focused)
            .onSubmit(of: .search) { model.commitText() }
            .onChange(of: focused) { _, focused in model.setFieldFocused(focused) }
            .onChange(of: model.focusRequests) { focused = true }
            .sheet(isPresented: Binding(get: { model.savingSearch != nil }, set: { if !$0 { model.cancelSavingSearch() } })) {
                SaveSearchSheet(model: model)
            }
            .alert(Text(verbatim: model.editingContains?.field ?? ""), isPresented: Binding(
                get: { model.editingContains != nil }, set: { if !$0 { model.dismissContains() } }),
                   presenting: model.editingContains) { editing in
                ContainsPrompt(model: model, editing: editing)
            }
    }
}

// MARK: Tokens

/// One token (N21.2): the kind label in the secondary style, "is not" after
/// it when negated, then the values; an advanced token grey with its text.
/// A click opens its menu (T10).
private struct BrowseTokenView: View {
    let model: BrowseModel
    let token: BrowseFieldToken

    var body: some View {
        if case .token(let index, let filter) = token {
            Menu {
                BrowseTokenMenu(model: model, index: index, token: filter)
            } label: {
                label(filter)
            }
            .accessibilityLabel(Text(verbatim: BrowseStrings.accessibility(filter, flagNames: model.flagNames)))
            .accessibilityIdentifier("browse.search.token.\(index)")
        }
    }

    @ViewBuilder private func label(_ filter: FilterToken) -> some View {
        switch filter {
        case .advanced(let text):
            Text(verbatim: text).foregroundStyle(.secondary)
        case .value(let values, _):
            if filter.kind != nil {
                let flag = values.lazy.compactMap { value -> UInt8? in if case .flag(let n) = value { n } else { nil } }.first
                // The kind (and "is not") is the token's menu part, drawn by the
                // system from the menu's first checked item; the label is the values.
                Label {
                    Text(verbatim: BrowseStrings.values(values, flagNames: model.flagNames))
                } icon: {
                    if let flag {
                        Image(systemName: "circle.fill").foregroundStyle(BrowseStrings.flagColor(flag))
                    }
                }
                .help(Text(verbatim: BrowseStrings.accessibility(filter, flagNames: model.flagNames)))
            }
        }
    }
}

/// T10's menu: Is and Is Not, the token's values checked, up to 8 others,
/// Add Another <kind>…, then Remove Filter. A Field token lists its tests;
/// an advanced token has Edit and Remove Filter only.
private struct BrowseTokenMenu: View {
    let model: BrowseModel
    let index: Int
    let token: FilterToken

    var body: some View {
        switch token {
        case .advanced:
            Button { model.editAdvanced(index) } label: { Text("browse.token.edit", bundle: .module) }
            Divider()
            remove
        case .value(let values, let negated):
            // Is and Is Not, as the kind with and without "is not": the system
            // token draws its menu part from the first checked item, so the
            // token reads "DECK ⌄ Spanish", "TAG IS NOT ⌄ alpha or beta".
            if let kind = token.kind {
                Toggle(isOn: Binding(get: { !negated }, set: { if $0 { model.setNegated(index, false) } })) {
                    Text(verbatim: BrowseStrings.kindLabel(kind, negated: false))
                }
                Toggle(isOn: Binding(get: { negated }, set: { if $0 { model.setNegated(index, true) } })) {
                    Text(verbatim: BrowseStrings.kindLabel(kind, negated: true))
                }
                .disabled(!token.canNegate)
            }
            Divider()
            if case .field(let field)? = token.kind {
                fieldTests(field, values)
            } else if let kind = token.kind {
                ForEach(values, id: \.self) { value in toggle(value, on: true) }
                ForEach(model.offeredValues(index), id: \.self) { value in toggle(value, on: false) }
                if kind.section != nil {
                    Button { model.addAnother(to: index) } label: { Text(verbatim: BrowseStrings.addAnother(kind)) }
                }
            }
            Divider()
            remove
        }
    }

    private func toggle(_ value: FilterValue, on: Bool) -> some View {
        Toggle(isOn: Binding(get: { on }, set: { _ in model.toggle(value, in: index) })) {
            Text(verbatim: BrowseStrings.value(value, flagNames: model.flagNames))
        }
    }

    @ViewBuilder private func fieldTests(_ field: String, _ values: [FilterValue]) -> some View {
        Button { model.editContains(in: index) } label: { Text("browse.token.containsMenu", bundle: .module) }
        Toggle(isOn: Binding(get: { values.contains(.field(field, .empty)) }, set: { _ in model.toggleFieldTest(.empty, in: index) })) {
            Text("browse.token.isEmptyMenu", bundle: .module)
        }
        Toggle(isOn: Binding(get: { values.contains(.field(field, .hasContent)) },
                             set: { _ in model.toggleFieldTest(.hasContent, in: index) })) {
            Text("browse.token.hasContentMenu", bundle: .module)
        }
    }

    private var remove: some View {
        Button { model.removeToken(index) } label: { Text("browse.token.remove", bundle: .module) }
    }
}

// MARK: Suggestions

/// T12's list: the rows in the core's order, under their section labels in
/// the order their first row appears; each with its symbol, label, detail
/// and count (N21.2). Accepting a row hands it to the model as a token.
private struct BrowseSuggestionList: View {
    let model: BrowseModel

    var body: some View {
        if let field = model.suggestedField {
            fieldTests(field)
        } else {
            ForEach(groups, id: \.id) { group in
                if let section = group.section {
                    Section {
                        rows(group.rows)
                    } header: {
                        Text(verbatim: BrowseStrings.section(section))
                            .accessibilityIdentifier("browse.search.section.\(section)")
                    }
                } else {
                    rows(group.rows)
                }
            }
        }
    }

    private struct Group {
        let id: Int
        let section: FilterSection?
        var rows: [FilterSuggestion]
    }

    /// Consecutive rows of one section, so each label shows once, where its
    /// first row is.
    private var groups: [Group] {
        var groups: [Group] = []
        for suggestion in model.suggestions {
            if let last = groups.last, last.section == suggestion.section {
                groups[groups.count - 1].rows.append(suggestion)
            } else {
                groups.append(Group(id: groups.count, section: suggestion.section, rows: [suggestion]))
            }
        }
        return groups
    }

    private func rows(_ rows: [FilterSuggestion]) -> some View {
        ForEach(rows, id: \.self) { suggestion in
            BrowseSuggestionRow(model: model, suggestion: suggestion)
                .searchCompletion(BrowseFieldToken.suggestion(suggestion))
        }
    }

    /// A Field suggestion's three tests (T12.1).
    @ViewBuilder private func fieldTests(_ field: SuggestedField) -> some View {
        Section {
            row(field.containsText.map { BrowseStrings.containsWord($0) } ?? BrowseStrings.text("browse.token.containsMenu"),
                symbol: "text.magnifyingglass", identifier: "contains")
                .searchCompletion(BrowseFieldToken.fieldTest(.contains(field.containsText ?? "")))
            row(BrowseStrings.text("browse.token.isEmptyMenu"), symbol: "square.dashed", identifier: "isEmpty")
                .searchCompletion(BrowseFieldToken.fieldTest(.empty))
            row(BrowseStrings.text("browse.token.hasContentMenu"), symbol: "square.fill.text.grid.1x2", identifier: "hasContent")
                .searchCompletion(BrowseFieldToken.fieldTest(.hasContent))
        } header: {
            Text(verbatim: field.name)
                .accessibilityIdentifier("browse.search.section.fieldTests")
        }
    }

    private func row(_ label: String, symbol: String, identifier: String) -> some View {
        Label { Text(verbatim: label) } icon: { Image(systemName: symbol) }
            .accessibilityIdentifier("browse.search.fieldTest.\(identifier)")
    }
}

/// One suggestion row: symbol, label, secondary detail and trailing count.
private struct BrowseSuggestionRow: View {
    let model: BrowseModel
    let suggestion: FilterSuggestion

    var body: some View {
        HStack {
            Label {
                if let detail {
                    Text({ () -> AttributedString in
                        var tail = AttributedString(" " + detail)
                        tail.foregroundColor = .secondary
                        return AttributedString(label) + tail
                    }())
                } else {
                    Text(verbatim: label)
                }
            } icon: {
                icon
            }
            Spacer()
            if let count {
                Text(count, format: .number).monospacedDigit().foregroundStyle(.secondary)
            }
        }
        // One element per row, named area.screen.element.id (R1964).
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("browse.search.suggestion.\(Self.compact(identifier))")
    }

    private var label: String {
        switch suggestion.row {
        case .contains(let word): BrowseStrings.containsWord(word)
        case .builtIn(let search): BrowseStrings.builtIn(search)
        case .savedSearch(let name, _): name
        case .filterBy(let section): BrowseStrings.filterBy(section)
        case .value(let value), .cardType(let value, _): BrowseStrings.value(value, flagNames: model.flagNames)
        case .field(let name, _): name
        case .showAll(let section): BrowseStrings.showAllSection(section)
        case .saveSearch: BrowseStrings.text("browse.search.saveSearch")
        case .syntax(let candidate): candidate.label
        }
    }

    private var detail: String? {
        switch suggestion.row {
        case .value(let value): BrowseStrings.path(value).flatMap { $0.contains("›") ? $0 : nil }
        case .field(_, let notetypes), .cardType(_, let notetypes):
            BrowseStrings.notetypesDetail(notetypes, names: model.notetypes)
        default: nil
        }
    }

    private var count: UInt64? {
        switch suggestion.row {
        case .value(let value), .cardType(let value, _): model.count(of: value)
        case .builtIn(let search): model.count(of: search)
        case .savedSearch(let name, _): model.count(ofSaved: name)
        default: nil
        }
    }

    @ViewBuilder private var icon: some View {
        switch suggestion.row {
        case .value(.flag(let flag)):
            Image(systemName: "flag.fill").foregroundStyle(BrowseStrings.flagColor(flag))
        default:
            Image(systemName: symbol)
        }
    }

    private var symbol: String {
        switch suggestion.row {
        case .contains: "text.magnifyingglass"
        case .builtIn, .savedSearch: "magnifyingglass"
        case .filterBy(let section), .showAll(let section): Self.symbol(section)
        case .value(let value): Self.symbol(value.kind.section ?? .filterBy)
        case .cardType: Self.symbol(.cardTypes)
        case .field: Self.symbol(.fields)
        case .saveSearch: "plus.magnifyingglass"
        case .syntax: "chevron.left.forwardslash.chevron.right"
        }
    }

    /// Each kind's symbol.
    private static func symbol(_ section: FilterSection) -> String {
        switch section {
        case .savedSearches: "magnifyingglass"
        case .cardState: "circle.lefthalf.filled"
        case .flag: "flag"
        case .due: "calendar"
        case .added: "plus.circle"
        case .edited: "pencil"
        case .studied: "checkmark.circle"
        case .decks: "rectangle.stack"
        case .tags: "tag"
        case .notetypes: "doc.text"
        case .cardTypes: "rectangle.on.rectangle"
        case .fields: "character.textbox"
        case .filterBy: "line.3.horizontal.decrease"
        }
    }

    private var identifier: String {
        switch suggestion.row {
        case .contains: "contains"
        case .builtIn(let search): "builtIn.\(search)"
        case .savedSearch(let name, _): "saved.\(name.hashValue & 0xFFFF)"
        case .filterBy(let section): "filterBy.\(section)"
        case .value(let value): "value.\(Self.identifier(value))"
        case .cardType(let value, _): "cardType.\(Self.identifier(value))"
        case .field(let name, _): "field.\(name.hashValue & 0xFFFF)"
        case .showAll(let section): "showAll.\(section)"
        case .saveSearch: "saveSearch"
        case .syntax(let candidate): "syntax.\(candidate.label.hashValue & 0xFFFF)"
        }
    }

    /// The identifier's last part: letters, digits, `_` and `-` only.
    private static func compact(_ parts: String) -> String {
        String(parts.unicodeScalars.map { ("a"..."z").contains($0) || ("A"..."Z").contains($0) || ("0"..."9").contains($0)
            || $0 == "_" ? Character($0) : "-" })
    }

    private static func identifier(_ value: FilterValue) -> String {
        switch value {
        case .deck(let id, _, _): "deck.\(id)"
        case .tag(let name): "tag.\(String(name.unicodeScalars.map { $0.properties.isAlphabetic || ("0"..."9").contains($0) ? Character($0) : "_" }))"
        case .notetype(let id, _): "notetype.\(id)"
        case .state(let state): "state.\(state)"
        case .flag(let flag): "flag.\(flag)"
        case .noFlag: "flag.0"
        case .anyFlag: "flag.any"
        case .currentDeck: "currentDeck"
        case .untagged: "untagged"
        case .due(let due): "due.\(due)"
        case .added(let period): "added.\(period)"
        case .edited(let period): "edited.\(period)"
        case .studied(let period): "studied.\(period)"
        default: "other"
        }
    }
}

// MARK: Save Search and Contains…

/// T13's sheet: a name, Cancel and Save; a name already saved asks whether
/// to replace it (Replace, Cancel). The command sheets' frame (U276): the
/// title leads above the field on the Mac, and Save stays disabled while the
/// name is empty or white space.
private struct SaveSearchSheet: View {
    let model: BrowseModel
    @State private var name = ""
    @FocusState private var focused: Bool

    private var canSave: Bool { !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && model.savingSearch?.saving != true }

    var body: some View {
        BrowseSheetFrame(title: Text("browse.saveSearch.title", bundle: .module), confirm: Text("browse.saveSearch.save", bundle: .module),
                         canConfirm: canSave, canCancel: true, identifier: "browse.saveSearch",
                         onConfirm: save, onCancel: { model.cancelSavingSearch() }) {
            form
        }
    }

    private func save() {
        guard canSave else { return }
        model.saveSearch(named: name)
    }

    private var form: some View {
        Form {
            TextField(text: $name) { Text("browse.saveSearch.name", bundle: .module) }
                .focused($focused)
                .onSubmit(save)
                .accessibilityIdentifier("browse.saveSearch.name")
            if model.savingSearch?.failed == true {
                Text("browse.saveSearch.failed", bundle: .module)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("browse.saveSearch.failed")
            }
        }
        #if os(macOS)
        .formStyle(.columns)
        #else
        .formStyle(.grouped)
        #endif
        .alert(Text(verbatim: BrowseStrings.replaceSavedSearch(model.savingSearch?.replacing ?? "")), isPresented: Binding(
            get: { model.savingSearch?.replacing != nil }, set: { if !$0 { model.keepNaming() } }),
               presenting: model.savingSearch?.replacing) { replacing in
            Button(role: .cancel) { model.keepNaming() } label: { Text("browse.saveSearch.cancel", bundle: .module) }
            Button { model.replaceSavedSearch(named: replacing) } label: {
                Text("browse.saveSearch.replace", bundle: .module)
            }
        }
        .onAppear {
            name = model.savingSearch?.name ?? ""
            focused = true
        }
    }
}

/// Contains…'s text (T10): the field's name as the title, the text, OK and Cancel.
private struct ContainsPrompt: View {
    let model: BrowseModel
    let editing: EditingContains
    @State private var text = ""

    var body: some View {
        TextField(text: $text) { Text("browse.token.containsMenu", bundle: .module) }
            .onAppear { text = editing.text }
            .accessibilityIdentifier("browse.search.contains.text")
        Button(role: .cancel) { model.dismissContains() } label: { Text("browse.saveSearch.cancel", bundle: .module) }
        Button { model.finishContains(editing, text) } label: { Text("browse.search.contains.ok", bundle: .module) }
    }
}

extension View {
    /// Browse's search field in the window toolbar (N21.2).
    func browseSearchField(_ model: BrowseModel) -> some View {
        modifier(BrowseSearchField(model: model))
    }
}
