import Foundation
import XCTest
#if os(macOS)
import AppKit
import CoreGraphics
#endif

extension XCUIApplication {
    /// On the Mac, makes the app the active app, with its window in front,
    /// before a click, key press or drag is synthesized. Does nothing on iOS.
    ///
    /// Another app (the terminal running the tests) is usually in front on
    /// the Mac that runs them. XCUITest then reports its windows as
    /// interruptions, activates the app and clicks at once, before the
    /// window is key, where AppKit may take the click for activation only.
    /// So this activates the app and waits until it is active and no other
    /// app's window covers its window, then lets the window settle into key
    /// state. Throws when that does not happen within `timeout`.
    @MainActor
    func bringToFront(bundleIdentifier: String, timeout: TimeInterval = 10) throws {
        #if os(macOS)
        let deadline = Date().addingTimeInterval(timeout)
        let frame = windows.firstMatch.frame
        var activated = Date.distantPast
        while true {
            // Again every 2 s, in case another app took the front meanwhile.
            if Date().timeIntervalSince(activated) >= 2 {
                activate()
                activated = Date()
            }
            let running = NSRunningApplication.runningApplications(withBundleIdentifier: bundleIdentifier)
                .first { !$0.isTerminated }
            if let running, running.isActive, Self.windowIsInFront(pid: running.processIdentifier, frame: frame) {
                break
            }
            guard Date() < deadline else {
                let front = NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "none"
                throw FrontmostError(
                    message: "\(bundleIdentifier) did not become the active app with its window in front within \(timeout) s (frontmost: \(front))"
                )
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        // Active, in front: AppKit makes the window key on its next turns.
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        #endif
    }

    #if os(macOS)
    /// Whether, front to back, a normal window of `pid` comes before any
    /// other app's normal window over `frame` (screen points, top-left
    /// origin, as XCUITest and the window list give them).
    private static func windowIsInFront(pid: pid_t, frame: CGRect) -> Bool {
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
            as? [[String: Any]]
        else { return false }
        for window in list where (window[kCGWindowLayer as String] as? Int) == 0 {
            let owner = window[kCGWindowOwnerPID as String] as? pid_t
            if owner == pid { return true }
            guard let bounds = window[kCGWindowBounds as String] as? NSDictionary,
                  let rect = CGRect(dictionaryRepresentation: bounds)
            else { continue }
            if rect.intersects(frame) { return false }
        }
        return false
    }
    #endif
}

struct FrontmostError: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}
