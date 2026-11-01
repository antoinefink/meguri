#if os(macOS)
import AppKit
import AppModel
import Settings
import SwiftUI
import Testing

@MainActor @Suite(.serialized) struct MacSettingsSearchTests {
    @Test func nativeSidebarSearchIdentifiersSurviveRemountWithText() throws {
        let collection = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation())
        collection.syncPresentation.settingsSearch = "iCloud"
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: 700),
            styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        for _ in 0..<2 {
            let host = NSHostingView(rootView: MacSettingsView(collection: collection, present: { _ in }))
            window.contentView = host
            window.orderFront(nil)
            window.layoutIfNeeded()
            host.layoutSubtreeIfNeeded()
            let fields = searchFields(in: host)
            try #require(fields.count == 1)
            let field = try #require(fields.first)
            #expect((field as any NSAccessibilityProtocol).accessibilityIdentifier() == "settings.root.search")
            let cell = try #require(field.cell as? NSSearchFieldCell)
            let searchLabel = try #require(cell.searchButtonCell?.accessibilityLabel())
            try #require(!searchLabel.isEmpty)
            var visited: Set<ObjectIdentifier> = []
            let buttons = try accessibilityButtons(in: field, visited: &visited)
            let matches = buttons.filter { $0.accessibilityLabel() == searchLabel }
            try recordSearchAccessibilityDiagnostic(window: window, field: field, cell: cell,
                expectedLabel: searchLabel, matchCount: matches.count)
            try #require(matches.count == 1)
            let search = try #require(matches.first)
            #expect(search.accessibilityIdentifier() == "settings.root.searchButton")
            let identified = buttons.filter { $0.accessibilityIdentifier() == "settings.root.searchButton" }
            try #require(identified.count == 1)
            #expect(identified[0] as AnyObject === search as AnyObject)
            #expect(field.stringValue == "iCloud")
            // A remount must identify the new native elements, without relying
            // on a retained field, timer or global window lookup.
            window.contentView = nil
        }
    }

    private func searchFields(in view: NSView) -> [NSSearchField] {
        if let field = view as? NSSearchField { return [field] }
        return view.subviews.flatMap { searchFields(in: $0) }
    }

    // Diagnostic only: never emit UI strings or
    // runtime class names outside this exact public-AppKit allowlist.
    private func recordSearchAccessibilityDiagnostic(window: NSWindow, field: NSSearchField,
        cell: NSSearchFieldCell, expectedLabel: String, matchCount: Int) throws {
        print("settingsSearchAX windowContentAttached=\(window.contentView != nil)"
            + " windowVisible=\(window.isVisible) windowMiniaturized=\(window.isMiniaturized)"
            + " fieldAttached=\(field.window === window) fieldHidden=\(field.isHidden)"
            + " fieldHiddenBySelfOrAncestor=\(field.isHiddenOrHasHiddenAncestor)"
            + " fieldVisibleRectEmpty=\(field.visibleRect.isEmpty)"
            + " cellAttached=\(cell.controlView === field) matches=\(matchCount)")

        let publicClasses: [(AnyClass, String)] = [
            (NSAccessibilityElement.self, "NSAccessibilityElement"),
            (NSSearchField.self, "NSSearchField"), (NSSearchFieldCell.self, "NSSearchFieldCell"),
            (NSButtonCell.self, "NSButtonCell"), (NSActionCell.self, "NSActionCell"),
            (NSTextFieldCell.self, "NSTextFieldCell"), (NSCell.self, "NSCell"),
            (NSButton.self, "NSButton"), (NSTextField.self, "NSTextField"), (NSView.self, "NSView")
        ]
        let publicRoles: [NSAccessibility.Role] = [
            .button, .textField, .group, .image, .staticText, .unknown, .window, .cell
        ]
        // Separate budgets and identity maps for the field (root=0) and cell
        // (root=1). Edge 0 is the root, 1 direct children, 2 navigation children.
        // IDs are local integers, never addresses. Repeated edges consume budget
        // too, since navigation children can merely reorder the same objects.
        for (root, element) in [field as AnyObject, cell as AnyObject].enumerated() {
            let maxRecords = 32
            let maxDepth = 3
            var identities: [ObjectIdentifier: Int] = [:]
            var records = 0
            var nodeLimitReached = false
            var depthLimitReached = false
            func visit(_ object: AnyObject, parent: Int, edge: Int, index: Int, depth: Int) throws {
                guard records < maxRecords else { nodeLimitReached = true; return }
                records += 1
                // Classify the actual class before attempting protocol conformance;
                // subclasses not exactly in the public allowlist become "other".
                let publicClass = publicClasses.first { type(of: object) == $0.0 }?.1 ?? "other"
                let identity = ObjectIdentifier(object)
                let repeated = identities[identity] != nil
                let node = identities[identity] ?? identities.count
                identities[identity] = node
                let accessibility = object as? any NSAccessibilityProtocol
                print("settingsSearchAX root=\(root) parent=\(parent) edge=\(edge) index=\(index)"
                    + " depth=\(depth) node=\(node) repeated=\(repeated) class=\(publicClass)"
                    + " conformsToNSAccessibility=\(accessibility != nil)")
                guard let accessibility else { return }
                let role = accessibility.accessibilityRole()
                let publicRole = publicRoles.first { $0 == role }?.rawValue ?? "otherOrAbsent"
                let identifier = accessibility.accessibilityIdentifier()
                // Identifier classification: 0 absent, 1 one of the two known
                // static Settings IDs already asserted here, 2 other. No raw IDs.
                let identifierClass: Int
                if let identifier, !identifier.isEmpty {
                    identifierClass = ["settings.root.search", "settings.root.searchButton"].contains(identifier) ? 1 : 2
                } else {
                    identifierClass = 0
                }
                print("settingsSearchAX root=\(root) node=\(node) role=\(publicRole)"
                    + " rolePresent=\(role != nil)"
                    + " identifierClass=\(identifierClass)"
                    + " labelMatchesExpected=\(accessibility.accessibilityLabel() == expectedLabel)"
                    + " axElement=\(accessibility.isAccessibilityElement())")
                guard !repeated else { return }
                let direct = try accessibilityChildArray(in: accessibility,
                    selector: #selector(NSAccessibilityProtocol.accessibilityChildren))
                let navigation = try accessibilityChildArray(in: accessibility,
                    selector: #selector(NSAccessibilityProtocol.accessibilityChildrenInNavigationOrder))
                print("settingsSearchAX root=\(root) node=\(node)"
                    + " directChildren=\(direct.count) navigationChildren=\(navigation.count)")
                guard depth < maxDepth else {
                    depthLimitReached = depthLimitReached || direct.count > 0 || navigation.count > 0
                    return
                }
                for (childEdge, children) in [(1, direct), (2, navigation)] {
                    for childIndex in 0..<children.count {
                        guard records < maxRecords else { nodeLimitReached = true; break }
                        try visit(children.object(at: childIndex) as AnyObject, parent: node, edge: childEdge,
                            index: childIndex, depth: depth + 1)
                    }
                }
            }
            try visit(element, parent: -1, edge: 0, index: 0, depth: 0)
            print("settingsSearchAX root=\(root) records=\(records) uniqueNodes=\(identities.count)"
                + " maxRecords=\(maxRecords) maxDepth=\(maxDepth)"
                + " nodeLimitReached=\(nodeLimitReached) depthLimitReached=\(depthLimitReached)")
        }
    }

    // Keep the Objective-C collection intact: typed Swift getter bridging can
    // trap before traversal can inspect a native accessibility child.
    private func accessibilityChildArray(in element: any NSAccessibilityProtocol,
                                         selector: Selector) throws -> NSArray {
        let object = try #require(element as? NSObject)
        try #require(object.responds(to: selector))
        guard let result = object.perform(selector)?.takeUnretainedValue() else { return NSArray() }
        return try #require(result as? NSArray)
    }

    private func accessibilityButtons(in element: any NSAccessibilityProtocol,
                                      visited: inout Set<ObjectIdentifier>) throws -> [any NSAccessibilityProtocol] {
        guard visited.insert(ObjectIdentifier(element as AnyObject)).inserted else { return [] }
        var buttons: [any NSAccessibilityProtocol] = element.accessibilityRole() == .button ? [element] : []
        let children = try accessibilityChildArray(in: element,
            selector: #selector(NSAccessibilityProtocol.accessibilityChildren))
        for index in 0..<children.count {
            guard let child = children.object(at: index) as? any NSAccessibilityProtocol else { continue }
            buttons += try accessibilityButtons(in: child, visited: &visited)
        }
        return buttons
    }
}
#endif
