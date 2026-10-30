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
        guard let collection, collection.foreground else { return }
        let lease = Lease(collection: collection)
        lease.identifier = UIApplication.shared.beginBackgroundTask(withName: "Review and iCloud flush") {
            Task { @MainActor in lease.expire() }
        }
        let remaining = UIApplication.shared.backgroundTimeRemaining
        let deadline: Int64?
        if lease.identifier != .invalid, remaining.isFinite, remaining > 0,
           remaining < Double(Int64.max / 1_000) - Date().timeIntervalSince1970 {
            deadline = Int64((Date().timeIntervalSince1970 + remaining) * 1_000)
        } else { deadline = nil }
        lease.token = collection.beginBackgroundTask(deadlineMilliseconds: deadline)
        Task {
            await collection.waitForLifecycle()
            lease.finish()
        }
    }
    @MainActor private final class Lease {
        weak var collection: ReviewCollection?
        var identifier: UIBackgroundTaskIdentifier = .invalid
        var token: UInt64?
        init(collection: ReviewCollection) { self.collection = collection }
        func expire() {
            finish()
            if let token { collection?.backgroundTaskExpired(token) }
        }
        func finish() {
            guard identifier != .invalid else { return }
            let original = identifier; identifier = .invalid
            UIApplication.shared.endBackgroundTask(original)
        }
    }
}
#endif
