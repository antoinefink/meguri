#if os(macOS)
import SwiftUI

/// Window > Meguri (R11): opens the main window again after it was closed
/// while an integration kept the app running, or brings it to the front.
/// The `Window` scene adds no such item on its own.
struct MainWindowCommands: Commands {
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(before: .windowList) {
            Button(String(localized: "Meguri", comment: "Window menu item that shows the main window")) {
                openWindow(id: WindowMetrics.mainWindowID)
            }
        }
    }
}
#endif
