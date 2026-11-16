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
            recordSearchAccessibilityDiagnostic(window: window, field: field, cell: cell,
                matchCount: matches.count, fieldVisibleRect: fieldVisibleRect)
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

    // Fixed-safe attachment and match evidence only.
    private func recordSearchAccessibilityDiagnostic(window: NSWindow, field: NSSearchField,
        cell: NSSearchFieldCell, matchCount: Int, fieldVisibleRect: NSRect) {
        print("settingsSearchAX windowContentAttached=\(window.contentView != nil)"
            + " windowVisible=\(window.isVisible) windowMiniaturized=\(window.isMiniaturized)"
            + " fieldAttached=\(field.window === window) fieldHidden=\(field.isHidden)"
            + " fieldHiddenBySelfOrAncestor=\(field.isHiddenOrHasHiddenAncestor)"
            + " capturedFieldVisibleRectEmpty=\(fieldVisibleRect.isEmpty)"
            + " cellAttached=\(cell.controlView === field) matches=\(matchCount)")
    }

    // Keep the Objective-C collection intact: typed Swift getter bridging can
    // trap before traversal can inspect a native accessibility child.
    private func accessibilityChildArray(in object: NSObject,
                                         selector: Selector) throws -> NSArray {
        guard let result = accessibilityValue(in: object, selector: selector) else { return NSArray() }
        return try #require(result as? NSArray)
    }

    private func accessibilityString(in object: NSObject, selector: Selector) -> String? {
        accessibilityValue(in: object, selector: selector) as? String
    }

    private func accessibilityValue(in object: NSObject, selector: Selector) -> AnyObject? {
        // A supported modern getter owns its result, including nil or a wrong type.
        if object.responds(to: selector) {
            return object.perform(selector)?.takeUnretainedValue()
        }
        let attribute: NSAccessibility.Attribute
        switch selector {
        case #selector(NSAccessibilityProtocol.accessibilityRole):
            attribute = .role
        case #selector(NSAccessibilityProtocol.accessibilityLabel):
            attribute = .description
        case #selector(NSAccessibilityProtocol.accessibilityIdentifier):
            attribute = .identifier
        case #selector(NSAccessibilityProtocol.accessibilityChildren):
            attribute = .children
        default:
            return nil
        }
        let legacyGetter = NSSelectorFromString("accessibilityAttributeValue:")
        guard object.responds(to: legacyGetter) else { return nil }
        return object.perform(legacyGetter, with: attribute.rawValue as NSString)?.takeUnretainedValue()
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
