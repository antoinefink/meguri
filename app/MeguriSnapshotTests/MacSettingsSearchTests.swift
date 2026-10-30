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
            window.layoutIfNeeded()
            host.layoutSubtreeIfNeeded()
            let fields = searchFields(in: host)
            try #require(fields.count == 1)
            let field = try #require(fields.first)
            #expect((field as any NSAccessibilityProtocol).accessibilityIdentifier == "settings.root.search")
            var visited: Set<ObjectIdentifier> = []
            let buttons = accessibilityButtons(in: field, visited: &visited)
            #expect(buttons.filter { $0.accessibilityIdentifier == "settings.root.searchButton" }.count == 1)
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
    private func accessibilityButtons(in element: any NSAccessibilityProtocol,
                                      visited: inout Set<ObjectIdentifier>) -> [any NSAccessibilityProtocol] {
        guard visited.insert(ObjectIdentifier(element as AnyObject)).inserted else { return [] }
        let own: [any NSAccessibilityProtocol] = element.accessibilityRole() == .button ? [element] : []
        return own + (element.accessibilityChildren() ?? []).flatMap { child -> [any NSAccessibilityProtocol] in
            guard let child = child as? any NSAccessibilityProtocol else { return [] }
            return accessibilityButtons(in: child, visited: &visited)
        }
    }
}
#endif
