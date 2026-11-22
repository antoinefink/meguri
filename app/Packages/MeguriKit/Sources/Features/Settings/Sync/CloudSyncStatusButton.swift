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
    @State private var popoverHeight: CGFloat?
    @State private var sheetHeight: CGFloat?
    #endif
    public init(collection: ReviewCollection) { self.collection = collection }
    private var meguri: Bool { collection.sync.providerState?.selection == .meguri }
    /// T20.1: the service's name, or `sync.button.label.off` with no service.
    private var accessibilityLabelKey: String {
        switch collection.sync.providerState?.selection {
        case .meguri: "sync.meguri.button.label"
        case .iCloud: "sync.button.label"
        case .localOnly, nil: "sync.button.label.off"
        }
    }
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
        HStack {
            if let mediaLine {
                Text(mediaLine).font(.footnote).foregroundStyle(.secondary)
                    .accessibilityIdentifier("sync.status.mediaIndicator")
            }
            Button { presented.wrappedValue.toggle() } label: {
                // N23.2: the moving symbol turns exactly while the panel's bar shows.
                SyncStatusSymbol(status: main, iCloud: collection.sync.symbolsAreICloud,
                    turning: !reduceMotion && collection.sync.panelProgress != nil)
            }
            #if os(iOS)
            // The sheet opens at the panel's height, not at a default that it then
            // leaves: the panel is measured here, out of sight, while no sheet shows.
            .background { sheetMeasure }
            #endif
            .accessibilityLabel(SyncText.value(accessibilityLabelKey))
            .accessibilityValue(SyncText.value(titleKey).replacingOccurrences(of: "…", with: ""))
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
                FittedPanelScroll(height: $popoverHeight, learns: .popover) {
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
                FittedPanelScroll(height: $sheetHeight, learns: .sheet) { panel.padding() }
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
        }
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
    private var mediaLine: String? {
        if meguri { return nil }
        guard let media = collection.sync.status?.media,
              (media.uploadsRunning ?? 0) > 0 || (media.downloadsRunning ?? 0) > 0,
              let count = SyncStatusCopy.sum(media.uploadFiles, media.downloadFiles) else { return nil }
        return SyncText.format("sync.media.filesLeft", Int64(clamping: count))
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
extension CloudSyncStatusButton {
    /// The panel laid out where its sheet would show it, hidden, so the sheet's
    /// one detent is known before the sheet opens. Width and insets come from the
    /// last sheet shown, or before the first from the window.
    @ViewBuilder fileprivate var sheetMeasure: some View {
        let kind: SheetMetrics.Kind? = compact ? .sheet
            : UIDevice.current.userInterfaceIdiom == .phone ? .popover : nil
        if let kind, !presented.wrappedValue, let width = SheetMetrics.contentWidth(kind) {
            SyncStatusPanel(collection: collection, openEntry: { _, _ in }, showSettings: { _ in })
                .padding()
                .frame(width: width)
                .fixedSize(horizontal: false, vertical: true)
                .hidden()
                .accessibilityHidden(true)
                .allowsHitTesting(false)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { panel in
                    let needed = panel + SheetMetrics.insets(kind)
                    switch kind {
                    case .sheet: sheetHeight = needed
                    case .popover: popoverHeight = needed
                    }
                }
        }
    }
}

/// What a sheet on an iPhone-class screen gives the panel: its width and the
/// insets around it, learned from the last sheet shown.
@MainActor enum SheetMetrics {
    enum Kind { case sheet, popover }
    private static var widths: [Kind: CGFloat] = [:]
    private static var learnedInsets: [Kind: CGFloat] = [:]

    static func learn(_ kind: Kind, width: CGFloat, insets: CGFloat) {
        widths[kind] = width
        learnedInsets[kind] = insets
    }

    /// Before the first sheet: the window's width less the floating sheet's
    /// margins; the popover's sheet keeps the popover's width.
    static func contentWidth(_ kind: Kind) -> CGFloat? {
        if let width = widths[kind] { return width }
        switch kind {
        case .popover: return ContentWidth.primaryAction
        case .sheet: return keyWindow.map { $0.bounds.width - 2 * Spacing.xSmall }
        }
    }

    static func insets(_ kind: Kind) -> CGFloat {
        learnedInsets[kind] ?? keyWindow?.safeAreaInsets.bottom ?? 0
    }

    private static var keyWindow: UIWindow? {
        UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows).first(where: \.isKeyWindow)
    }
}

/// The panel in a scroll view that reports the height it needs, its insets
/// included (the sheet's safe area), so a sheet can be exactly that tall. It
/// also records the sheet's width and insets for the next measurement.
private struct FittedPanelScroll<Content: View>: View {
    @Binding var height: CGFloat?
    let learns: SheetMetrics.Kind
    @ViewBuilder let content: () -> Content
    private struct Fit: Equatable { var needed, width, insets: CGFloat }
    var body: some View {
        ScrollView { content() }
            .scrollBounceBehavior(.basedOnSize)
            .onScrollGeometryChange(for: Fit.self) { geometry in
                let insets = geometry.contentInsets.top + geometry.contentInsets.bottom
                return Fit(needed: geometry.contentSize.height + insets, width: geometry.containerSize.width, insets: insets)
            } action: { _, fit in
                guard fit.needed > 0 else { return }
                SheetMetrics.learn(learns, width: fit.width, insets: fit.insets)
                height = fit.needed
            }
    }
}
#endif
