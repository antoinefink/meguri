#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import Foundation
import SwiftUI
import Testing
@testable import ImageOcclusion
#if os(macOS)
import AppKit
#endif

@testable import Meguri

/// The app shell (U150) in the app's own process: which states its
/// snapshots cover, and on the Mac the window's lifetime policy (R11) and the
/// menu bar, which prevents a second main window and provides native Settings
/// (N1, N5). The snapshots themselves render in `ScreenSnapshots` (`MatrixRegistry` lists
/// `shell.root`).
@MainActor
@Suite struct ShellSnapshotTests {
    @Test func maskHistoryWorksWithoutWorkspaceContextAndRelinquishesFieldFocus() {
        let session = MaskEditorSession(field: "", isEditing: false)
        session.setReferenceSizeIfNeeded(imageSize: .init(width: 400, height: 400),
            area: .init(width: 400, height: 400), displayScale: 1)
        session.addBox(from: .init(x: 10, y: 10), to: .init(x: 80, y: 80))
        session.addBox(from: .init(x: 100, y: 100), to: .init(x: 180, y: 180))
        var focused = true
        let mask = MaskEditorCommandContext(enabled: { command in
            guard focused else { return false }
            switch command {
            case .undo: return session.canUndo
            case .redo: return session.canRedo
            default: return true
            }
        }, action: { command in
            if command == .undo { session.undo() }
            if command == .redo { session.redo() }
        })
        let history = NativeUndoCommands(context: nil, maskEditor: mask)
        #expect(history.replacesUndo && history.enabled(redo: false))
        history.perform(redo: false)
        #expect(session.document.shapes.count == 1 && session.canUndo && session.canRedo)
        #expect(history.enabled(redo: true))
        history.perform(redo: true)
        #expect(session.document.shapes.count == 2 && !session.canRedo)
        // The canvas key consumes one step; the menu shares that dispatch.
        #expect(mask.keyPressed(character: "z", modifiers: .command))
        #expect(session.document.shapes.count == 1 && session.canUndo)
        focused = false
        #expect(!history.replacesUndo)
        #expect(!mask.keyPressed(character: "z", modifiers: .command))
        #expect(!NativeUndoCommands(context: nil, maskEditor: nil).replacesUndo)
    }

    private func shellCells(on platform: SnapshotPlatform) -> [SnapshotCase] {
        MatrixRegistry.screenCases(on: platform).filter { $0.subject == "shell.root" }
    }

    private func rendered(_ cases: [SnapshotCase]) -> [SnapshotCase] {
        cases.filter { $0.cell.deferredReason == nil }
    }

    /// Both layouts in every state: the tab layout on the iPhone, the
    /// sidebar layout on the iPad and on the Mac (its split view drawn
    /// flat), each in light and dark.
    @Test func bothLayoutsAreSnapshottedInEveryState() {
        #expect(ShellRootPreview.previewStates == [.typical, .error, .loading])
        let states: Set<String> = ["typical", "loading", "error"]
        // The iPhone and the Mac render every cell they have.
        for platform in [SnapshotPlatform.iphone, .mac] {
            let cells = shellCells(on: platform)
            #expect(Set(cells.map(\.state)) == states, "\(platform)")
            #expect(Set(cells.map(\.cell.appearance)) == [.light, .dark], "\(platform)")
            #expect(cells.allSatisfy { $0.cell.deferredReason == nil }, "\(platform): \(cells.compactMap(\.cell.deferredReason))")
        }
        #expect(shellCells(on: .iphone).allSatisfy { !$0.cell.device.horizontalSizeClassIsRegular })
    }

    /// The iPad's system sidebar glass is not the same twice
    /// (MatrixRegistry): its light cells with Reduce Transparency on render,
    /// and its dark cells and its Liquid Glass ones stay listed as deferred,
    /// with the reason, rather than missing from the matrix.
    @Test func theIPadRendersItsOpaqueLightSidebarAndListsTheRestAsDeferred() {
        let states: Set<String> = ["typical", "loading", "error"]
        let ipad = shellCells(on: .ipad)
        #expect(ipad.allSatisfy { $0.cell.device.horizontalSizeClassIsRegular })
        let light = rendered(ipad)
        #expect(Set(light.map(\.state)) == states)
        #expect(light.allSatisfy { $0.cell.appearance == .light && $0.cell.reduceTransparency })
        for state in states {
            let mine = ipad.filter { $0.state == state }
            let dark = mine.filter { $0.cell.appearance == .dark }
            #expect(dark.count == 4, "\(state): dark cells \(dark.map(\.cell.name))")
            #expect(dark.allSatisfy { $0.cell.deferredReason != nil }, "\(state)")
            #expect(mine.filter { $0.cell.appearance == .light && !$0.cell.reduceTransparency }.allSatisfy { $0.cell.deferredReason != nil })
            #expect(mine.contains { $0.cell.appearance == .dark && $0.cell.reduceTransparency }, "\(state): no dark sidebar cell")
        }
    }

    #if os(macOS)
    /// Every item of the menu bar, submenus included.
    private func menuItems(_ menu: NSMenu?) -> [NSMenuItem] {
        (menu?.items ?? []).flatMap { [$0] + menuItems($0.submenu) }
    }

