#if os(macOS)
import AppKit

extension NSApplication {
    /// Returns once the app is not hidden: at once, or when it is unhidden.
    ///
    /// An alert presented while the app is hidden (launched with `open -j`, as
    /// agents launch it, or a login item set to Hide) closes its window, and a
    /// `Window`-scene app quits when its last window closes. Launch-time
    /// alerts wait for this.
    func untilUnhidden() async {
        // Created before the check, so an unhide right after it is not missed.
        let unhidden = NotificationCenter.default.notifications(named: NSApplication.didUnhideNotification, object: self)
        guard isHidden else { return }
        for await _ in unhidden {
            return
        }
    }
}
#endif
