#if os(macOS)
import AppKit
@testable import AppModel
import CoreBridge
import Darwin
import Settings
import SwiftUI
import Testing

@MainActor @Suite(.serialized) struct MacSettingsSearchTests {
    @Test(.timeLimit(.minutes(1))) func normalAccountSyncShowsRealMeguriSetupControls() async throws {
        try await SnapshotAppLifecycle.shared.withExclusiveAccess {
            let files = FileManager.default
            let temporary = try #require(realpath(files.temporaryDirectory.path, nil))
            defer { free(temporary) }
            let root = URL(filePath: String(cString: temporary)).appending(path: "synthetic-native-account-sync-\(UUID().uuidString)")
            let directory = root.appending(path: "collection")
            let backups = root.appending(path: "backups")
            let installation = root.appending(path: "installation")
            try files.createDirectory(at: directory, withIntermediateDirectories: true)
            defer { try? files.removeItem(at: root) }
            var configuration = LaunchConfiguration()
            configuration.collectionLocation = AppModel.CollectionLocation(directory: directory, backupsDirectory: backups)
            let sync = SyncCoordinator(directory: directory, defaults: nil)
            let collection = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
                providerDirectory: installation)
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: 700),
                styleMask: [.titled, .resizable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let controller = NSHostingController(rootView: AccountSyncView(collection: collection, host: .settings))
            defer {
                window.contentViewController = nil; window.close()
                withExtendedLifetime(controller) {}
            }
            do {
                await collection.start()
                try #require(collection.state == .noFile)
                // Import the public synthetic package through the ordinary
                // Files owner, with every persistence root under this fixture.
                var checkout = URL(filePath: #filePath)
                for _ in 0..<3 { checkout.deleteLastPathComponent() }
                let source = checkout.appending(path: "app/Fixtures/CollectionEntry/review.colpkg")
                collection.entry.present(); collection.entry.inspect(source)
                await collection.entry.waitForCompletion()
                try #require(collection.entry.preview != nil)
                collection.entry.confirm(); await collection.entry.waitForCompletion()
                try #require(collection.state == .ready)
                try #require(collection.sync.providerState?.selection == .localOnly)
                window.contentViewController = controller
                window.orderFront(nil); window.layoutIfNeeded(); controller.view.layoutSubtreeIfNeeded()
                let picker = try #require(nativeServicePickers(in: controller.view).first)
                #expect(Set(picker.itemTitles) == [SyncText.value("sync.service.iCloud"), SyncText.value("sync.service.meguri")])
                picker.selectItem(withTitle: SyncText.value("sync.service.meguri"))
                try #require(picker.sendAction(picker.action, to: picker.target))
                try await waitForNativeAccountState {
                    collection.sync.providerState?.selection == .meguri && !collection.sync.meguriBusy
                }
                window.layoutIfNeeded(); controller.view.layoutSubtreeIfNeeded()
                for id in ["address", "username", "password", "signIn"] {
                    let accountElementResult = try nativeAccountElement("settings.accountSync.meguri.\(id)", in: controller.view)
                    _ = try #require(accountElementResult)
                }
                let passwordElementResult = try nativeAccountElement("settings.accountSync.meguri.password", in: controller.view)
                let password = try #require(passwordElementResult)
                #expect(accessibilityString(in: password, selector: #selector(NSAccessibilityProtocol.accessibilitySubrole))
                    == NSAccessibility.Subrole.secureTextField.rawValue)
                for id in ["settings.accountSync.iCloudHeading", "settings.accountSync.connect", "settings.accountSync.automatic", "settings.accountSync.media"] {
                    #expect(try nativeAccountElement(id, in: controller.view) == nil)
                }
                #expect(collection.sync.meguriPhase == .unconfigured)
                #expect(collection.sync.meguriPreview == nil && collection.sync.meguriRoundReport == nil)
                let currentPicker = try #require(nativeServicePickers(in: controller.view).first)
                currentPicker.selectItem(withTitle: SyncText.value("sync.service.iCloud"))
                try #require(currentPicker.sendAction(currentPicker.action, to: currentPicker.target))
                try await waitForNativeAccountState { window.attachedSheet != nil }
                let sheet = try #require(window.attachedSheet)
                let sheetContent = try #require(sheet.contentView)
                let serviceSwitchConfirmElementResult = try nativeAccountElement("sync.serviceSwitch.confirm", in: sheetContent)
                _ = try #require(serviceSwitchConfirmElementResult)
                let revision = collection.sync.providerState?.revision
                let enter = try #require(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                    timestamp: 0, windowNumber: sheet.windowNumber, context: nil,
                    characters: "\r", charactersIgnoringModifiers: "\r", isARepeat: false, keyCode: 36))
                sheet.sendEvent(enter)
                await Task.yield()
                #expect(window.attachedSheet === sheet)
                #expect(!collection.syncPresentation.serviceChanging)
                #expect(collection.sync.providerState?.selection == .meguri)
                #expect(collection.sync.providerState?.revision == revision)
                let serviceSwitchCancelElementResult = try nativeAccountElement("sync.serviceSwitch.cancel", in: sheetContent)
                let cancel = try #require(serviceSwitchCancelElementResult)
                let press = NSSelectorFromString("accessibilityPerformPress")
                try #require(cancel.responds(to: press))
                _ = cancel.perform(press)
                try await waitForNativeAccountState { window.attachedSheet == nil }
                #expect(collection.sync.providerState?.selection == .meguri)
                #expect(collection.sync.meguriLastCommand == nil && collection.sync.meguriRoundReport == nil)
                try #require(await collection.close())
            } catch { _ = await collection.close(); throw error }
        }
    }
    private func waitForNativeAccountState(_ ready: () throws -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(20)
        while ContinuousClock.now < deadline {
            if try ready() { return }
            await Task.yield()
        }
        throw NativeAccountStateFailure.notSettled
    }
    private enum NativeAccountStateFailure: Error { case notSettled }
    private func nativeServicePickers(in view: NSView) -> [NSPopUpButton] {
        var matches: [NSPopUpButton] = []
        if let picker = view as? NSPopUpButton,
           picker.itemTitles.contains(SyncText.value("sync.service.meguri")) { matches.append(picker) }
        return matches + view.subviews.flatMap { nativeServicePickers(in: $0) }
    }
    private func nativeAccountElement(_ identifier: String, in root: NSObject) throws -> NSObject? {
        var visited: Set<ObjectIdentifier> = []
        func find(_ object: NSObject) throws -> NSObject? {
            guard visited.insert(ObjectIdentifier(object)).inserted else { return nil }
            if accessibilityString(in: object, selector: #selector(NSAccessibilityProtocol.accessibilityIdentifier)) == identifier { return object }
            let children = try accessibilityChildArray(in: object, selector: #selector(NSAccessibilityProtocol.accessibilityChildren))
            defer { withExtendedLifetime(children) {} }
            for index in 0..<children.count {
                let child = try #require(children.object(at: index) as? NSObject)
                if let found = try find(child) { return found }
            }
            return nil
        }
        return try find(root)
    }

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
        case #selector(NSAccessibilityProtocol.accessibilitySubrole):
            attribute = .subrole
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
