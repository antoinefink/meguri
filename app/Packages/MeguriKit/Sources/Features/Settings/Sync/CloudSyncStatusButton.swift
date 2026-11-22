import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

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
    /// The panel's own height, which sizes the sheet it shows in on touch.
    @State private var panelHeight: CGFloat?
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
            .accessibilityLabel(SyncText.value(accessibilityLabelKey))
            .accessibilityValue(SyncText.value(titleKey).replacingOccurrences(of: "…", with: ""))
            .help([SyncText.value(titleKey), SyncStatusCopy.detail(collection.sync, now: clock.now())]
                .compactMap { $0 }.joined(separator: "\n"))
            .accessibilityIdentifier("sync.status.button")
            .disabled(collection.syncPresentation.statusHandoffPending)
            .popover(isPresented: presented(compact: false)) {
                // N23.1: 320 points wide, as tall as its content.
                panel
                    .padding()
                    .frame(width: ContentWidth.primaryAction)
                    .fixedSize(horizontal: false, vertical: true)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { panelHeight = $0 }
                    .presentationCompactAdaptation(.sheet)
                    // Where the popover becomes a sheet (an iPhone held wide, the Duo
                    // opened), the sheet fits the panel, as at compact width.
                    .presentationDetents(fittedDetents)
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
                ScrollView {
                    panel
                        .padding()
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { panelHeight = $0 }
                }
                .scrollBounceBehavior(.basedOnSize)
                .presentationDetents(fittedDetents)
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
    private var fittedDetents: Set<PresentationDetent> {
        panelHeight.map { [.height($0)] } ?? [.medium]
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
