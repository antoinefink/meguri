import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(iOS)
import UIKit
#endif

/// The same quiet control is injected into the platform toolbar and review.
public struct CloudSyncStatusButton: View {
    let collection: ReviewCollection
    @Environment(\.appClock) private var clock
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var nativeCompactStatusID: UUID?
    #endif
    #if os(macOS)
    @Environment(\.openSettings) private var openSettings
    #endif
    #if os(iOS)
    /// The panel's height in each presentation, whose widths differ: it sizes the sheet.
    private var popoverHeight: CGFloat? { collection.syncPresentation.panelPopoverHeight.map { CGFloat($0) } }
    private var sheetHeight: CGFloat? { collection.syncPresentation.panelSheetHeight.map { CGFloat($0) } }
    private func height(_ kind: SheetMetrics.Kind) -> Binding<CGFloat?> {
        let presentation = collection.syncPresentation
        return Binding(get: {
            (kind == .sheet ? presentation.panelSheetHeight : presentation.panelPopoverHeight).map { CGFloat($0) }
        }, set: { value in
            let height = value.map { Double($0) }
            if kind == .sheet { presentation.panelSheetHeight = height } else { presentation.panelPopoverHeight = height }
        })
    }
    #endif
    public init(collection: ReviewCollection) { self.collection = collection }
    /// N15.4: the needs-attention symbol only for needs you; otherwise the state underneath.
    private var main: SyncMainStatus { collection.sync.mainStatus }
    private var titleKey: String { main.titleKey }
    private var presented: Binding<Bool> {
        Binding(get: { collection.navigation.sheet == nil && collection.syncPresentation.statusPresented },
            set: { collection.syncPresentation.statusPresented = $0 })
    }
    private var compact: Bool {
        #if os(iOS)
        sizeClass == .compact
        #else
        false
        #endif
    }
    private func presented(compact: Bool) -> Binding<Bool> {
        Binding(get: { self.compact == compact && presented.wrappedValue },
            set: { if self.compact == compact { presented.wrappedValue = $0 } })
    }
    public var body: some View {
        // N23.6: one plain toolbar symbol, like its neighbours. A tap syncs at once
        // in the ordinary states and opens the panel only where a sync cannot
        // resolve the state; a long press (a secondary click on the Mac) shows
        // the status line, Sync Now and Sync Settings. No text beside it and
        // no view around it, so the system draws it as one item of its group.
        Menu {
            Section(statusLine) {
                Button(SyncText.value("sync.now.command")) { syncNowFromMenu() }
                    .disabled(!syncNowEnabled)
                    .accessibilityIdentifier("sync.status.menu.syncNow")
                Button(SyncText.value("sync.status.openSettings")) { openSettingsFromMenu() }
                    .accessibilityIdentifier("sync.status.menu.settings")
            }
        } label: {
            // N23.2: the moving symbol turns exactly while the panel's bar shows.
            SyncStatusSymbol(status: main, iCloud: collection.sync.symbolsAreICloud,
                turning: !reduceMotion && collection.sync.panelProgress != nil,
                accented: feedback.accented || collection.syncPresentation.buttonAccentHeld)
        } primaryAction: {
            tap()
        }
        .menuIndicator(.hidden)
        .accessibilityLabel(SyncText.value("sync.button.accessibilityLabel"))
        .accessibilityValue(SyncText.value(titleKey).replacingOccurrences(of: "…", with: ""))
        .accessibilityHint(hint)
        .help([SyncText.value(titleKey), SyncStatusCopy.detail(collection.sync, now: clock.now())]
            .compactMap { $0 }.joined(separator: "\n"))
        .accessibilityIdentifier("sync.status.button")
        .disabled(collection.syncPresentation.statusHandoffPending)
        .popover(isPresented: presented(compact: false)) {
            // N23.1: 320 points wide, as tall as its content.
            #if os(iOS)
            // Where the popover becomes a sheet (an iPhone held wide, the Duo
            // opened), the sheet fits the panel and scrolls past the screen's
            // height, as at compact width; as a popover it is as tall as the panel.
            FittedPanelScroll(height: height(.popover), learns: .popover) {
                panel.padding().frame(width: ContentWidth.primaryAction)
            }
            .frame(width: ContentWidth.primaryAction)
            .frame(idealHeight: popoverHeight)
            .presentationCompactAdaptation(.sheet)
            .presentationDetents(Self.fittedDetents(popoverHeight))
            #else
            panel
                .padding()
                .frame(width: ContentWidth.primaryAction)
            #endif
        }
        #if os(iOS)
        // Keep status and connection separate; native dismissal, rather
        // than a delay or view disappearance, orders the compact handoff.
        .sheet(isPresented: presented(compact: true), onDismiss: {
            guard let id = nativeCompactStatusID else { return }
            nativeCompactStatusID = nil
            collection.compactSyncStatusDidDismiss(id)
        }) {
            // The same panel as the popover, in a sheet as tall as the panel
            // (N23.1): the detent grows with the text and stops at the screen's
            // height, where the panel scrolls.
            FittedPanelScroll(height: height(.sheet), learns: .sheet) { panel.padding() }
                .presentationDetents(Self.fittedDetents(sheetHeight))
                .onAppear {
                    if nativeCompactStatusID == nil {
                        nativeCompactStatusID = collection.syncPresentation.statusPresentationID
                    }
                }
        }
        .onChange(of: sizeClass) { _, _ in
            collection.syncPresentation.cancelStatusHandoff()
            collection.syncPresentation.statusPresented = false
        }
        #endif
        .onChange(of: observed) { _, now in observe(now) }
        // The check keeps the accent colour for a moment after a success, then
        // returns to the default colour.
        .task(id: feedback.accented) {
            guard feedback.accented else { return }
            try? await Task.sleep(for: SyncButton.accentDuration)
            guard !Task.isCancelled else { return }
            collection.syncPresentation.buttonFeedback.endAccent()
        }
    }
    private var feedback: SyncButton.Feedback { collection.syncPresentation.buttonFeedback }
    private var moving: Bool { collection.sync.panelProgress != nil }
    private var syncNowAvailable: Bool {
        SyncNowButton.available(collection) && collection.navigation.sheet == nil
    }
    /// The panel's Sync Now rule (N23.3): disabled while the status moves.
    private var syncNowEnabled: Bool { SyncPanel.syncNowEnabled(available: syncNowAvailable, moving: moving) }
    private var tapDecision: SyncButton.Tap {
        SyncButton.tap(main, serviceSelected: collection.sync.panelServiceSelected,
            slot: SyncStatusPanel.slot(collection), syncNowAvailable: syncNowAvailable, moving: moving)
    }
    private var hint: String {
        switch tapDecision {
        case .syncNow: SyncText.value("sync.button.hint.syncNow")
        case .openPanel: SyncText.value("sync.button.hint.status")
        case .nothing: ""
        }
    }
    /// The status line at the top of the long-press menu: the title and its sentence.
    private var statusLine: String {
        [SyncText.value(titleKey), SyncStatusCopy.detail(collection.sync, now: clock.now())]
            .compactMap { $0 }.joined(separator: "\n")
    }
    private func tap() {
        switch tapDecision {
        case .syncNow: startSync()
        case .openPanel: presented.wrappedValue.toggle()
        case .nothing: break
        }
    }
    private func syncNowFromMenu() {
        guard syncNowEnabled else { return }
        startSync()
    }
    private func startSync() {
        guard collection.requestSyncNow(source: .command) else { return }
        collection.syncPresentation.buttonFeedback.requested(at: clock.now())
    }
    /// What the feedback follows: whether the status moves, and how it ends.
    private struct Observed: Equatable { var moving, upToDate, needsYou: Bool }
    private var observed: Observed {
        Observed(moving: moving, upToDate: main.symbol == SyncSymbol.upToDate, needsYou: main.needsYou)
    }
    private func observe(_ now: Observed) {
        let outcome = collection.syncPresentation.buttonFeedback.observe(moving: now.moving, upToDate: now.upToDate,
            needsYou: now.needsYou, now: clock.now())
        // T1: the end of a sync the person started is announced.
        switch outcome {
        case .succeeded?: announce(SyncText.value("sync.result.succeeded"))
        case .ended?: announce(SyncText.value(titleKey).replacingOccurrences(of: "…", with: ""))
        case nil: break
        }
    }
    private func announce(_ text: String) {
        AccessibilityNotification.Announcement(text).post()
    }
    private func openSettingsFromMenu() {
        collection.syncPresentation.statusPresented = false
        #if os(macOS)
        collection.syncPresentation.settingsRequest = UUID()
        openSettings()
        #else
        collection.openAccountSyncFromStatus(keepCollection: false)
        #endif
    }
    /// One detent, the panel's height: no larger detent that would only show
    /// empty space. Until the panel is measured, the system's medium height.
    private static func fittedDetents(_ height: CGFloat?) -> Set<PresentationDetent> {
        height.map { [.height($0)] } ?? [.medium]
    }
    private var panel: some View {
        SyncStatusPanel(collection: collection, openEntry: openStatusEntry,
            showSettings: { showSettings(keepCollection: $0) })
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("sync.status.popover")
    }
    private func openStatusEntry(_ entry: CloudSyncEntry, _ host: AppNavigation.SheetHost) {
        if compact { collection.queueCloudSyncAfterStatusDismissal(entry, host: host) }
        else { collection.presentCloudSync(entry, host: host) }
    }
    /// `keepCollection`: Choose… or Try Again for Meguri Sync. The Sync section
    /// presents N15.3's sheet, so the action opens it there.
    private func showSettings(keepCollection: Bool = false) {
        if compact {
            collection.queueAccountSyncAfterStatusDismissal(keepCollection: keepCollection)
            return
        }
        collection.syncPresentation.statusPresented = false
        #if os(macOS)
        if keepCollection { collection.syncPresentation.keepCollectionRequested = true }
        collection.syncPresentation.settingsRequest = UUID()
        openSettings()
        #else
        collection.openAccountSyncFromStatus(keepCollection: keepCollection)
        #endif
    }
}

