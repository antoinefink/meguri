import AppModel
import CoreBridge
import SwiftUI

/// Browse's search field (U271; N21.2, `search/filter-tokens` T8, T10,
/// T12, T13): the system's search field in the window toolbar with its
/// standard prompt, the query's tokens as the system's search tokens, each
/// with its menu, and the system's suggestions list in sections.
struct BrowseSearchField: ViewModifier {
    let model: BrowseModel
    @FocusState private var focused: Bool
    @Environment(\.locale) private var locale

    func body(content: Content) -> some View {
        content
            .searchable(text: Binding(get: { model.query.text }, set: { model.editText($0) }),
                        editableTokens: Binding(get: { model.fieldTokens }, set: { model.setFieldTokens($0) }),
                        placement: .toolbar) { $token in
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
                get: { model.editingContains != nil }, set: { if !$0 { model.finishContains(nil) } })) {
                ContainsPrompt(model: model)
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

    /// The first part in the secondary style, a space, then the second.
    static func styled(_ secondary: String, then primary: String) -> AttributedString {
        var first = AttributedString(secondary)
        first.foregroundColor = .secondary
        return first + AttributedString(" " + primary)
    }

    @ViewBuilder private func label(_ filter: FilterToken) -> some View {
        switch filter {
        case .advanced(let text):
            Text(verbatim: text).foregroundStyle(.secondary)
        case .value(let values, let negated):
            if let kind = filter.kind {
                let flag = values.lazy.compactMap { value -> UInt8? in if case .flag(let n) = value { n } else { nil } }.first
                Label {
                    Text(Self.styled(BrowseStrings.kindLabel(kind, negated: negated),
                                     then: BrowseStrings.values(values, flagNames: model.flagNames)))
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
            Picker(selection: Binding(get: { negated }, set: { model.setNegated(index, $0) })) {
                Text("browse.token.is", bundle: .module).tag(false)
                Text("browse.token.isNotMenu", bundle: .module).tag(true)
                    .disabled(!token.canNegate)
            } label: { EmptyView() }
            .pickerStyle(.inline)
            .labelsHidden()
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
            .keyboardShortcut(.delete, modifiers: [])
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
                symbol: "text.magnifyingglass")
                .searchCompletion(BrowseFieldToken.fieldTest(.contains(field.containsText ?? "")))
            row(BrowseStrings.text("browse.token.isEmptyMenu"), symbol: "square.dashed")
                .searchCompletion(BrowseFieldToken.fieldTest(.empty))
            row(BrowseStrings.text("browse.token.hasContentMenu"), symbol: "square.fill.text.grid.1x2")
                .searchCompletion(BrowseFieldToken.fieldTest(.hasContent))
        } header: {
            Text(verbatim: field.name)
        }
    }

    private func row(_ label: String, symbol: String) -> some View {
        Label { Text(verbatim: label) } icon: { Image(systemName: symbol) }
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
        .accessibilityIdentifier("browse.search.suggestion.\(identifier)")
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
/// to replace it (Replace, Cancel).
private struct SaveSearchSheet: View {
    let model: BrowseModel
    @State private var name = ""
    @FocusState private var focused: Bool

    var body: some View {
        Form {
            TextField(text: $name) { Text("browse.saveSearch.name", bundle: .module) }
                .focused($focused)
                .onSubmit { model.saveSearch(named: name) }
                .accessibilityIdentifier("browse.saveSearch.name")
            if model.savingSearch?.failed == true {
                Text("browse.saveSearch.failed", bundle: .module)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("browse.saveSearch.failed")
            }
        }
        .formStyle(.grouped)
        .navigationTitle(Text("browse.saveSearch.title", bundle: .module))
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button { model.cancelSavingSearch() } label: { Text("browse.saveSearch.cancel", bundle: .module) }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button { model.saveSearch(named: name) } label: { Text("browse.saveSearch.save", bundle: .module) }
                    .accessibilityIdentifier("browse.saveSearch.save")
            }
        }
        .alert(Text(verbatim: BrowseStrings.replaceSavedSearch(model.savingSearch?.replacing ?? "")), isPresented: Binding(
            get: { model.savingSearch?.replacing != nil }, set: { if !$0 { model.keepNaming() } })) {
            Button(role: .cancel) { model.keepNaming() } label: { Text("browse.saveSearch.cancel", bundle: .module) }
            Button { model.saveSearch(named: model.savingSearch?.replacing ?? name) } label: {
                Text("browse.saveSearch.replace", bundle: .module)
            }
        }
        .frame(minWidth: 320)
        .onAppear {
            name = model.savingSearch?.name ?? ""
            focused = true
        }
    }
}

/// Contains…'s text (T10): the field's name as the title, the text, OK and Cancel.
private struct ContainsPrompt: View {
    let model: BrowseModel
    @State private var text = ""

    var body: some View {
        TextField(text: $text) { Text("browse.token.containsMenu", bundle: .module) }
            .onAppear { text = model.editingContains?.text ?? "" }
            .accessibilityIdentifier("browse.search.contains.text")
        Button(role: .cancel) { model.finishContains(nil) } label: { Text("browse.saveSearch.cancel", bundle: .module) }
        Button { model.finishContains(text) } label: { Text("browse.search.contains.ok", bundle: .module) }
    }
}

extension View {
    /// Browse's search field in the window toolbar (N21.2).
    func browseSearchField(_ model: BrowseModel) -> some View {
        modifier(BrowseSearchField(model: model))
    }
}
