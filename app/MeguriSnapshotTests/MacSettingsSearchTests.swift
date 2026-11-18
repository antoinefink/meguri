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
            let contentSize = NSSize(width: 900, height: 700)
            let window = NSWindow(contentRect: NSRect(origin: .zero, size: contentSize),
                styleMask: [.titled, .resizable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let controller = NSHostingController(rootView: AccountSyncView(collection: collection, host: .settings))
            controller.sizingOptions = []
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
                window.setContentSize(contentSize)
                controller.view.frame = NSRect(origin: .zero, size: contentSize)
                window.orderFront(nil); window.layoutIfNeeded(); controller.view.layoutSubtreeIfNeeded()
                var lastObservedGraph: NativeAccountGraph?
                do {
                    try await waitForNativeAccountState {
                        window.layoutIfNeeded(); controller.view.layoutSubtreeIfNeeded(); window.displayIfNeeded()
                        let graph = nativeAccountGraph(in: controller.view, window: window, controller: controller)
                        lastObservedGraph = graph
                        return graph.complete && graph.matches("settings.accountSync.service").count == 1
                    }
                    guard let graph = lastObservedGraph else { throw NativeAccountStateFailure.incompleteGraph }
                    try selectNativeService(SyncText.value("sync.service.meguri"), in: graph)
                } catch {
                    lastObservedGraph?.recordDiagnostic()
                    throw error
                }
                try await waitForNativeAccountState {
                    collection.sync.providerState?.selection == .meguri && !collection.sync.meguriBusy
                }
                window.layoutIfNeeded(); controller.view.layoutSubtreeIfNeeded()
                let accountGraph = nativeAccountGraph(in: controller.view, window: window, controller: controller)
                for id in ["address", "username", "password", "signIn"] {
                    let accountElementResult = try accountGraph.element("settings.accountSync.meguri.\(id)")
                    _ = try #require(accountElementResult)
                }
                let passwordElementResult = try accountGraph.element("settings.accountSync.meguri.password")
                let password = try #require(passwordElementResult)
                let securePassword = try nativeAccountString(in: password,
                    selector: #selector(NSAccessibilityProtocol.accessibilitySubrole)) == NSAccessibility.Subrole.secureTextField.rawValue
                #expect(securePassword)
                for id in ["settings.accountSync.iCloudHeading", "settings.accountSync.connect", "settings.accountSync.automatic", "settings.accountSync.media"] {
                    let absent = try accountGraph.element(id) == nil
                    #expect(absent)
                }
                #expect(collection.sync.meguriPhase == .unconfigured)
                #expect(collection.sync.meguriPreview == nil && collection.sync.meguriRoundReport == nil)
                let currentGraph = nativeAccountGraph(in: controller.view, window: window, controller: controller)
                do { try selectNativeService(SyncText.value("sync.service.iCloud"), in: currentGraph) }
                catch { currentGraph.recordDiagnostic(); throw error }
                try await waitForNativeAccountState { window.attachedSheet != nil }
                let sheet = try #require(window.attachedSheet)
                let sheetContent = try #require(sheet.contentView)
                let sheetGraph = nativeAccountGraph(in: sheetContent, window: sheet)
                let serviceSwitchConfirmElementResult = try sheetGraph.element("sync.serviceSwitch.confirm")
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
                let cancelGraph = nativeAccountGraph(in: sheetContent, window: sheet)
                let serviceSwitchCancelElementResult = try cancelGraph.element("sync.serviceSwitch.cancel")
                let cancel = try #require(serviceSwitchCancelElementResult)
                guard cancelGraph.nodes[ObjectIdentifier(cancel)]?.role == NSAccessibility.Role.button.rawValue else {
                    throw NativeAccountStateFailure.inappropriateRole
                }
                try performNativeAccountPress(on: cancel)
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
    private enum NativeAccountStateFailure: Error {
        case notSettled, incompleteGraph, duplicateIdentifier, inappropriateRole, invalidOptions
        case unsupportedGetter, malformedGetter, unsupportedAction, actionFailed
    }
    private struct NativeAccountNode {
        let object: NSObject
        let identifier: String?
        let role: String?
        let parent: ObjectIdentifier?
    }
    private struct NativeAccountGraph {
        var nodes: [ObjectIdentifier: NativeAccountNode] = [:]
        var edges: [ObjectIdentifier: Set<ObjectIdentifier>] = [:]
        var visited: Set<ObjectIdentifier> = []
        var mounted = false
        var objectLimit = false, depthLimit = false, foreign = false
        var unsupported = false, malformed = false, unknownNilChildren = false
        var complete: Bool {
            mounted && !objectLimit && !depthLimit && !foreign
                && !unsupported && !malformed && !unknownNilChildren
        }
        func matches(_ identifier: String) -> [NativeAccountNode] {
            nodes.values.filter { $0.identifier == identifier }
        }
        func element(_ identifier: String) throws -> NSObject? {
            guard complete else {
                recordDiagnostic()
                throw NativeAccountStateFailure.incompleteGraph
            }
            let found = matches(identifier)
            guard found.count <= 1 else { throw NativeAccountStateFailure.duplicateIdentifier }
            return found.first?.object
        }
        // This traverses only the already bounded snapshot, without additional getters or roots.
        func descendants(of object: NSObject) -> Set<ObjectIdentifier> {
            var found: Set<ObjectIdentifier> = []
            var pending = Array(edges[ObjectIdentifier(object)] ?? [])
            while let next = pending.popLast() {
                guard found.insert(next).inserted else { continue }
                pending.append(contentsOf: edges[next] ?? [])
            }
            return found
        }
        func recordDiagnostic() {
            print("nativeAccountGraph mounted=\(mounted) complete=\(complete)"
                + " visited=\(visited.count) nodes=\(nodes.count)"
                + " exactServiceCount=\(matches("settings.accountSync.service").count)"
                + " objectLimit=\(objectLimit) depthLimit=\(depthLimit) foreign=\(foreign)"
                + " unsupported=\(unsupported) malformed=\(malformed) unknownNilChildren=\(unknownNilChildren)")
        }
    }
    // Modern object getters retain their actual nullable result. Unsupported getters and
    // wrong types are failures, rather than an empty relationship or a missing identifier.
    private func nativeAccountString(in object: NSObject, selector: Selector) throws -> String? {
        guard object.responds(to: selector) else { throw NativeAccountStateFailure.unsupportedGetter }
        let value = object.perform(selector)?.takeUnretainedValue()
        guard value == nil || value is String else { throw NativeAccountStateFailure.malformedGetter }
        return value as? String
    }
    private func nativeAccountGraph(in root: NSView, window: NSWindow,
        controller: NSViewController? = nil) -> NativeAccountGraph {
        let maxObjects = 128, maxDepth = 16
        var graph = NativeAccountGraph()
        graph.mounted = root.window === window && window.contentView === root
            && (controller == nil || window.contentViewController === controller)
        guard graph.mounted else { return graph }
        var active: Set<ObjectIdentifier> = []
        // These roles permit terminal AX elements. A supported nullable children getter
        // can describe a leaf; nil on an unknown/container role cannot establish completeness.
        let leafRoles: Set<String> = [
            NSAccessibility.Role.button.rawValue, NSAccessibility.Role.checkBox.rawValue,
            NSAccessibility.Role.radioButton.rawValue, NSAccessibility.Role.menuItem.rawValue,
            NSAccessibility.Role.staticText.rawValue, NSAccessibility.Role.textField.rawValue,
            NSAccessibility.Role.image.rawValue
        ]
        func readString(_ object: NSObject, _ selector: Selector) -> String? {
            guard object.responds(to: selector) else { graph.unsupported = true; return nil }
            let value = object.perform(selector)?.takeUnretainedValue()
            guard value == nil || value is String else { graph.malformed = true; return nil }
            return value as? String
        }
        func inspect(_ object: NSObject, parent: NSObject?, depth: Int, native: Bool) {
            let identity = ObjectIdentifier(object)
            if graph.visited.contains(identity) {
                if active.contains(identity) { graph.malformed = true }
                if let storedParent = graph.nodes[identity]?.parent,
                   parent.map({ ObjectIdentifier($0) }) != storedParent { graph.malformed = true }
                return
            }
            guard graph.visited.count < maxObjects else { graph.objectLimit = true; return }
            graph.visited.insert(identity)
            guard depth <= maxDepth else { graph.depthLimit = true; return }
            // No getters or outgoing edges from a foreign window/application/view.
            guard !(object is NSWindow), !(object is NSApplication) else { graph.foreign = true; return }
            var axParent: ObjectIdentifier?
            if let view = object as? NSView {
                guard view.window === window, view === root || view.isDescendant(of: root) else {
                    graph.foreign = true; return
                }
                if native && view.superview !== parent { graph.malformed = true; return }
            } else {
                // A virtual node must have the actual public parent that exposed it.
                // Parent identities are checked, never followed into a second graph.
                let selector = #selector(NSAccessibilityProtocol.accessibilityParent)
                guard object.responds(to: selector) else { graph.unsupported = true; return }
                let value = object.perform(selector)?.takeUnretainedValue()
                guard let actualParent = value as? NSObject, let parent, actualParent === parent else {
                    graph.malformed = true; return
                }
                axParent = ObjectIdentifier(actualParent)
            }
            let identifier = readString(object, #selector(NSAccessibilityProtocol.accessibilityIdentifier))
            let role = readString(object, #selector(NSAccessibilityProtocol.accessibilityRole))
            if !(object is NSView) && role == nil { graph.malformed = true }
            graph.nodes[identity] = NativeAccountNode(object: object, identifier: identifier, role: role, parent: axParent)
            active.insert(identity)
            defer { active.remove(identity) }
            func visit(_ child: NSObject, native: Bool) {
                guard depth < maxDepth else { graph.depthLimit = true; return }
                let childIdentity = ObjectIdentifier(child)
                guard graph.visited.contains(childIdentity) || graph.visited.count < maxObjects else {
                    graph.objectLimit = true; return
                }
                graph.edges[identity, default: []].insert(childIdentity)
                inspect(child, parent: object, depth: depth + 1, native: native)
            }
            if let view = object as? NSView {
                let children = view.subviews
                if children.count > maxObjects { graph.objectLimit = true }
                for child in children.prefix(maxObjects) { visit(child, native: true) }
            }
            func inspectAXChildren(_ selector: Selector) {
                guard object.responds(to: selector) else { graph.unsupported = true; return }
                let value = object.perform(selector)?.takeUnretainedValue()
                guard let value else {
                    if !leafRoles.contains(role ?? "") { graph.unknownNilChildren = true }
                    return
                }
                guard let children = value as? NSArray else { graph.malformed = true; return }
                defer { withExtendedLifetime(children) {} }
                if children.count > maxObjects { graph.objectLimit = true }
                for index in 0..<min(children.count, maxObjects) {
                    guard let child = children.object(at: index) as? NSObject else {
                        graph.malformed = true; continue
                    }
                    visit(child, native: false)
                }
            }
            inspectAXChildren(#selector(NSAccessibilityProtocol.accessibilityChildren))
            // This is another ordering of public children, not a promise of extra nodes.
            // Union both observations under the same identity/depth/object budget.
            inspectAXChildren(#selector(NSAccessibilityProtocol.accessibilityChildrenInNavigationOrder))
        }
        inspect(root, parent: nil, depth: 0, native: false)
        return graph
    }
    private func selectNativeService(_ intendedLabel: String, in graph: NativeAccountGraph) throws {
        let matches = graph.matches("settings.accountSync.service")
        guard graph.complete else { throw NativeAccountStateFailure.incompleteGraph }
        guard matches.count == 1, let service = matches.first else {
            throw NativeAccountStateFailure.duplicateIdentifier
        }
        let serviceRoles: Set<String> = [NSAccessibility.Role.popUpButton.rawValue,
            NSAccessibility.Role.menuButton.rawValue, NSAccessibility.Role.comboBox.rawValue]
        guard let role = service.role, serviceRoles.contains(role) else {
            throw NativeAccountStateFailure.inappropriateRole
        }
        let optionRoles: Set<String> = [NSAccessibility.Role.menuItem.rawValue, NSAccessibility.Role.radioButton.rawValue]
        let options = graph.descendants(of: service.object).compactMap { graph.nodes[$0] }.filter {
            guard let role = $0.role else { return false }
            return optionRoles.contains(role)
        }
        guard options.count == 2 else { throw NativeAccountStateFailure.invalidOptions }
        let labels = try options.map {
            try nativeAccountString(in: $0.object, selector: #selector(NSAccessibilityProtocol.accessibilityLabel))
        }
        let expected: Set<String> = [SyncText.value("sync.service.iCloud"), SyncText.value("sync.service.meguri")]
        guard expected.count == 2, labels.allSatisfy({ $0 != nil }),
              Set(labels.compactMap { $0 }) == expected else { throw NativeAccountStateFailure.invalidOptions }
        let targets = zip(options, labels).filter { $0.1 == intendedLabel }.map { $0.0 }
        guard targets.count == 1, let target = targets.first else { throw NativeAccountStateFailure.invalidOptions }
        // Only options ALREADY exposed by this exact control are actionable here.
        // Never open the picker: public menu-opening actions may synchronously track.
        try performNativeAccountPress(on: target.object)
    }
    private func performNativeAccountPress(on object: NSObject) throws {
        let succeeded: Bool
        guard object.responds(to: #selector(NSAccessibilityProtocol.accessibilityPerformPress)) else {
            throw NativeAccountStateFailure.unsupportedAction
        }
        if let accessible = object as? any NSAccessibilityProtocol {
            succeeded = accessible.accessibilityPerformPress()
        } else if let accessible = object as? any NSAccessibilityButton {
            succeeded = accessible.accessibilityPerformPress()
        } else { throw NativeAccountStateFailure.unsupportedAction }
        // One actual typed Bool action, with no alternate action after a failure.
        guard succeeded else { throw NativeAccountStateFailure.actionFailed }
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
