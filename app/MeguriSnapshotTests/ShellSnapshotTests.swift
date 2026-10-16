#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import Foundation
import SwiftUI
import Testing
#if os(macOS)
import AppKit
#endif

@testable import Meguri

/// The app shell (U150) in the app's own process: which states its
/// snapshots cover, and on the Mac the window's lifetime policy (R11) and the
/// menu bar, where nothing opens a second window (R6, R110). The snapshots
/// themselves render in `ScreenSnapshots` (`MatrixRegistry` lists
/// `shell.root`).
@MainActor
@Suite struct ShellSnapshotTests {
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

    @Test func noMenuOpensASecondWindow() throws {
        let items = menuItems(NSApp.mainMenu)
        #expect(!items.isEmpty, "the host app has no menu bar")
        let titles = items.map(\.title)
        #expect(!titles.contains("New Window"), "\(titles)")
        #expect(!titles.contains { $0.hasPrefix("New Meguri Window") }, "\(titles)")
        #expect(!items.contains { $0.keyEquivalent == "n" && $0.keyEquivalentModifierMask == .command }, "⌘N opens something")
        #expect(!titles.contains { $0.hasPrefix("Settings") }, "a Settings item opens a Settings window (R110)")
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
