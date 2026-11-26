#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import Foundation
import Stats
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
            if options.statsRollout == .open {
                // After the app's own launch selection (Today), once the shell starts.
                Task { @MainActor in
                    navigation.select(.section(.stats))
                    guard let name = options.statsSection, let section = StatsSectionID(rawValue: name) else { return }
                    // Once every box shows its value (the collection opened and read), as View › Go to
                    // Section would then.
                    for _ in 0..<300 where StatsHooks.shared.page?.filled != true { try? await Task.sleep(for: .milliseconds(100)) }
                    try? await Task.sleep(for: .milliseconds(300))
                    StatsHooks.shared.page?.go(to: section)
                }
            }
            if let render = options.syncRender, let collection {
                SyncRoutes.showRender(render, over: collection, clock: collection.configuration.clock)
            }
            BrowseScrollDriver.shared.collection = { [weak collection] in collection?.browse }
            BrowseScrollDriver.shared.listen()
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
            if options.statsRollout != nil { StatsRollout.enabled = true }
            if options.statsRolloutOff { StatsRollout.enabled = false }
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

        /// These fixtures own their tab shell and navigation stacks. Present them
        /// directly so RootView's screen stack cannot push away the whole shell.
        var standaloneShellContent: AnyView? {
            #if os(iOS)
            guard configuration.initialPresentation?.style == .screen else { return nil }
            switch session.options.route?.path {
            case "navigation/feedback", "navigation/feedback-cold":
                return configuration.initialPresentation?.content
            default:
                return nil
            }
            #else
            return nil
            #endif
        }

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
                    if let standaloneShellContent {
                        standaloneShellContent
                            .modifier(LaunchEnvironment(configuration: configuration))
                    } else if let collectionSheetContent {
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
                    if session.options.homeTrace {
                        HomeTraceProbe(collection: collection)
                    }
                }
                #if os(iOS)
                .background(SizeClassOverride(sizeClass: session.options.sizeClass))
                .environment(\.layoutFollowsForcedSizeClass, session.options.sizeClass != nil)
                #endif
                #if os(macOS)
                .modifier(ReviewInputEvidence(collection: collection,
                    enabled: ["review-editor-1", "EmptyCollection"].contains(session.options.fixture?.lastPathComponent ?? "")))
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

    /// `-MeguriHomeTrace on`: every Home phase shown since launch, in order (N27.1), so a
    /// launch test reads from the hierarchy whether the welcome or "No Decks" ever showed
    /// before the decks. A phase that lasted less than one update is never drawn either.
    struct HomeTraceProbe: View {
        let collection: ReviewCollection?
        @State private var phases: [String] = []

        var body: some View {
            Text(verbatim: phases.joined(separator: ","))
                .font(.caption2)
                .dynamicTypeSize(...DynamicTypeSize.large)
                .foregroundStyle(.token(.textSecondary))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Spacing.medium)
                .padding(.vertical, Spacing.xxSmall)
                .background(.token(.backgroundSecondary))
                .allowsHitTesting(false)
                .accessibilityIdentifier("testhooks.home.phases")
                .onChange(of: name, initial: true) { _, phase in
                    if phases.last != phase { phases.append(phase) }
                }
        }

        private var name: String {
            guard let collection else { return "none" }
            switch collection.homePhase {
            case .welcome: return "welcome"
            case .pending: return "pending"
            case .kept: return collection.homeCountsHeld ? "keptHeld" : "kept"
            case .decks: return "decks"
            case .noDecks: return "noDecks"
            case .unavailable: return "unavailable"
            case .failed: return "failed"
            case .creationRecovery: return "creationRecovery"
            case .readFailed: return "readFailed"
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
