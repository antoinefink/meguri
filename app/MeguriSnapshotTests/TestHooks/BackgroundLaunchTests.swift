#if MEGURI_TEST_HOOKS && DEBUG && os(macOS)
import AppKit
import AppModel
import Testing

@testable import Meguri

/// U8d: the Mac app stays out of the Dock and never becomes active when it
/// runs as a test host or is launched hidden by `tools/mg`.
@MainActor
@Suite struct BackgroundLaunchTests {
    typealias Switch = TestHooks.BackgroundLaunch

    static let dev = "com.antoinefink.meguri.dev"

    /// This process is the test host: Xcode launched it for these tests.
    @Test func theTestHostRunsWithoutTheRegularPolicy() {
        #expect(NSApp.activationPolicy() != .regular)
        #expect(NSRunningApplication.current.activationPolicy != .regular)
        #expect(!NSApp.isActive)
    }

    /// LaunchServices registers every launch of a Debug build as a UI
    /// element, before any app code runs; the app promotes launches by hand.
    @Test func debugBuildsStartAsUIElements() {
        #expect(Bundle.main.object(forInfoDictionaryKey: "LSUIElement") as? Bool == true)
    }

    @Test func aTestHostRunsInTheBackground() {
        let policy = Switch.policy(arguments: ["Meguri"], environment: ["XCTestConfigurationFilePath": "/tmp/x.xctestconfiguration"], bundleIdentifier: Self.dev)
        #expect(policy == .accessory)
    }

    @Test func theLaunchArgumentRunsInTheBackground() {
        let arguments = ["Meguri", "-ApplePersistenceIgnoreState", "YES", "-MeguriBackgroundLaunch", "YES", "-MeguriRoute", "sample/screen"]
        #expect(Switch.policy(arguments: arguments, environment: [:], bundleIdentifier: Self.dev) == .accessory)
    }

    @Test(arguments: [
        ["Meguri"],
        ["Meguri", "-MeguriRoute", "sample/screen"],
        ["Meguri", "-MeguriBackgroundLaunch", "NO"],
        ["Meguri", "-MeguriBackgroundLaunch"],
        ["Meguri", "YES", "-MeguriBackgroundLaunch"],
    ])
    func otherLaunchesKeepTheRegularPolicy(arguments: [String]) {
        #expect(Switch.policy(arguments: arguments, environment: ["HOME": "/Users/x"], bundleIdentifier: Self.dev) == .regular)
    }

    /// The daily driver's id never takes the switch, whatever asks for it.
    @Test func onlyTheDevIdHasTheSwitch() {
        let policy = Switch.policy(
            arguments: ["Meguri", "-MeguriBackgroundLaunch", "YES"],
            environment: ["XCTestConfigurationFilePath": "/tmp/x.xctestconfiguration"],
            bundleIdentifier: "com.antoinefink.meguri"
        )
        #expect(policy == .regular)
    }

    /// The argument changes nothing else: it is not a hook option that
    /// starts from a fresh UI state or resets the window frame.
    @Test func theArgumentIsNotAUIStateOption() throws {
        let options = try TestHooks.LaunchOptions(arguments: ["-MeguriBackgroundLaunch", "YES"])
        #expect(options == TestHooks.LaunchOptions())
        var configuration = LaunchConfiguration()
        let session = TestHooks.prepare(&configuration, arguments: ["Meguri", "-MeguriBackgroundLaunch", "YES"])
        #expect(session.error == nil)
        #expect(!session.startsFresh)
    }

    @Test func aBadValueIsAHookError() {
        #expect(throws: TestHookError("-MeguriBackgroundLaunch takes YES or NO, got 'on'")) {
            try TestHooks.LaunchOptions(arguments: ["-MeguriBackgroundLaunch", "on"])
        }
    }
}
#endif