    @Test func noMenuOpensASecondMainWindow() throws {
        let items = menuItems(NSApp.mainMenu)
        #expect(!items.isEmpty, "the host app has no menu bar")
        let titles = items.map(\.title)
        #expect(!titles.contains("New Window"), "\(titles)")
        #expect(!titles.contains { $0.hasPrefix("New Meguri Window") }, "\(titles)")
        #expect(!items.contains { $0.keyEquivalent == "n" && $0.keyEquivalentModifierMask == .command }, "⌘N opens something")
        let settings = try #require(items.first { $0.title.hasPrefix("Settings") }, "no native Settings command (N1, N5)")
        #expect(settings.keyEquivalent == "," && settings.keyEquivalentModifierMask == .command, "Settings must use ⌘,")
    }

    /// N21.8: View › Browse takes ⇧⌘B, and nothing else in the menu bar
    /// does (Card › Show in Browse, which shares it while review presents a
    /// card, is U280's and not built yet).
    @Test func viewBrowseOwnsShiftCommandB() throws {
        let view = try #require(NSApp.mainMenu?.items.first { $0.title == "View" }?.submenu, "no View menu")
        let item = try #require(view.items.first { $0.title == "Browse" }, "\(view.items.map(\.title))")
        func isShiftCommandB(_ item: NSMenuItem) -> Bool {
            let mask = item.keyEquivalentModifierMask.intersection([.command, .shift, .option, .control])
            return (item.keyEquivalent == "b" && mask == [.command, .shift])
                || (item.keyEquivalent == "B" && mask.contains(.command) && !mask.contains(.option) && !mask.contains(.control))
        }
        #expect(isShiftCommandB(item), "key \(item.keyEquivalent) mask \(item.keyEquivalentModifierMask.rawValue)")
        let claims = menuItems(NSApp.mainMenu).filter(isShiftCommandB).map(\.title)
        #expect(claims == ["Browse"], "\(claims)")
    }

    /// W14, N22.1: View › Note Types takes ⇧⌘T, and nothing else in the menu
    /// bar does (the system's Show Tab Bar would, for a window that tabs).
    @Test func viewNoteTypesOwnsShiftCommandT() throws {
        let view = try #require(NSApp.mainMenu?.items.first { $0.title == "View" }?.submenu, "no View menu")
        let item = try #require(view.items.first { $0.title == "Note Types" }, "\(view.items.map(\.title))")
        func isShiftCommandT(_ item: NSMenuItem) -> Bool {
            let mask = item.keyEquivalentModifierMask.intersection([.command, .shift, .option, .control])
            return (item.keyEquivalent == "t" && mask == [.command, .shift])
                || (item.keyEquivalent == "T" && mask.contains(.command) && !mask.contains(.option) && !mask.contains(.control))
        }
        #expect(isShiftCommandT(item), "key \(item.keyEquivalent) mask \(item.keyEquivalentModifierMask.rawValue)")
        let claims = menuItems(NSApp.mainMenu).filter(isShiftCommandT).map(\.title)
        #expect(claims == ["Note Types"], "\(claims)")
        let titles = menuItems(NSApp.mainMenu).map(\.title)
        #expect(!titles.contains("Show Tab Bar") && !titles.contains("Show All Tabs"), "\(titles)")
        // The one main window never joins a tab group (R6); fixture windows of other suites are not it.
        let main = NSApp.windows.filter { $0.identifier?.rawValue.hasPrefix(WindowMetrics.mainWindowID) == true }
        #expect(!NSWindow.allowsAutomaticWindowTabbing || (!main.isEmpty && main.allSatisfy { $0.tabbingMode == .disallowed }),
                "\(main.map { "\($0.identifier?.rawValue ?? "-"): \($0.tabbingMode.rawValue)" })")
    }

    @Test func theWindowMenuShowsTheMainWindow() throws {
        let window = try #require(NSApp.mainMenu?.items.first { $0.title == "Window" }?.submenu, "no Window menu")
        #expect(window.items.contains { $0.title == "Meguri" }, "\(window.items.map(\.title))")
    }

    @Test func closingTheWindowQuitsUnlessAnIntegrationIsOn() {
        let saved = AppDelegate.keepsRunningWithoutWindow
        defer { AppDelegate.keepsRunningWithoutWindow = saved }
        let delegate = AppDelegate()
        AppDelegate.keepsRunningWithoutWindow = false
        #expect(delegate.applicationShouldTerminateAfterLastWindowClosed(NSApp))
        AppDelegate.keepsRunningWithoutWindow = true
        #expect(!delegate.applicationShouldTerminateAfterLastWindowClosed(NSApp))
    }

    @Test func reopeningWithoutAVisibleWindowOpensIt() {
        let saved = AppDelegate.openMainWindow
        defer { AppDelegate.openMainWindow = saved }
        var opened = 0
        AppDelegate.openMainWindow = { opened += 1 }
        let delegate = AppDelegate()
        #expect(delegate.applicationShouldHandleReopen(NSApp, hasVisibleWindows: true))
        #expect(opened == 0)
        #expect(!delegate.applicationShouldHandleReopen(NSApp, hasVisibleWindows: false))
        #expect(opened == 1)
    }
    #endif
}
#endif
