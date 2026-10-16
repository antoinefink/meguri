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
        MatrixRegistry.screenCases(on: platform).filter { $0.subject == "shell.root" && $0.cell.deferredReason == nil }
    }

    /// The tab layout on the iPhone in every state. The sidebar layout's
    /// renders are not deterministic yet (MatrixRegistry, U252).
    @Test func theShellsTabLayoutIsSnapshottedInEveryState() {
        #expect(ShellRootPreview.previewStates == [.typical, .error, .loading])
        let iphone = shellCells(on: .iphone)
        #expect(!iphone.isEmpty && iphone.allSatisfy { !$0.cell.device.horizontalSizeClassIsRegular })
        #expect(Set(iphone.map(\.state)) == ["typical", "loading", "error"])
        #expect(shellCells(on: .ipad).isEmpty && shellCells(on: .mac).isEmpty)
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
