import AppModel
import CoreBridge
import SwiftUI

/// The column chooser's menu (U272; `browser/workspace` V2.2, V2.3), one
/// list of items that the table header's context menu, View › Columns and
/// the status line's set menu draw.
///
/// - **Standard set**: every column with a check, the list's columns first in
///   their order, then the others by label (`browser/columns` C2's labels in
///   the mode); then **Fields ›**, the field names grouped by note type (a
///   name several note types hold appears under each and adds the same
///   column); then Size Columns to Fit and Reset to Standard Columns.
/// - **A note type's own set** leads with that note type's fields in field
///   order, the first 8, then "<N> More Fields ›"; then **Standard ›** with
///   the columns as above; then Size Columns to Fit and Use Standard Columns.
/// - Checking adds a column at the end, unchecking removes it; the last
///   column cannot be unchecked (R1133).
struct BrowseColumnMenu: Equatable {
    struct Input: Equatable {
        var columns: [String]
        var mode: BrowseMode
        /// The note type whose set is shown, when it is one (V2.2).
        var notetype: Int64?
        var notetypes: [NotetypeName]
        var fields: [Int64: [String]]
        var canReset: Bool
        /// Size Columns to Fit needs the table (the Mac header).
        var canSizeToFit: Bool
    }

    indirect enum Item: Equatable {
        case column(key: String, title: String, checked: Bool, enabled: Bool)
        case submenu(title: String, identifier: String, items: [Item])
        /// A note type's name over its fields.
        case section(title: String, items: [Item])
        case separator
        case sizeToFit
        case reset(own: Bool, enabled: Bool)
    }

    let items: [Item]

    /// Own sets show this many of their fields before "<N> More Fields ›".
    static let leadingFields = 8

    init(_ input: Input, locale: Locale = .current) {
        let shown = Set(input.columns)
        let column = { (key: String) -> Item in
            let checked = shown.contains(key)
            return .column(key: key, title: BrowseStrings.column(key, mode: input.mode, locale: locale), checked: checked,
                           enabled: !(checked && input.columns.count == 1))
        }
        // The list's catalog columns first, in their order, then the others by label.
        let listed = input.columns.filter { BrowseResults.catalog.contains($0) }
        let others = BrowseResults.catalog.filter { !shown.contains($0) }
            .sorted { BrowseStrings.column($0, mode: input.mode, locale: locale)
                .localizedStandardCompare(BrowseStrings.column($1, mode: input.mode, locale: locale)) == .orderedAscending }
        let standard = (listed + others).map(column)
        var items: [Item] = []
        if let id = input.notetype {
            let fields = (input.fields[id] ?? []).map { column("fld:" + $0) }
            items += fields.prefix(Self.leadingFields)
            if fields.count > Self.leadingFields {
                let more = fields.count - Self.leadingFields
                items.append(.submenu(title: BrowseColumnText.moreFields(more, locale: locale), identifier: "moreFields",
                                      items: Array(fields.dropFirst(Self.leadingFields))))
            }
            if !items.isEmpty { items.append(.separator) }
            items.append(.submenu(title: BrowseStrings.text("browse.columns.standardMenu", locale: locale), identifier: "standard",
                                  items: standard))
        } else {
            items += standard
            let groups = input.notetypes.compactMap { type -> Item? in
                guard let fields = input.fields[type.id], !fields.isEmpty else { return nil }
                return .section(title: type.name, items: fields.map { column("fld:" + $0) })
            }
            if !groups.isEmpty {
                items.append(.separator)
                items.append(.submenu(title: BrowseStrings.text("browse.columns.fields", locale: locale), identifier: "fields",
                                      items: groups))
            }
        }
        items.append(.separator)
        if input.canSizeToFit { items.append(.sizeToFit) }
        items.append(.reset(own: input.notetype != nil, enabled: input.canReset))
        self.items = items
    }

    /// The menu for the set the model shows.
    @MainActor static func of(_ model: BrowseModel, canSizeToFit: Bool, locale: Locale = .current) -> BrowseColumnMenu {
        let results = model.results
        var notetype: Int64?
        if let set = results.columnSet, case .notetype(let id) = set.set, set.own { notetype = id }
        return BrowseColumnMenu(Input(columns: results.shownColumns, mode: results.columnsMode, notetype: notetype,
                                      notetypes: model.notetypes, fields: model.notetypeFields,
                                      canReset: results.canResetColumns, canSizeToFit: canSizeToFit), locale: locale)
    }
}

enum BrowseColumnText {
    /// "<N> More Fields" (V2.3, V16).
    static func moreFields(_ count: Int, locale: Locale = .current) -> String {
        BrowseInspectorText.counted("browse.columns.moreFields", UInt64(count), locale: locale)
    }

    /// "Reset to Standard Columns" in the standard set; V2.2's "Use Standard
    /// Columns" in a note type's own set.
    static func reset(own: Bool, locale: Locale = .current) -> String {
        BrowseStrings.text(own ? "browse.columns.useStandard" : "browse.columns.reset", locale: locale)
    }
}

/// The chooser's items as SwiftUI menu content: View › Columns (the menu
/// bar) and the status line's set menu. Checked columns are toggles, which
/// menus draw with their checkmark.
public struct BrowseColumnsMenuContent: View {
    let model: BrowseModel
    @Environment(\.locale) private var locale

    public init(model: BrowseModel) {
        self.model = model
    }

    public var body: some View {
        let menu = BrowseColumnMenu.of(model, canSizeToFit: false, locale: locale)
        ForEach(Array(menu.items.enumerated()), id: \.offset) { _, item in
            BrowseColumnMenuItemView(model: model, item: item)
        }
    }
}

private struct BrowseColumnMenuItemView: View {
    let model: BrowseModel
    let item: BrowseColumnMenu.Item

    var body: some View {
        switch item {
        case .column(let key, let title, let checked, let enabled):
            Toggle(isOn: Binding(get: { checked }, set: { _ in model.results.toggleColumn(key) })) { Text(verbatim: title) }
                .disabled(!enabled)
                .accessibilityIdentifier("browse.columns.column.\(key)")
        case .submenu(let title, let identifier, let items):
            Menu {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in BrowseColumnMenuItemView(model: model, item: item) }
            } label: { Text(verbatim: title) }
                .accessibilityIdentifier("browse.columns.\(identifier)")
        case .section(let title, let items):
            Section {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in BrowseColumnMenuItemView(model: model, item: item) }
            } header: { Text(verbatim: title) }
        case .separator:
            Divider()
        case .sizeToFit:
            EmptyView()
        case .reset(let own, let enabled):
            Button { model.results.resetColumns() } label: { Text(verbatim: BrowseColumnText.reset(own: own)) }
                .disabled(!enabled)
                .accessibilityIdentifier("browse.columns.reset")
        }
    }
}
