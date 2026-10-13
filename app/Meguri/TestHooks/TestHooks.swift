#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import Foundation
import SwiftUI

/// Test-only launch hooks (KTD18, R1963, R1967), compiled into Debug and
/// Profile builds only. `prepare` reads the `-Meguri…` launch arguments once,
/// before the first scene, and overrides the launch configuration through the
/// seams MeguriKit exposes; `Root` applies the overrides that live in the view
/// tree. A bad option replaces the whole UI with the error, so a test or an
/// agent can never mistake a half-applied launch for the requested state.
enum TestHooks {
    struct Session {
        var options = LaunchOptions()
        var error: TestHookError?
    }

    static func prepare(
        _ configuration: inout LaunchConfiguration,
        arguments: [String] = ProcessInfo.processInfo.arguments,
        bundleIdentifier: String = Bundle.main.bundleIdentifier ?? ""
    ) -> Session {
        do throws(TestHookError) {
            let options = try LaunchOptions(arguments: Array(arguments.dropFirst()))
            try options.apply(
                to: &configuration,
                installer: FixtureInstaller(bundleIdentifier: bundleIdentifier),
                routes: .standard
            )
            AppearanceOverrides.applyProcessWide(options)
            return Session(options: options)
        } catch {
            return Session(error: error)
        }
    }

    /// Wraps the app's root view: the hook error, or the root with the
    /// appearance overrides.
    struct Root<Content: View>: View {
        let session: Session
        @ViewBuilder let content: () -> Content

        var body: some View {
            if let error = session.error {
                ErrorView(error: error)
            } else {
                content()
                    .modifier(AppearanceOverrides(options: session.options))
            }
        }
    }

    struct ErrorView: View {
        let error: TestHookError

        var body: some View {
            VStack(spacing: Spacing.small) {
                Text("Test hook error")
                    .font(.headline)
                Text(error.message)
                    .accessibilityIdentifier("testhooks.error.message")
            }
            .padding()
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

/// Why the test hooks refused a launch.
struct TestHookError: Error, Equatable {
    let message: String

    init(_ message: String) {
        self.message = message
    }
}
#endif
