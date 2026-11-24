import AppModel
import CoreBridge
import Foundation
import Testing
@testable import Browse

/// The column chooser's menu (U272; `browser/workspace` V2.3): every column
/// with its check, the list's first in its order then the others by label,
/// in the mode's labels; Fields › grouped by note type; a note type's own set
/// leading with its first 8 fields, then "<N> More Fields ›" and Standard ›;
/// the last column cannot be unchecked; the reset item names the set's kind.
@MainActor @Suite struct BrowseColumnMenuTests {
    private let english = Locale(identifier: "en_US")
    private let notetypes = [NotetypeName(id: 1, name: "Basic"), NotetypeName(id: 2, name: "Vocabulary")]

    private func menu(_ columns: [String], mode: BrowseMode = .cards, notetype: Int64? = nil,
                      fields: [Int64: [String]] = [:], canReset: Bool = true) -> [BrowseColumnMenu.Item] {
        BrowseColumnMenu(.init(columns: columns, mode: mode, notetype: notetype, notetypes: notetypes, fields: fields,
                               canReset: canReset, canSizeToFit: true), locale: english).items
    }

    private func titles(_ items: [BrowseColumnMenu.Item]) -> [String] {
        items.compactMap { if case .column(_, let title, let checked, _) = $0 { (checked ? "✓" : "") + title } else { nil } }
    }

    @Test func theStandardSetListsItsColumnsFirstThenTheOthersByLabel() {
        let items = menu(["noteFld", "deck", "cardDue"])
        #expect(titles(items) == ["✓Sort Field", "✓Deck", "✓Due", "Answer", "Card Modified", "Card Type", "Created", "Difficulty",
                                  "Ease", "Interval", "Lapses", "Note Modified", "Note Type", "Position", "Question",
                                  "Retrievability", "Reviews", "Stability", "Tags"])
        #expect(items.suffix(2) == [.sizeToFit, .reset(own: false, enabled: true)])
        // Notes mode's labels (C2).
        #expect(titles(menu(["noteFld", "template"], mode: .notes)).prefix(2) == ["✓Sort Field", "✓Cards"])
        #expect(titles(menu(["noteFld"], mode: .notes)).contains("Avg. Ease"))
    }

    @Test func fieldsAreGroupedByNoteTypeAndANameSeveralHoldAddsOneColumn() throws {
        let items = menu(["noteFld", "fld:Front"], fields: [1: ["Front", "Back"], 2: ["Word", "Front"]])
        let fields = try #require(items.first { if case .submenu(_, "fields", _) = $0 { true } else { false } })
        guard case .submenu(let title, _, let groups) = fields else { return }
        #expect(title == "Fields")
        #expect(groups == [
            .section(title: "Basic", items: [.column(key: "fld:Front", title: "Front", checked: true, enabled: true),
                                             .column(key: "fld:Back", title: "Back", checked: false, enabled: true)]),
            .section(title: "Vocabulary", items: [.column(key: "fld:Word", title: "Word", checked: false, enabled: true),
                                                  .column(key: "fld:Front", title: "Front", checked: true, enabled: true)]),
        ])
        // No fields read yet: no Fields submenu.
        #expect(!menu(["noteFld"]).contains { if case .submenu = $0 { true } else { false } })
    }

    @Test func anOwnSetLeadsWithItsFirstEightFieldsThenMoreFieldsAndStandard() throws {
        let names = (1...11).map { "F\($0)" }
        let items = menu(["fld:F1", "noteFld"], notetype: 2, fields: [2: names])
        #expect(titles(Array(items.prefix(8))) == ["✓F1"] + (2...8).map { "F\($0)" })
        guard case .submenu(let more, "moreFields", let rest) = items[8] else { Issue.record("no More Fields"); return }
        #expect(more == "3 More Fields" && titles(rest) == ["F9", "F10", "F11"])
        #expect(items[9] == .separator)
        guard case .submenu(let standard, "standard", let columns) = items[10] else { Issue.record("no Standard"); return }
        #expect(standard == "Standard" && titles(columns).first == "✓Sort Field" && columns.count == 19)
        #expect(items.last == .reset(own: true, enabled: true))
        #expect(BrowseColumnText.reset(own: true, locale: english) == "Use Standard Columns")
        #expect(BrowseColumnText.reset(own: false, locale: english) == "Reset to Standard Columns")
        #expect(BrowseColumnText.moreFields(1, locale: english) == "1 More Field")
    }

    @Test func theLastColumnCannotBeUnchecked() {
        let items = menu(["deck"], canReset: false)
        #expect(items.first == .column(key: "deck", title: "Deck", checked: true, enabled: false))
        #expect(items.last == .reset(own: false, enabled: false))
        #expect(BrowseStrings.text("browse.columns.sizeToFit", locale: english) == "Size Columns to Fit")
    }
}
