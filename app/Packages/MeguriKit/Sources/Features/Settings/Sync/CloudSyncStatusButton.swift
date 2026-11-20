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
    public init(collection: ReviewCollection) { self.collection = collection }
    private var meguri: Bool { collection.sync.providerState?.selection == .meguri }
    /// N15.4: the needs-attention symbol only for needs you; otherwise the state underneath.
    private var main: SyncMainStatus { collection.sync.mainStatus }
    private var titleKey: String { main.titleKey }
    private var detailKey: String? { main.detailCount == nil ? main.detailKey : nil }
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
                Image(systemName: main.symbol)
                    .symbolEffect(.rotate, options: .repeating,
                        isActive: !reduceMotion && (meguri ? collection.sync.meguriStatus.isBusy : collection.sync.status?.state == .syncingCollection))
            }
            .accessibilityLabel(SyncText.value(meguri ? "sync.meguri.button.label" : "sync.button.label"))
            .accessibilityValue(SyncText.value(titleKey).replacingOccurrences(of: "…", with: ""))
            .help([SyncText.value(titleKey), detailKey.map(SyncText.value)].compactMap { $0 }.joined(separator: "\n"))
            .accessibilityIdentifier("sync.status.button")
            .disabled(collection.syncPresentation.statusHandoffPending)
            .popover(isPresented: presented(compact: false)) {
                status
                    // A navigation stack has no size of its own: without one the popover
                    // shrinks to the two buttons under the form and hides the status.
                    .frame(idealWidth: ContentWidth.primaryAction,
                        idealHeight: ContentWidth.primaryAction + ControlMetrics.minimumTarget + ControlMetrics.minimumTarget)
                    .presentationCompactAdaptation(.sheet)
                    .presentationDetents([.medium, .large])
            }
            #if os(iOS)
            // Keep status and connection separate; native dismissal, rather
            // than a delay or view disappearance, orders the compact handoff.
            .sheet(isPresented: presented(compact: true), onDismiss: {
                guard let id = nativeCompactStatusID else { return }
                nativeCompactStatusID = nil
                collection.compactSyncStatusDidDismiss(id)
            }) {
                status
                    .presentationDetents([.medium, .large])
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
    private var status: some View {
        NavigationStack {
            VStack {
                Form {
                    Section {
                        SyncMainStatusRows(collection: collection, openEntry: openStatusEntry,
                            showInSettings: { showSettings(keepCollection: $0) })
                        if meguri, MeguriMediaFactRows.isVisible(collection.sync) {
                            MeguriMediaFactRows(collection: collection)
                        }
                    }
                }
                .formStyle(.grouped)
                Divider()
                VStack {
                    SyncNowButton(collection: collection)
                    Button(SyncText.value("sync.status.openSettings")) { showSettings() }
                        .accessibilityIdentifier("sync.status.settings")
                }
                .padding()
            }
        }
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
