#if MEGURI_TEST_HOOKS && os(macOS)
import AppKit
import Foundation
import Testing

@testable import Meguri

/// The Mac window resize request that the agent driver's resize step posts
/// (U8, R1962): the hook view resizes the window it is in, as the launch
/// option would, and answers. In the app process, on a window never shown;
/// the full step, against the running app, is `AgentDriverTests`
/// (`tools/mg mac-ui`).
@MainActor
@Suite struct WindowResizeHookTests {
    typealias HookView = TestHooks.WindowHookView

    @Test func aRequestResizesTheWindowItIsInAndIsAnswered() async throws {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1180, height: 780), styleMask: [.titled, .resizable],
            backing: .buffered, defer: true
        )
        window.isReleasedWhenClosed = false
        let hook = HookView(size: nil, animationsEnabled: false, isFreshLaunch: false)
        window.contentView?.addSubview(hook)
        defer { hook.removeFromSuperview() }

        let expected: [(String, CGSize)] = [
            ("minimum", CGSize(width: 800, height: 560)),
            ("1000x700", CGSize(width: 1000, height: 700)),
            ("default", CGSize(width: 1180, height: 780)),
        ]
        for (request, size) in expected {
            let answered = try await Self.request(request)
            #expect(window.contentRect(forFrameRect: window.frame).size == size, "\(request)")
            #expect(answered, "no answer to the resize request '\(request)' within 10 s")
        }
    }

    /// Posts a request as the driver does and waits for the answer. It
    /// waits suspended, not in a run loop: Swift Testing runs main-actor
    /// code on the main queue, which delivers the notifications.
    private static func request(_ size: String) async throws -> Bool {
        let answer = Answer()
        let center = DistributedNotificationCenter.default()
        center.addObserver(
            answer, selector: #selector(Answer.received(_:)), name: HookView.resized, object: nil,
            suspensionBehavior: .deliverImmediately
        )
        defer { center.removeObserver(answer) }
        center.postNotificationName(HookView.resizeRequest, object: size, userInfo: nil, deliverImmediately: true)
        let deadline = ContinuousClock.now + .seconds(10)
        while !answer.answered, ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(50))
        }
        return answer.answered
    }

    @MainActor
    private final class Answer: NSObject {
        private(set) var answered = false

        @objc func received(_ notification: Notification) {
            answered = true
        }
    }
}
#endif
