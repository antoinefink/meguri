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
        var setupOnly = false
        var options = LaunchOptions()
        var error: TestHookError?
        /// A shell route's state and features.
        var shell: ShellLaunch?
        var ownedProvider: OwnedProviderFixture?

        /// A launch with any hook option starts from a fresh UI state and
        /// leaves none behind, so a run never depends on the previous one;
        /// a launch with none restores the device's as usual.
        var startsFresh: Bool { setupOnly || error != nil || options != LaunchOptions() }

        /// Puts the navigation in the shell route's state and starts the
        /// run-time commands (`ShellCommands`).
        func start(_ navigation: AppNavigation, collection: ReviewCollection? = nil) {
            #if os(macOS)
            if let integrations = options.integrations {
                AppDelegate.keepsRunningWithoutWindow = integrations
            }
            #endif
            guard !setupOnly, error == nil else { return }
            #if os(macOS)
            if let ownedProvider {
                ownedProvider.transport.bind(to: collection, owned: ownedProvider)
            }
            #endif
            shell?.prepare(navigation)
            ShellCommands.shared.listen(navigation: navigation, sizeClass: options.sizeClass,
                nativeCollection: options.nativeReview == nil ? nil : collection)
        }
    }

    static func prepare(
        _ configuration: inout LaunchConfiguration,
        arguments: [String] = ProcessInfo.processInfo.arguments,
        bundleIdentifier: String = Bundle.main.bundleIdentifier ?? ""
    ) -> Session {
        // Environment admission precedes every setup-only/agent early return.
        let environment = ProcessInfo.processInfo.environment
        let transportRequested = environment["MEGURI_OWNED_PROVIDER_TRANSPORT_RUN_ID"] != nil
            || environment["MEGURI_OWNED_PROVIDER_TRANSPORT_MANIFEST_SHA256"] != nil
        var admittedOptions: LaunchOptions?
        if transportRequested || arguments.contains("-MeguriOwnedProviderFixture") {
            do throws(TestHookError) {
                admittedOptions = try LaunchOptions(arguments: Array(arguments.dropFirst()),
                    environment: environment, bundleIdentifier: bundleIdentifier)
            } catch {
                if transportRequested { return Session(error: TestHookError("Owned provider transport refused")) }
                return Session(error: error)
            }
        }
        #if os(macOS)
        var transportRequest: OwnedProviderTransport.Request?
        if transportRequested {
            do {
                transportRequest = try OwnedProviderTransport.Request.admit(environment: environment,
                    ownedMode: admittedOptions?.ownedProviderFixture == true && bundleIdentifier.hasSuffix(".dev"))
            } catch { return Session(error: OwnedProviderTransport.refusal) }
        }
        #else
        if transportRequested { return Session(error: TestHookError("Owned provider transport refused")) }
        #endif
        // Reject setup-only environments before they can exclude normal startup.
        if arguments.contains("-MeguriOwnedProviderFixture") {
            do throws(TestHookError) {
                if admittedOptions?.ownedProviderFixture == true {
                    guard ["MEGURI_CLOUD_AGENT_PATH_PREFLIGHT", "MEGURI_CLOUD_ACCOUNT_SETUP",
                        "MEGURI_CLOUD_AGENT_RUN_DIRECTORY"].allSatisfy({ environment[$0] == nil }) else {
                        throw TestHookError("OwnedProviderFixture is incompatible with setup-only or agent startup")
                    }
                }
            } catch { return Session(error: error) }
        }
        #if DEBUG
        if CloudAgentPreflight.requested() {
            CloudAgentPreflight.excludeNormalStartup()
            return Session(setupOnly: true)
        }
        #endif
        #if DEBUG && os(macOS)
        if CloudAccountSetupStartup.requested() {
            CloudAccountSetupStartup.excludeNormalStartup()
            return Session(setupOnly: true)
        }
        #endif
        // Performance tests ask for a warmed web content process (R1866).
        WebContentWarmup.shared.listen()
        do throws(TestHookError) {
            let options: LaunchOptions
            if let admittedOptions { options = admittedOptions }
            else {
                options = try LaunchOptions(arguments: Array(arguments.dropFirst()),
                    environment: environment, bundleIdentifier: bundleIdentifier)
            }
            let ownedProvider = try options.prepareOwnedProviderFixture(bundleIdentifier: bundleIdentifier)
            #if os(macOS)
            ownedProvider?.transport.admit(transportRequest)
            #endif
            let shell = try options.apply(
                to: &configuration,
                installer: FixtureInstaller(bundleIdentifier: bundleIdentifier),
                routes: .standard,
                ownedProvider: ownedProvider
            )
            AppearanceOverrides.applyProcessWide(options)
            return Session(options: options, shell: shell, ownedProvider: ownedProvider)
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
        var collection: ReviewCollection? = nil
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
            if session.setupOnly {
                EmptyView()
            } else if let error = session.error {
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
                    if session.options.studyTimings {
                        StudyTimingsProbe(collection: collection)
                    }
                }
                #if os(iOS)
                .background(SizeClassOverride(sizeClass: session.options.sizeClass))
                .environment(\.layoutFollowsForcedSizeClass, session.options.sizeClass != nil)
                #endif
                .modifier(AppearanceOverrides(options: session.options))
            }
        }
    }

    /// `-MeguriStudyTimings on`: the last Study's way to its first card, as
    /// `StudyStartTrace` measured it (R1813), for drive scripts to read.
    struct StudyTimingsProbe: View {
        let collection: ReviewCollection?

        var body: some View {
            Text(verbatim: collection?.lastStudyStart?.summary ?? "none")
                .font(.caption2)
                .dynamicTypeSize(...DynamicTypeSize.large)
                .fixedSize(horizontal: false, vertical: true)
                .foregroundStyle(.token(.textSecondary))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Spacing.medium)
                .padding(.vertical, Spacing.xxSmall)
                .background(.token(.backgroundSecondary))
                .allowsHitTesting(false)
                .accessibilityIdentifier("testhooks.studyStart.timings")
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