#if os(iOS)
/// N23.1: the panel laid out where its sheet would show it, hidden, so the sheet's
/// one detent is known before the sheet opens. Its width comes from the last
/// sheet shown, or before the first from the window. The shell hosts it behind
/// its root, never in the toolbar: the toolbar item is the symbol alone (N23.6).
public struct CloudSyncPanelMeasure: View {
    let collection: ReviewCollection
    @Environment(\.horizontalSizeClass) private var sizeClass
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        let presentation = collection.syncPresentation
        let kind: SheetMetrics.Kind? = sizeClass == .compact ? .sheet
            : UIDevice.current.userInterfaceIdiom == .phone ? .popover : nil
        if let kind, !presentation.statusPresented, let width = SheetMetrics.contentWidth(kind) {
            SyncStatusPanel(collection: collection, openEntry: { _, _ in }, showSettings: { _ in })
                .padding()
                .frame(width: width)
                .fixedSize(horizontal: false, vertical: true)
                .hidden()
                .accessibilityHidden(true)
                .allowsHitTesting(false)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { needed in
                    switch kind {
                    case .sheet: presentation.panelSheetHeight = Double(needed)
                    case .popover: presentation.panelPopoverHeight = Double(needed)
                    }
                }
        }
    }
}

/// The width a sheet on an iPhone-class screen gives the panel, learned from
/// the last sheet shown.
@MainActor enum SheetMetrics {
    enum Kind { case sheet, popover }
    private static var widths: [Kind: CGFloat] = [:]

