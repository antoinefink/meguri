import AppModel
@testable import Browse
import CoreBridge
import Testing

@MainActor struct BrowseSidebarDeleteSelectionTests {
    private let browse: Set<BrowseSidebarTag> = [.section(.browse)]
    private let tree = [
        TagNode(name: "Ink", fullName: "Ink", collapsed: false, implicit: true, children: [
            TagNode(name: "pen", fullName: "Ink::pen", collapsed: false, implicit: false, children: []),
        ]),
        TagNode(name: "Other", fullName: "Other", collapsed: false, implicit: false, children: []),
    ]

    @Test func mouseAndKeyboardSelectionsRetainLiteralNamesInDisplayOrder() {
        var selection = BrowseSidebarDeleteSelection()
        selection.changed(from: browse, to: [.tag("Other")], byKey: false, commandClick: false)
        // The List has restored Browse before the next command-click.
        selection.changed(from: browse, to: [.section(.browse), .tag("Ink::pen")], byKey: false, commandClick: true)
        #expect(BrowseSidebarDeleteSelection.tagNames(in: selection.rows, tree: tree) == ["Ink::pen", "Other"])
        selection.changed(from: browse, to: [.section(.browse), .tag("Other")], byKey: false, commandClick: true)
        #expect(selection.rows == [.tag("Ink::pen")])
        // A keyboard range replaces the retained click, including implicit parents.
        selection.changed(from: browse, to: [.tag("Ink"), .tag("Other")], byKey: true, commandClick: false)
        #expect(BrowseSidebarDeleteSelection.tagNames(in: selection.rows, tree: tree) == ["Ink", "Other"])
        selection.clicked(.tag("Ink::pen"), commandClick: false)
        #expect(selection.rows == [.tag("Ink::pen")])
    }

    @Test func groupsOtherKindsEmptyAndMissingRowsNeverDeleteAPartialSelection() {
        let refused: [Set<BrowseSidebarTag>] = [
            [], browse, [.builtIn(.dueToday)], [.showAllTags], [.notetype(1)],
            [.tag("Ink"), .deck(1)], [.tag("Ink"), .section(.browse)],
            [.tag("Ink"), .state(.new)], [.tag("Ink"), .saved("Saved")],
            [.tag("Ink"), .tag("missing")], [.tag("")],
        ]
        for rows in refused {
            var selection = BrowseSidebarDeleteSelection()
            selection.changed(from: browse, to: [.tag("Ink")], byKey: false, commandClick: false)
            selection.changed(from: browse, to: rows, byKey: true, commandClick: false)
            #expect(BrowseSidebarDeleteSelection.tagNames(in: selection.rows, tree: tree).isEmpty)
        }
        var selection = BrowseSidebarDeleteSelection()
        selection.clicked(.tag("Ink"), commandClick: false)
        selection.changed(from: browse, to: [.section(.browse), .state(.new)], byKey: false, commandClick: true)
        #expect(BrowseSidebarDeleteSelection.tagNames(in: selection.rows, tree: tree).isEmpty)
        let untagged = TagNode(name: "Untagged", fullName: "", collapsed: false, implicit: false, children: [])
        #expect(BrowseSidebarDeleteSelection.tagNames(in: [.tag("")], tree: [untagged]).isEmpty)
    }
}
