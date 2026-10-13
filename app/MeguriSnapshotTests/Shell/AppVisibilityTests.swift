#if os(macOS)
import AppKit
import Testing

@testable import Meguri

/// `untilUnhidden`, on the host app itself: launch-time alerts wait for it,
/// since an alert presented while the app is hidden makes the app quit. The
/// host is unhidden without activation, so the tests never take the focus.
@MainActor
@Suite(.serialized, .timeLimit(.minutes(1))) struct AppVisibilityTests {
    @MainActor final class Flag {
        var isSet = false
    }

    /// Hides the app; `hide` takes effect asynchronously.
    private func hide() async {
        let hidden = NotificationCenter.default.notifications(named: NSApplication.didHideNotification, object: NSApp)
        NSApp.hide(nil)
        guard !NSApp.isHidden else { return }
        for await _ in hidden {
            return
        }
    }

    @Test func returnsAtOnceWhenTheAppIsVisible() async {
        #expect(!NSApp.isHidden)
        await NSApp.untilUnhidden()
    }

    @Test func waitsWhileTheAppIsHiddenAndReturnsWhenItIsUnhidden() async throws {
        defer { NSApp.unhideWithoutActivation() }
        await hide()
        #expect(NSApp.isHidden)
        let returned = Flag()
        let waiter = Task {
            await NSApp.untilUnhidden()
            returned.isSet = true
        }
        try await Task.sleep(for: .milliseconds(500))
        #expect(!returned.isSet, "returned while the app was hidden")
        NSApp.unhideWithoutActivation()
        await waiter.value
        #expect(returned.isSet)
        #expect(!NSApp.isHidden)
    }
}
#endif
