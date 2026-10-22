import AppModel
import SwiftUI
#if os(macOS)
import AppKit
import CardRendering
#else
import UIKit
#endif

/// Platform signals supplement the scene's background/foreground ordering.
struct ReviewLifecycle: ViewModifier {
    let collection: ReviewCollection?
    func body(content: Content) -> some View {
        content
            .onReceive(NotificationCenter.default.publisher(for: .NSSystemTimeZoneDidChange)) { _ in collection?.systemTimeChanged() }
            #if os(macOS)
            .onReceive(NotificationCenter.default.publisher(for: .NSSystemClockDidChange)) { _ in collection?.systemTimeChanged() }
            .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.didWakeNotification)) { _ in collection?.systemTimeChanged() }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.willResignActiveNotification)) { _ in collection?.setApplicationActive(false) }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in collection?.setApplicationActive(!NSApp.isHidden) }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didHideNotification)) { _ in collection?.setApplicationActive(false) }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didUnhideNotification)) { _ in collection?.setApplicationActive(NSApp.isActive && !NSApp.isHidden) }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didMiniaturizeNotification)) { note in
                if isReviewWindow(note) { collection?.setReviewWindowVisible(false) }
            }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didDeminiaturizeNotification)) { note in
                if isReviewWindow(note) { collection?.setReviewWindowVisible(true) }
            }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didChangeOcclusionStateNotification)) { note in
                if isReviewWindow(note), let window = note.object as? NSWindow {
                    collection?.setReviewWindowVisible(window.occlusionState.contains(.visible))
                }
            }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didResignKeyNotification)) { note in
                if isReviewWindow(note) { collection?.setReviewWindowActive(false) }
            }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { note in
                if isReviewWindow(note) { collection?.setReviewWindowActive(true) }
            }
            #else
            .onReceive(NotificationCenter.default.publisher(for: UIApplication.significantTimeChangeNotification)) { _ in collection?.systemTimeChanged() }
            #endif
    }
    #if os(macOS)
    private func isReviewWindow(_ note: Notification) -> Bool {
        guard let window = note.object as? NSWindow,
              let reviewWindow = collection?.review?.pair?.container.window else { return false }
        return window === reviewWindow
    }
    #endif
}

#if os(iOS)
/// Keep the local flush alive while iOS transitions to background execution.
enum ReviewBackgroundLease {
    static func begin(_ collection: ReviewCollection?) {
        guard let collection else { return }
        let lease = UIApplication.shared.beginBackgroundTask(withName: "Review local flush", expirationHandler: nil)
        collection.setForeground(false)
        Task {
            await collection.waitForLifecycle()
            if lease != .invalid { UIApplication.shared.endBackgroundTask(lease) }
        }
    }
}
#endif
