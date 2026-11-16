#if os(macOS)
import AppKit
import AppModel
import ApplicationServices
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
        let controllers = (0..<2).map { _ in
            NSHostingController(rootView: MacSettingsView(collection: collection, present: { _ in }))
        }
        defer {
            window.contentViewController = nil
            window.close()
            withExtendedLifetime(controllers) {}
        }
        for controller in controllers {
            window.contentViewController = controller
            window.setContentSize(NSSize(width: 900, height: 700))
            let host = controller.view
            window.orderFront(nil)
            window.layoutIfNeeded()
            host.layoutSubtreeIfNeeded()
            window.displayIfNeeded()
            let fields = searchFields(in: host)
            try #require(fields.count == 1)
            let field = try #require(fields.first)
            let fieldVisibleRect = field.visibleRect
            let hasVisibleGeometry = !fieldVisibleRect.isEmpty
            let geometryDiagnostic = "Search field must have visible geometry before AX traversal"
                + " capturedVisibleRect=\(fieldVisibleRect) hasVisibleGeometry=\(hasVisibleGeometry)"
            try #require(hasVisibleGeometry, "\(geometryDiagnostic)")
            try recordSearchGeometryDiagnostic(window: window, field: field, host: host,
                fieldVisibleRect: fieldVisibleRect)
            #expect((field as any NSAccessibilityProtocol).accessibilityIdentifier() == "settings.root.search")
            let cell = try #require(field.cell as? NSSearchFieldCell)
            let searchLabel = try #require(cell.searchButtonCell?.accessibilityLabel())
            try #require(!searchLabel.isEmpty)
            var visited: Set<ObjectIdentifier> = []
            let buttons = try accessibilityButtons(in: field, visited: &visited)
            let matches = buttons.filter {
                accessibilityString(in: $0, selector: #selector(NSAccessibilityProtocol.accessibilityLabel)) == searchLabel
            }
            try recordSearchAccessibilityDiagnostic(window: window, field: field, cell: cell,
                expectedLabel: searchLabel, matchCount: matches.count, fieldVisibleRect: fieldVisibleRect)
            try #require(matches.count == 1)
            let search = try #require(matches.first)
            #expect(accessibilityString(in: search,
                selector: #selector(NSAccessibilityProtocol.accessibilityIdentifier)) == "settings.root.searchButton")
            let identified = buttons.filter {
                accessibilityString(in: $0,
                    selector: #selector(NSAccessibilityProtocol.accessibilityIdentifier)) == "settings.root.searchButton"
            }
            try #require(identified.count == 1)
            #expect(identified[0] === search)
            #expect(field.stringValue == "iCloud")
            // A remount must identify the new native elements, without relying
            // on a retained field, timer or global window lookup.
            window.contentViewController = nil
        }
    }

    private func searchFields(in view: NSView) -> [NSSearchField] {
        if let field = view as? NSSearchField { return [field] }
        return view.subviews.flatMap { searchFields(in: $0) }
    }

    private func recordSearchGeometryDiagnostic(window: NSWindow, field: NSSearchField,
        host: NSView, fieldVisibleRect: NSRect) throws {
        let maxNodes = 16
        var chain: [NSView] = []
        var identities: Set<ObjectIdentifier> = []
        var current: NSView? = field
        var reachedHost = false
        var repeated = false
        while let view = current, chain.count < maxNodes {
            guard identities.insert(ObjectIdentifier(view)).inserted else {
                repeated = true
                break
            }
            chain.append(view)
            if view === host {
                reachedHost = true
                break
            }
            current = view.superview
        }
        print("settingsSearchGeometry ownedHostReached=\(reachedHost) nodes=\(chain.count)"
            + " maxNodes=\(maxNodes) repeated=\(repeated)"
            + " hostBounds=\(host.bounds) windowContentLayoutRect=\(window.contentLayoutRect)")
        // Validate ownership before recording any ancestor geometry. Never
        // inspect the host's parent or emit metadata from an unowned chain.
        try #require(reachedHost, "Search field ancestor chain must reach this test's hosting view")
        let publicClasses: [(AnyClass, String)] = [
            (NSSearchField.self, "NSSearchField"), (NSTextField.self, "NSTextField"),
            (NSClipView.self, "NSClipView"), (NSScrollView.self, "NSScrollView"),
            (NSStackView.self, "NSStackView"), (NSView.self, "NSView")
        ]
        for (index, view) in chain.enumerated() {
            let identity = view === field ? "field" : view === host ? "ownedHost" : "ancestor"
            let publicClass = view === host ? "NSHostingView"
                : publicClasses.first { type(of: view) == $0.0 }?.1 ?? "other"
            let visibleRect = view === field ? fieldVisibleRect : view.visibleRect
            let visibleIntersection = view.bounds.intersection(visibleRect)
            print("settingsSearchGeometry index=\(index) identity=\(identity) class=\(publicClass)"
                + " frame=\(view.frame) bounds=\(view.bounds) visibleRect=\(visibleRect)"
                + " frameEmpty=\(view.frame.isEmpty) boundsEmpty=\(view.bounds.isEmpty)"
                + " visibleRectEmpty=\(visibleRect.isEmpty)"
                + " visibleIntersection=\(visibleIntersection) visibleIntersectionEmpty=\(visibleIntersection.isEmpty)"
                + " hidden=\(view.isHidden) hiddenBySelfOrAncestor=\(view.isHiddenOrHasHiddenAncestor)")
            if index + 1 < chain.count {
                let parent = chain[index + 1]
                let intersection = view.frame.intersection(parent.bounds)
                print("settingsSearchGeometry index=\(index) parentIndex=\(index + 1)"
                    + " parentBoundsIntersection=\(intersection) parentBoundsIntersectionEmpty=\(intersection.isEmpty)"
                    + " clippedByParentBounds=\(!parent.bounds.contains(view.frame))")
            }
        }
    }

    // Diagnostic only: never emit UI strings or
    // runtime class names outside this exact public-AppKit allowlist.
    private func recordSearchAccessibilityDiagnostic(window: NSWindow, field: NSSearchField,
        cell: NSSearchFieldCell, expectedLabel: String, matchCount: Int, fieldVisibleRect: NSRect) throws {
        print("settingsSearchAX windowContentAttached=\(window.contentView != nil)"
            + " windowVisible=\(window.isVisible) windowMiniaturized=\(window.isMiniaturized)"
            + " fieldAttached=\(field.window === window) fieldHidden=\(field.isHidden)"
            + " fieldHiddenBySelfOrAncestor=\(field.isHiddenOrHasHiddenAncestor)"
            + " capturedFieldVisibleRectEmpty=\(fieldVisibleRect.isEmpty)"
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
        func cfTypeClass(_ value: CFTypeRef?) -> String {
            guard let value else { return "absent" }
            let typeID = CFGetTypeID(value)
            if typeID == AXUIElementGetTypeID() { return "axElement" }
            if typeID == CFStringGetTypeID() { return "string" }
            if typeID == CFArrayGetTypeID() { return "array" }
            return "other"
        }
        func legacyValueClass(_ value: AnyObject?) -> String {
            guard let value else { return "absent" }
            if value is String { return "string" }
            if value is NSArray { return "array" }
            return "other"
        }
        // Separate budgets and identity maps for the field (root=0) and cell
        // (root=1). Edge 0 is the root, 1 direct children, 2 navigation children.
        // IDs are local integers, never addresses. Repeated edges consume budget
        // too, since navigation children can merely reorder the same objects.
        for (root, element) in [field as NSObject, cell as NSObject].enumerated() {
            let maxRecords = 32
            let maxDepth = 3
            var identities: [ObjectIdentifier: Int] = [:]
            var records = 0
            var nodeLimitReached = false
            var depthLimitReached = false
            func visit(_ object: AnyObject, parent: Int, edge: Int, index: Int, depth: Int) throws {
                guard records < maxRecords else { nodeLimitReached = true; return }
                records += 1
                // Classify the retained object before any AX call or typed cast.
                print("settingsSearchAX root=\(root) parent=\(parent) edge=\(edge) index=\(index)"
                    + " depth=\(depth) cfType=\(cfTypeClass(object))")
                guard let object = object as? NSObject else {
                    print("settingsSearchAX root=\(root) cfObjectBridge=unsupported")
                    return
                }
                // Subclasses not exactly in the public allowlist become "other".
                let publicClass = publicClasses.first { type(of: object) == $0.0 }?.1 ?? "other"
                let identity = ObjectIdentifier(object)
                let repeated = identities[identity] != nil
                let node = identities[identity] ?? identities.count
                identities[identity] = node
                let roleGetter = #selector(NSAccessibilityProtocol.accessibilityRole)
                let labelGetter = #selector(NSAccessibilityProtocol.accessibilityLabel)
                let identifierGetter = #selector(NSAccessibilityProtocol.accessibilityIdentifier)
                let childrenGetter = #selector(NSAccessibilityProtocol.accessibilityChildren)
                let navigationGetter = #selector(NSAccessibilityProtocol.accessibilityChildrenInNavigationOrder)
                print("settingsSearchAX root=\(root) parent=\(parent) edge=\(edge) index=\(index)"
                    + " depth=\(depth) node=\(node) repeated=\(repeated) class=\(publicClass)"
                    + " roleGetter=\(object.responds(to: roleGetter))"
                    + " labelGetter=\(object.responds(to: labelGetter))"
                    + " identifierGetter=\(object.responds(to: identifierGetter))"
                    + " childrenGetter=\(object.responds(to: childrenGetter))"
                    + " navigationGetter=\(object.responds(to: navigationGetter))")
                let role = accessibilityString(in: object, selector: roleGetter)
                let publicRole = publicRoles.first { $0.rawValue == role }?.rawValue ?? "otherOrAbsent"
                let identifier = accessibilityString(in: object, selector: identifierGetter)
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
                    + " labelMatchesExpected=\(accessibilityString(in: object, selector: labelGetter) == expectedLabel)")
                let legacyNamesGetter = #selector(NSObject.accessibilityAttributeNames)
                let legacyValueGetter = #selector(NSObject.accessibilityAttributeValue(_:))
                let legacyIgnoredGetter = #selector(NSObject.accessibilityIsIgnored)
                let supportsLegacyNames = object.responds(to: legacyNamesGetter)
                let supportsLegacyValue = object.responds(to: legacyValueGetter)
                print("settingsSearchAX root=\(root) node=\(node)"
                    + " legacyNamesGetter=\(supportsLegacyNames) legacyValueGetter=\(supportsLegacyValue)"
                    + " legacyIgnoredGetter=\(object.responds(to: legacyIgnoredGetter))")
                let legacyNames = supportsLegacyNames
                    ? object.perform(legacyNamesGetter)?.takeUnretainedValue() : nil
                let names = legacyNames as? NSArray
                defer { withExtendedLifetime(legacyNames) {} }
                print("settingsSearchAX root=\(root) node=\(node) legacyNamesType=\(legacyValueClass(legacyNames))"
                    + " legacyHasRole=\(names?.contains(NSAccessibility.Attribute.role.rawValue) ?? false)"
                    + " legacyHasDescription=\(names?.contains(NSAccessibility.Attribute.description.rawValue) ?? false)"
                    + " legacyHasIdentifier=\(names?.contains(NSAccessibility.Attribute.identifier.rawValue) ?? false)"
                    + " legacyHasChildren=\(names?.contains(NSAccessibility.Attribute.children.rawValue) ?? false)")
                func legacyValue(_ attribute: NSAccessibility.Attribute) -> AnyObject? {
                    guard supportsLegacyValue else { return nil }
                    return object.perform(legacyValueGetter, with: attribute.rawValue as NSString)?.takeUnretainedValue()
                }
                let legacyRole = legacyValue(.role)
                let legacyDescription = legacyValue(.description)
                let legacyIdentifier = legacyValue(.identifier)
                let legacyChildren = legacyValue(.children)
                defer { withExtendedLifetime(legacyChildren) {} }
                let legacyPublicRole = publicRoles.first { $0.rawValue == (legacyRole as? String) }?.rawValue
                    ?? "otherOrAbsent"
                let legacyIdentifierString = legacyIdentifier as? String
                let legacyIdentifierClass: Int
                if let legacyIdentifierString, !legacyIdentifierString.isEmpty {
                    legacyIdentifierClass = ["settings.root.search", "settings.root.searchButton"]
                        .contains(legacyIdentifierString) ? 1 : 2
                } else {
                    legacyIdentifierClass = 0
                }
                print("settingsSearchAX root=\(root) node=\(node)"
                    + " legacyRoleType=\(legacyValueClass(legacyRole)) legacyRole=\(legacyPublicRole)"
                    + " legacyDescriptionType=\(legacyValueClass(legacyDescription))"
                    + " legacyDescriptionMatchesExpected=\((legacyDescription as? String) == expectedLabel)"
                    + " legacyIdentifierType=\(legacyValueClass(legacyIdentifier))"
                    + " legacyIdentifierClass=\(legacyIdentifierClass)"
                    + " legacyChildrenType=\(legacyValueClass(legacyChildren))"
                    + " legacyChildCount=\((legacyChildren as? NSArray)?.count ?? 0)")
                // Observe legacy children only; traversal remains on the modern getters.
                guard !repeated else { return }
                let direct = try accessibilityChildArray(in: object, selector: childrenGetter)
                let navigation = try accessibilityChildArray(in: object, selector: navigationGetter)
                print("settingsSearchAX root=\(root) node=\(node)"
                    + " directChildren=\(direct.count) navigationChildren=\(navigation.count)")
                guard depth < maxDepth else {
                    depthLimitReached = depthLimitReached || direct.count > 0 || navigation.count > 0
                    return
                }
                for (childEdge, children) in [(1, direct), (2, navigation)] {
                    defer { withExtendedLifetime(children) {} }
                    for childIndex in 0..<children.count {
                        guard records < maxRecords else { nodeLimitReached = true; break }
                        let child = try #require(children.object(at: childIndex) as? NSObject)
                        try visit(child, parent: node, edge: childEdge,
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
    private func accessibilityChildArray(in object: NSObject,
                                         selector: Selector) throws -> NSArray {
        guard object.responds(to: selector) else { return NSArray() }
        guard let result = object.perform(selector)?.takeUnretainedValue() else { return NSArray() }
        return try #require(result as? NSArray)
    }

    private func accessibilityString(in object: NSObject, selector: Selector) -> String? {
        guard object.responds(to: selector) else { return nil }
        return object.perform(selector)?.takeUnretainedValue() as? String
    }

    private func accessibilityButtons(in element: NSObject,
                                      visited: inout Set<ObjectIdentifier>) throws -> [NSObject] {
        guard visited.insert(ObjectIdentifier(element)).inserted else { return [] }
        let role = accessibilityString(in: element, selector: #selector(NSAccessibilityProtocol.accessibilityRole))
        var buttons: [NSObject] = role == NSAccessibility.Role.button.rawValue ? [element] : []
        let children = try accessibilityChildArray(in: element,
            selector: #selector(NSAccessibilityProtocol.accessibilityChildren))
        defer { withExtendedLifetime(children) {} }
        for index in 0..<children.count {
            let child = try #require(children.object(at: index) as? NSObject)
            buttons += try accessibilityButtons(in: child, visited: &visited)
        }
        return buttons
    }
}
#endif