    static func learn(_ kind: Kind, width: CGFloat) { widths[kind] = width }

    /// Before the first sheet: the window's width less the floating sheet's
    /// margins; the popover's sheet keeps the popover's width.
    static func contentWidth(_ kind: Kind) -> CGFloat? {
        if let width = widths[kind] { return width }
        switch kind {
        case .popover: return ContentWidth.primaryAction
        case .sheet: return keyWindow.map { $0.bounds.width - 2 * Spacing.xSmall }
        }
    }

    private static var keyWindow: UIWindow? {
        UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows).first(where: \.isKeyWindow)
    }
}

/// The panel in a scroll view that reports its height, so a sheet can be
/// exactly that tall (a height detent keeps the sheet's own safe area). It
/// also records the sheet's width for the next measurement.
private struct FittedPanelScroll<Content: View>: View {
    @Binding var height: CGFloat?
    let learns: SheetMetrics.Kind
    @ViewBuilder let content: () -> Content
    private struct Fit: Equatable { var needed, width: CGFloat }
    var body: some View {
        ScrollView { content() }
            .scrollBounceBehavior(.basedOnSize)
            .onScrollGeometryChange(for: Fit.self) { geometry in
                Fit(needed: geometry.contentSize.height, width: geometry.containerSize.width)
            } action: { _, fit in
                guard fit.needed > 0 else { return }
                SheetMetrics.learn(learns, width: fit.width)
                height = fit.needed
            }
    }
}
#endif
