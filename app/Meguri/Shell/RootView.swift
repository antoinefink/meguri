import AppModel
import CoreBridge
import Editor
import Home
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
    @Environment(\.layoutFollowsForcedSizeClass) private var layoutFollowsForcedSizeClass
    #else
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings
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
        // An iPhone keeps the tab layout at any width, unfolded included (R3, N13).
        (UIDevice.current.userInterfaceIdiom == .phone && !layoutFollowsForcedSizeClass) || sizeClass == .compact
            ? .compact : .sidebar
        #else
        .sidebar
        #endif
    }

    #if os(iOS)
    /// The tab layout's list beside the deck: not for the welcome (N18.3), and not
    /// decided while the collection is still opening.
    private var besideDetail: Bool? {
        guard let collection else { return true }
        switch collection.state {
        case .unopened, .loading: return nil
        case .noFile: return false
        case .ready, .unavailable, .creationRecovery, .failed: return true
        }
    }
    #endif

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
                    CompactRootView(navigation: navigation, features: features, configuration: configuration,
                        hasDecks: collection?.decks.isEmpty == false,
                        besideDetail: besideDetail)
                    #endif
                case .sidebar:
                    SidebarRootView(navigation: navigation, features: features, configuration: configuration)
                }
            }
        }
        #if os(macOS)
        .focusedSceneValue(\.nativeCommandContext, collection.map { NativeCommandContext(collection: $0, host: .main) })
        // Hide only the presenting shell; sheets and alerts own their AX tree.
        .accessibilityHidden(launchSheet || launchAlert || linkRefusal != nil || hasModalSheet)
        .background(AddNoteWindowGuard(navigation: navigation))
        // Here, not in the status button: review mounts that button only while sync needs the person (N16.2).
        .onChange(of: collection?.syncPresentation.settingsRequest) { _, request in
            if request != nil, collection?.sync.providerState?.selection == .meguri { openSettings() }
        }
        #endif
        .modifier(PresentationHost(
            navigation: navigation, features: features, configuration: configuration,
            isActive: !(layout == .compact && navigation.review != nil)
        ))
        .environment(\.shellLayout, layout)
        .sheet(isPresented: $launchSheet) {
            launchContent
                #if os(macOS)
                .accessibilityHidden(false)
                #endif
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
            collection?.syncPresentation.showStatusForCommands = layout == .sidebar
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
        .onChange(of: navigation.sheet) { _, sheet in
            updateReviewObscuration()
            collection?.entry.presentationChanged()
        }
        .onChange(of: launchSheet) { _, _ in updateReviewObscuration() }
        .onChange(of: launchAlert) { _, _ in updateReviewObscuration() }
        .onChange(of: linkRefusal != nil) { _, _ in updateReviewObscuration() }
        .onChange(of: navigation.review?.id) { _, _ in collection?.navigationChanged() }
        .onChange(of: layout) { _, layout in collection?.syncPresentation.showStatusForCommands = layout == .sidebar }
        #if os(iOS)
        .onChange(of: sizeClass) { _, _ in
            // The toolbar host itself can be replaced by the layout switch.
            collection?.syncPresentation.cancelStatusHandoff()
            collection?.syncPresentation.statusPresented = false
        }
        #endif
        .onChange(of: collection?.syncPresentation.statusOwnsInput) { _, _ in updateReviewObscuration() }
        .onKeyPress("y", phases: .down) { _ in
            guard let collection, SyncKeyboardOwnership.admits(collection),
                  collection.requestSyncNow(source: .key, showStatus: layout == .sidebar) else { return .ignored }
            return .handled
        }
        .modifier(ReviewLifecycle(collection: collection))
        .modifier(AddNoteRequestFeedback(collection: collection))
        .modifier(StudyRefusalFeedback(collection: collection))
        .modifier(LaunchEnvironment(configuration: configuration))
    }

    private var hasModalSheet: Bool {
        #if os(macOS)
        if features.nativeMacWorkspace, case .add = navigation.sheet { return false }
        #endif
        return navigation.sheet != nil
    }

    private func updateReviewObscuration() {
        collection?.setReviewObscured(navigation.sheet != nil || launchSheet || launchAlert || linkRefusal != nil
            || collection?.syncPresentation.statusOwnsInput == true)
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
