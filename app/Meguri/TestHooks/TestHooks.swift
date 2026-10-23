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
        /// A shell route's state and features.
        var shell: ShellLaunch?

        /// A launch with any hook option starts from a fresh UI state and
        /// leaves none behind, so a run never depends on the previous one;
        /// a launch with none restores the device's as usual.
        var startsFresh: Bool { error != nil || options != LaunchOptions() }

        /// Puts the navigation in the shell route's state and starts the
        /// run-time commands (`ShellCommands`).
        func start(_ navigation: AppNavigation) {
            #if os(macOS)
            if let integrations = options.integrations {
                AppDelegate.keepsRunningWithoutWindow = integrations
            }
            #endif
            guard error == nil else { return }
            shell?.prepare(navigation)
            ShellCommands.shared.listen(navigation: navigation, sizeClass: options.sizeClass)
        }
    }

    static func prepare(
        _ configuration: inout LaunchConfiguration,
        arguments: [String] = ProcessInfo.processInfo.arguments,
        bundleIdentifier: String = Bundle.main.bundleIdentifier ?? ""
    ) -> Session {
        // Performance tests ask for a warmed web content process (R1866).
        WebContentWarmup.shared.listen()
        do throws(TestHookError) {
            let options = try LaunchOptions(arguments: Array(arguments.dropFirst()))
            let shell = try options.apply(
                to: &configuration,
                installer: FixtureInstaller(bundleIdentifier: bundleIdentifier),
                routes: .standard
            )
            AppearanceOverrides.applyProcessWide(options)
            return Session(options: options, shell: shell)
        } catch {
            return Session(error: error)
        }
    }

    /// Wraps the app's root view: the hook error, or the root with the
    /// appearance overrides, the forced size class and, for a shell route,
    /// the navigation state probe.
    struct Root<Content: View>: View {
        let session: Session
        let configuration: LaunchConfiguration
        let navigation: AppNavigation
        @ViewBuilder let content: () -> Content
        @State private var collectionSheetPresented = true

        private var collectionSheetContent: AnyView? {
            guard configuration.initialPresentation?.style == .sheet else { return nil }
            switch session.options.route?.path {
            case "collection/import/empty", "collection/import/typical", "collection/import/overflow",
                 "collection/import/error", "collection/import/loading",
                 "settings/import-export/typical", "settings/import-export/error", "settings/import-export/loading":
                return configuration.initialPresentation?.content
            default:
                return nil
            }
        }

        var body: some View {
            if let error = session.error {
                ErrorView(error: error)
            } else {
                // The probe takes a strip below the shell rather than a
                // place over it: it covers no text that a test, an agent's
                // hierarchy check or a screenshot needs, nothing scrolls
                // under it, and the iPhone's overlaid sidebar leaves it be.
                VStack(spacing: 0) {
                    if let collectionSheetContent {
                        // Audit the prepared collection sheet without unrelated,
                        // dimmed shell labels behind its native presentation.
                        ColorToken.backgroundPrimary.color
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            .ignoresSafeArea()
                            #if os(macOS)
                            .frame(minWidth: WindowMetrics.minimum.width, minHeight: WindowMetrics.minimum.height)
                            #endif
                            .sheet(isPresented: $collectionSheetPresented) {
                                collectionSheetContent
                                    .modifier(LaunchEnvironment(configuration: configuration))
                            }
                            .modifier(LaunchEnvironment(configuration: configuration))
                    } else {
                        content()
                    }
                    if session.shell?.probe == true {
                        ShellStateProbe(navigation: navigation)
                    }
                }
                #if os(iOS)
                .background(SizeClassOverride(sizeClass: session.options.sizeClass))
                #endif
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
