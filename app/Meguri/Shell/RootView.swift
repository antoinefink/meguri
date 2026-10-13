import AppModel
import SwiftUI

/// The root of the window: a placeholder Home until U150's adaptive shell,
/// plus whatever the launch configuration presents over it (the initial route
/// seam, KTD18). It injects the launch clock, time zone and locale.
struct RootView: View {
    let configuration: LaunchConfiguration
    @State private var showsScreen: Bool
    @State private var showsSheet: Bool
    @State private var showsAlert: Bool

    init(configuration: LaunchConfiguration) {
        self.configuration = configuration
        let style = configuration.initialPresentation?.style
        _showsScreen = State(initialValue: style == .screen)
        _showsSheet = State(initialValue: style == .sheet)
        #if os(macOS)
        // Presented by `.task` once the app is visible (`untilUnhidden`).
        _showsAlert = State(initialValue: false)
        #else
        _showsAlert = State(initialValue: style == .alert)
        #endif
    }

    var body: some View {
        NavigationStack {
            Text("Meguri")
                .font(.largeTitle)
                .accessibilityIdentifier("home.placeholder.title")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .navigationDestination(isPresented: $showsScreen) {
                    presentedContent
                }
        }
        .sheet(isPresented: $showsSheet) {
            presentedContent
        }
        .alert(configuration.initialPresentation?.alertTitle ?? "", isPresented: $showsAlert) {
            configuration.initialPresentation?.alertActions
        } message: {
            Text(configuration.initialPresentation?.alertMessage ?? "")
        }
        #if os(macOS)
        .task {
            guard configuration.initialPresentation?.style == .alert else { return }
            await NSApplication.shared.untilUnhidden()
            showsAlert = true
        }
        .frame(minWidth: WindowMetrics.minimum.width, minHeight: WindowMetrics.minimum.height)
        #endif
        .modifier(LaunchEnvironment(configuration: configuration))
    }

    @ViewBuilder private var presentedContent: some View {
        if let content = configuration.initialPresentation?.content {
            // A Mac sheet takes its locale from its own window, not from the
            // presenter, so presented content gets the launch values again.
            content
                .modifier(LaunchEnvironment(configuration: configuration))
        }
    }
}

/// The launch clock, time zone and locale in the environment.
private struct LaunchEnvironment: ViewModifier {
    let configuration: LaunchConfiguration

    func body(content: Content) -> some View {
        content
            .environment(\.appClock, configuration.clock)
            .environment(\.timeZone, configuration.timeZone ?? .autoupdatingCurrent)
            .environment(\.locale, configuration.locale ?? .autoupdatingCurrent)
    }
}
