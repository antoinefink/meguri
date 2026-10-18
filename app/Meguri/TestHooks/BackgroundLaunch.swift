#if MEGURI_TEST_HOOKS && DEBUG && os(macOS)
import AppKit
import Foundation

extension TestHooks {
    /// Keeps a Debug build out of the Dock, and from ever becoming the active
    /// app, when it runs on Antoine's Mac without anyone looking at it (U8d):
    /// as the test host of the app-hosted tests (Xcode sets
    /// `XCTestConfigurationFilePath` in its environment) and when a
    /// `tools/mg` job launches it hidden with `-MeguriBackgroundLaunch YES`.
    ///
    /// A policy set at run time comes too late: LaunchServices registers a
    /// launch from the Info.plist, so a regular app is reported `Foreground`
    /// (and gets a Dock tile) before any of its code runs. Debug builds
    /// therefore declare `LSUIElement` (`MEGURI_MAC_UI_ELEMENT`), and
    /// `MeguriApp.init` decides once, before the app finishes launching: a
    /// launch by hand, or an XCUIApplication launch of the Mac UI suites
    /// (which pass no argument), takes the regular policy; the others stay
    /// accessory, which keeps windows (off-screen snapshots render, and
    /// `screencapture -l` captures a hidden launch's window). Profile starts
    /// as a regular app and has no switch: it keeps Release's launch for
    /// performance runs (R1866).
    enum BackgroundLaunch {
        static let argument = "-MeguriBackgroundLaunch"
        static let testHostVariable = "XCTestConfigurationFilePath"

        /// The policy for this launch. Only the `.dev` id has the switch, as
        /// `FixtureInstaller` checks it.
        static func policy(arguments: [String], environment: [String: String], bundleIdentifier: String) -> NSApplication.ActivationPolicy {
            guard bundleIdentifier.hasSuffix(".dev") else { return .regular }
            let isTestHost = environment[testHostVariable] != nil
            let isRequested = arguments.firstIndex(of: argument).map { index in
                let value = arguments.index(after: index)
                return value < arguments.endIndex && arguments[value] == "YES"
            } ?? false
            return isTestHost || isRequested ? .accessory : .regular
        }

        /// Sets this process's policy; called before the first scene.
        static func apply(
            arguments: [String] = ProcessInfo.processInfo.arguments,
            environment: [String: String] = ProcessInfo.processInfo.environment,
            bundleIdentifier: String = Bundle.main.bundleIdentifier ?? ""
        ) {
            NSApplication.shared.setActivationPolicy(policy(arguments: arguments, environment: environment, bundleIdentifier: bundleIdentifier))
        }
    }
}
#endif
