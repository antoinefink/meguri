import AppModel
import SwiftUI

/// The root of the window (KTD10): the tab layout at compact width, the
/// sidebar layout at regular width and always on the Mac (R2–R4), both over
/// one `AppNavigation`, so a switch between them keeps everything (R5). It
/// presents sheets over whichever is shown (R25), opens `meguri://` links
/// (R319–R321), keeps the device's UI state for the next launch (R12, R13,
/// R20), and injects the launch clock, time zone and locale. A test-hook
/// route's sheet or alert shows over it at launch, a route's screen in its
/// place (KTD18).
struct RootView: View {
    let configuration: LaunchConfiguration
    let navigation: AppNavigation
    let features: ShellFeatures
    let uiStateStore: UIStateStore
    let collection: ReviewCollection?
    @Environment(\.scenePhase) private var scenePhase
    @State private var launchSheet: Bool
    @State private var launchAlert: Bool
    @State private var linkRefusal: DeepLinkRefusal?
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #else
    @Environment(\.openWindow) private var openWindow
    #endif

    init(configuration: LaunchConfiguration, navigation: AppNavigation, features: ShellFeatures, uiStateStore: UIStateStore, collection: ReviewCollection? = nil) {
        self.collection = collection
        self.configuration = configuration
        self.navigation = navigation
        self.features = features
        self.uiStateStore = uiStateStore
        let style = configuration.initialPresentation?.style
        _launchSheet = State(initialValue: style == .sheet)
        #if os(macOS)
        // Presented by `.task` once the app is visible (`untilUnhidden`).
        _launchAlert = State(initialValue: false)
        #else
        _launchAlert = State(initialValue: style == .alert)
        #endif
    }

    private var layout: ShellLayout {
        #if os(iOS)
        sizeClass == .compact ? .compact : .sidebar
        #else
        .sidebar
        #endif
    }

    var body: some View {
        Group {
            if let launchScreen {
                // A test-hook route's screen, on its own: it is not one of
                // the shell's.
                NavigationStack { launchScreen }
            } else {
                switch layout {
                case .compact:
                    #if os(iOS)
                    CompactRootView(navigation: navigation, features: features, configuration: configuration)
                    #endif
                case .sidebar:
                    SidebarRootView(navigation: navigation, features: features, configuration: configuration)
                }
            }
        }
        .modifier(PresentationHost(
            navigation: navigation, features: features, configuration: configuration,
            isActive: !(layout == .compact && navigation.review != nil)
        ))
        .sheet(isPresented: $launchSheet) {
            launchContent
        }
        .alert(configuration.initialPresentation?.alertTitle ?? "", isPresented: $launchAlert) {
            configuration.initialPresentation?.alertActions
        } message: {
            Text(configuration.initialPresentation?.alertMessage ?? "")
        }
        .alert(Text("Can’t Open Link"), isPresented: Binding(get: { linkRefusal != nil }, set: { if !$0 { linkRefusal = nil } })) {
            Button("OK") {}
        } message: {
            linkRefusal.map(Self.message)
        }
        .onOpenURL { url in
            Task { await open(url) }
        }
        .onAppear {
            navigation.adoptLaunchLayout(layout)
            collection?.setSceneActive(scenePhase == .active)
            updateReviewObscuration()
            #if os(macOS)
            collection?.setApplicationActive(NSApp.isActive && !NSApp.isHidden)
            #endif
            #if os(macOS)
            AppDelegate.openMainWindow = { [openWindow] in openWindow(id: WindowMetrics.mainWindowID) }
            #endif
        }
        .onChange(of: navigation.uiState) { _, state in
            uiStateStore.save(state)
        }
        #if os(macOS)
        .task {
            guard configuration.initialPresentation?.style == .alert else { return }
            await NSApplication.shared.untilUnhidden()
            launchAlert = true
        }
        .frame(minWidth: WindowMetrics.minimum.width, minHeight: WindowMetrics.minimum.height)
        #endif
        .task { await collection?.start() }
        .onChange(of: scenePhase) { _, phase in
            collection?.setSceneActive(phase == .active)
            if phase == .active { collection?.setForeground(true) }
            else if phase == .background {
                #if os(iOS)
                ReviewBackgroundLease.begin(collection)
                #else
                collection?.setForeground(false)
                #endif
            }
        }
        .onChange(of: navigation.sheet) { _, _ in
            updateReviewObscuration()
            collection?.entry.presentationChanged()
        }
        .onChange(of: launchSheet) { _, _ in updateReviewObscuration() }
        .onChange(of: launchAlert) { _, _ in updateReviewObscuration() }
        .onChange(of: linkRefusal != nil) { _, _ in updateReviewObscuration() }
        .onChange(of: navigation.review?.id) { _, _ in collection?.navigationChanged() }
        .modifier(ReviewLifecycle(collection: collection))
        .modifier(LaunchEnvironment(configuration: configuration))
    }

    private func updateReviewObscuration() {
        collection?.setReviewObscured(navigation.sheet != nil || launchSheet || launchAlert || linkRefusal != nil)
    }

    /// A launch route's screen, shown in place of the shell.
    private var launchScreen: AnyView? {
        configuration.initialPresentation?.style == .screen ? configuration.initialPresentation?.content : nil
    }

    @ViewBuilder private var launchContent: some View {
        if let content = configuration.initialPresentation?.content {
            // A Mac sheet takes its locale from its own window, not from the
            // presenter, so presented content gets the launch values again.
            content
                .modifier(LaunchEnvironment(configuration: configuration))
        }
    }

    /// Opens a `meguri://` link; a refused one is explained in a short alert
    /// (U153 turns it into a status message).
    private func open(_ url: URL) async {
        guard case let .refused(refusal) = await DeepLinkRouter.open(url, in: navigation, deckExists: features.deckExists) else { return }
        #if os(macOS)
        // An alert while the app is hidden closes its window (app/AGENTS.md).
        await NSApplication.shared.untilUnhidden()
        #endif
        linkRefusal = refusal
    }

    private static func message(_ refusal: DeepLinkRefusal) -> Text {
        switch refusal {
        case .unknownLink: Text("Meguri doesn’t know this link.")
        case let .missingDeck(deck): Text("No deck has the ID \(String(deck.rawValue)).")
        }
    }
}

/// The launch clock, time zone and locale in the environment.
struct LaunchEnvironment: ViewModifier {
    let configuration: LaunchConfiguration

    func body(content: Content) -> some View {
        content
            .environment(\.appClock, configuration.clock)
            .environment(\.timeZone, configuration.timeZone ?? .autoupdatingCurrent)
            .environment(\.locale, configuration.locale ?? .autoupdatingCurrent)
    }
}
