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
                var lastObservedNativePickerCount: Int?
                do {
                    try await waitForNativeAccountState {
                        window.layoutIfNeeded(); controller.view.layoutSubtreeIfNeeded(); window.displayIfNeeded()
                        let pickerCount = nativeServicePickers(in: controller.view).count
                        lastObservedNativePickerCount = pickerCount
                        return pickerCount > 0
                    }
                } catch {
                    recordNativeServiceMountDiagnostic(window: window, controller: controller,
                        lastObservedNativePickerCount: lastObservedNativePickerCount)
                    throw error
                }
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
    // Diagnostic only: one owned-root graph, fixed scalars, public getters, no AX actions.
    private func recordNativeServiceMountDiagnostic(window: NSWindow, controller: NSViewController,
        lastObservedNativePickerCount: Int?) {
        let root = controller.view
        let controllerAttached = window.contentViewController === controller
        let hostAttached = root.window === window
        let contentAttached = window.contentView === root
        let mounted = controllerAttached && hostAttached && contentAttached
        let maxObjects = 128, maxDepth = 16
        var visited: Set<ObjectIdentifier> = []
        var visitedRoot = 0, visitedNative = 0, visitedAX = 0, skippedElsewhere = 0
        var objectLimit = false, depthLimit = false, incomplete = !mounted, readFailed = false
        var popupCount = 0, buttonCount = 0, controlCount = 0, viewCount = 0, otherCount = 0
        var matchingTitlePopups = 0, menuPresentPopups = 0, menuNonemptyPopups = 0
        var popupsWithICloudOption = 0, popupsWithMeguriOption = 0
        var exactServiceCount = 0, category = "missing", role = "missing"
        var roleGetterAvailable = false, childrenGetterAvailable = false
        var pickAvailable = false, pressAvailable = false, showMenuAvailable = false
        var rootIdentifierRoute = "not-observed", rootIdentifierKind = "not-observed"
        var rootChildrenRoute = "not-observed", rootChildrenKind = "not-observed"
        let identifierSelector = #selector(NSAccessibilityProtocol.accessibilityIdentifier)
        let roleSelector = #selector(NSAccessibilityProtocol.accessibilityRole)
        let childrenSelector = #selector(NSAccessibilityProtocol.accessibilityChildren)
        let legacyGetter = NSSelectorFromString("accessibilityAttributeValue:")
        let iCloudOption = SyncText.value("sync.service.iCloud")
        let meguriOption = SyncText.value("sync.service.meguri")
        struct GetterCounts {
            var modern = 0, legacy = 0, neither = 0
            var nilValue = 0, string = 0, array = 0, other = 0
            mutating func record(route: String, kind: String) {
                switch route {
                case "modern": modern += 1
                case "legacy": legacy += 1
                default: neither += 1
                }
                switch kind {
                case "nil": nilValue += 1
                case "string": string += 1
                case "array": array += 1
                default: other += 1
                }
            }
        }
        var identifiers = GetterCounts(), children = GetterCounts()
        func read(_ object: NSObject, selector: Selector) -> (value: AnyObject?, route: String, kind: String) {
            let route = object.responds(to: selector) ? "modern"
                : (object.responds(to: legacyGetter) ? "legacy" : "neither")
            // The existing reader preserves modern precedence, including nil/wrong types.
            let value = accessibilityValue(in: object, selector: selector)
            let kind = value == nil ? "nil" : (value is String ? "string" : (value is NSArray ? "array" : "other"))
            return (value, route, kind)
        }
        func publicCategory(_ object: NSObject) -> String {
            if object is NSPopUpButton { return "NSPopUpButton" }
            if object is NSButton { return "NSButton" }
            if object is NSControl { return "NSControl" }
            if object is NSView { return "NSView" }
            return "other"
        }
        func inspect(_ object: NSObject, depth: Int, viaNative: Bool) {
            guard !visited.contains(ObjectIdentifier(object)) else { return }
            guard visited.count < maxObjects else { objectLimit = true; incomplete = true; return }
            visited.insert(ObjectIdentifier(object))
            if depth == 0 { visitedRoot += 1 }
            else if viaNative { visitedNative += 1 }
            else { visitedAX += 1 }
            // These rejected objects consume a slot, but supply no getters or outgoing edges.
            if object is NSWindow { skippedElsewhere += 1; incomplete = true; return }
            if let view = object as? NSView, let owner = view.window, owner !== window {
                skippedElsewhere += 1; incomplete = true; return
            }
            let objectCategory = publicCategory(object)
            switch objectCategory {
            case "NSPopUpButton": popupCount += 1
            case "NSButton": buttonCount += 1
            case "NSControl": controlCount += 1
            case "NSView": viewCount += 1
            default: otherCount += 1
            }
            if let popup = object as? NSPopUpButton {
                // Read only this visited control's menu/title properties; do not walk menus/items.
                if popup.menu != nil { menuPresentPopups += 1 }
                if popup.numberOfItems > 0 { menuNonemptyPopups += 1 }
                let titles = popup.itemTitles
                if titles.contains(iCloudOption) { popupsWithICloudOption += 1 }
                if titles.contains(meguriOption) {
                    popupsWithMeguriOption += 1
                    matchingTitlePopups += 1
                }
            }
            let identifier = read(object, selector: identifierSelector)
            identifiers.record(route: identifier.route, kind: identifier.kind)
            let childResult = read(object, selector: childrenSelector)
            children.record(route: childResult.route, kind: childResult.kind)
            if depth == 0 {
                rootIdentifierRoute = identifier.route; rootIdentifierKind = identifier.kind
                rootChildrenRoute = childResult.route; rootChildrenKind = childResult.kind
            }
            // A supported nil identifier is missing metadata, not a getter failure.
            if identifier.route == "neither" || (identifier.kind != "nil" && identifier.kind != "string") {
                readFailed = true; incomplete = true
            }
            if identifier.value as? String == "settings.accountSync.service" {
                exactServiceCount += 1
                // Describe only the first exact-ID observation, never a title-only candidate.
                if exactServiceCount == 1 {
                    category = objectCategory
                    if let value = accessibilityString(in: object, selector: roleSelector) {
                        switch value {
                        case NSAccessibility.Role.popUpButton.rawValue: role = "popup"
                        case NSAccessibility.Role.comboBox.rawValue: role = "combo"
                        case NSAccessibility.Role.menuButton.rawValue: role = "menu-button"
                        case NSAccessibility.Role.group.rawValue: role = "group"
                        default: role = "other"
                        }
                    }
                    roleGetterAvailable = object.responds(to: roleSelector)
                    childrenGetterAvailable = object.responds(to: childrenSelector)
                    pickAvailable = object.responds(to: NSSelectorFromString("accessibilityPerformPick"))
                    pressAvailable = object.responds(to: NSSelectorFromString("accessibilityPerformPress"))
                    showMenuAvailable = object.responds(to: NSSelectorFromString("accessibilityPerformShowMenu"))
                }
            }
            // Inspect NSArray conditionally, without assertion-bearing/typed child helpers.
            let axChildren = childResult.value as? NSArray
            defer { withExtendedLifetime(axChildren) {} }
            if childResult.route == "neither" || (childResult.kind != "nil" && childResult.kind != "array") {
                readFailed = true; incomplete = true
            }
            // Nil children cannot establish a complete child observation, even with a getter.
            if axChildren == nil { incomplete = true }
            let nativeChildren = (object as? NSView)?.subviews ?? []
            guard depth < maxDepth else {
                if !nativeChildren.isEmpty || (axChildren?.count ?? 0) > 0 {
                    depthLimit = true; incomplete = true
                }
                return
            }
            // Bound edge-list inspection too, including duplicate or malformed entries.
            // Any omitted edge keeps the observation incomplete; no absence is inferred.
            if nativeChildren.count > maxObjects { objectLimit = true; incomplete = true }
            for child in nativeChildren.prefix(maxObjects) {
                guard visited.count < maxObjects else { objectLimit = true; incomplete = true; break }
                inspect(child, depth: depth + 1, viaNative: true)
            }
            if let axChildren {
                if axChildren.count > maxObjects { objectLimit = true; incomplete = true }
                for index in 0..<min(axChildren.count, maxObjects) {
                    guard visited.count < maxObjects else { objectLimit = true; incomplete = true; break }
                    guard let child = axChildren.object(at: index) as? NSObject else {
                        readFailed = true; incomplete = true; continue
                    }
                    inspect(child, depth: depth + 1, viaNative: false)
                }
            }
        }
        if mounted { inspect(root, depth: 0, viaNative: false) }
        let lookup = exactServiceCount > 0 ? "found"
            : (readFailed ? "read-failed" : (incomplete ? "incomplete" : "not-found"))
        print("nativeServiceMount controllerAttached=\(controllerAttached) hostAttached=\(hostAttached)"
            + " contentAttached=\(contentAttached) windowVisible=\(window.isVisible)"
            + " windowOcclusionVisible=\(window.occlusionState.contains(.visible))"
            + " hostFrameNonempty=\(!root.frame.isEmpty) hostBoundsNonempty=\(!root.bounds.isEmpty)"
            + " windowFrameNonempty=\(!window.frame.isEmpty)"
            + " nativePickerMatches=LAST-OBSERVED:\(lastObservedNativePickerCount.map { String($0) } ?? "not-observed")"
            + " visited=\(visited.count) visitedRoot=\(visitedRoot) visitedNative=\(visitedNative) visitedAX=\(visitedAX)"
            + " skippedElsewhere=\(skippedElsewhere) objectLimit=\(objectLimit) depthLimit=\(depthLimit)"
            + " readFailed=\(readFailed) incomplete=\(incomplete) axService=\(lookup) exactServiceCount=\(exactServiceCount)")
        print("nativeServiceMount rawPopUpButtons=\(popupCount) matchingTitlePopups=\(matchingTitlePopups)"
            + " menuPresentPopups=\(menuPresentPopups) menuNonemptyPopups=\(menuNonemptyPopups)"
            + " popupsWithICloudOption=\(popupsWithICloudOption) popupsWithMeguriOption=\(popupsWithMeguriOption)"
            + " classNSPopUpButton=\(popupCount) classNSButton=\(buttonCount) classNSControl=\(controlCount)"
            + " classNSView=\(viewCount) classOther=\(otherCount)")
        print("nativeServiceMount rootIdentifierRoute=\(rootIdentifierRoute) rootIdentifierKind=\(rootIdentifierKind)"
            + " rootChildrenRoute=\(rootChildrenRoute) rootChildrenKind=\(rootChildrenKind)"
            + " identifierModern=\(identifiers.modern) identifierLegacy=\(identifiers.legacy) identifierNeither=\(identifiers.neither)"
            + " identifierNil=\(identifiers.nilValue) identifierString=\(identifiers.string)"
            + " identifierArray=\(identifiers.array) identifierOther=\(identifiers.other)"
            + " childrenModern=\(children.modern) childrenLegacy=\(children.legacy) childrenNeither=\(children.neither)"
            + " childrenNil=\(children.nilValue) childrenString=\(children.string)"
            + " childrenArray=\(children.array) childrenOther=\(children.other)")
        if exactServiceCount > 0 {
            print("nativeServiceMount firstExactServiceCategory=\(category) firstExactServiceRole=\(role)"
                + " roleGetterAvailable=\(roleGetterAvailable) childrenGetterAvailable=\(childrenGetterAvailable)"
                + " pickSelectorAvailable=\(pickAvailable) pressSelectorAvailable=\(pressAvailable)"
                + " showMenuSelectorAvailable=\(showMenuAvailable)")
        }
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
