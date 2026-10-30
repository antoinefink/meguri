import AppModel
import CoreBridge
import SwiftUI

/// The same quiet control is injected into the platform toolbar and review.
public struct CloudSyncStatusButton: View {
    let collection: ReviewCollection
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    #if os(macOS)
    @Environment(\.openSettings) private var openSettings
    #endif
    public init(collection: ReviewCollection) { self.collection = collection }
    private var copy: SyncStatusPresentation { .init(state: collection.sync.status?.state, setup: collection.sync.setupState) }
    private var presented: Binding<Bool> {
        Binding(get: { collection.navigation.sheet == nil && collection.syncPresentation.statusPresented },
            set: { collection.syncPresentation.statusPresented = $0 })
    }
    public var body: some View {
        HStack {
            if let mediaLine {
                Text(mediaLine).font(.footnote).foregroundStyle(.secondary)
                    .accessibilityIdentifier("sync.status.mediaIndicator")
            }
            Button { presented.wrappedValue.toggle() } label: {
                Image(systemName: copy.symbol)
                    .symbolEffect(.rotate, options: .repeating,
                        isActive: !reduceMotion && collection.sync.status?.state == .syncingCollection)
            }
            .accessibilityLabel(SyncText.value("sync.button.label"))
            .accessibilityValue(SyncText.value(copy.titleKey).replacingOccurrences(of: "…", with: ""))
            .help([SyncText.value(copy.titleKey), copy.detailKey.map(SyncText.value)].compactMap { $0 }.joined(separator: "\n"))
            .accessibilityIdentifier("sync.status.button")
            .popover(isPresented: presented) {
                status
                    .presentationCompactAdaptation(.sheet)
                    .presentationDetents([.medium, .large])
            }
        }
    }
    private var status: some View {
        VStack {
            Form {
                SyncStatusContent(collection: collection)
                if collection.sync.status?.controls.enable == .connect {
                    Section { CloudPreviewEntryButton(collection: collection, entry: .connect) }
                }
                if collection.sync.status?.controls.decisionRequired == true {
                    Section {
                        Button(SyncText.value("sync.decision.action")) { collection.presentCloudSync(.publicationChoice) }
                            .accessibilityLabel(SyncText.value("sync.decision.action.label"))
                    }
                }
                if collection.sync.settings.paused {
                    Section {
                        Button(SyncText.value("sync.settings.resumePause")) {
                            let settings = collection.sync.settings
                            collection.sync.setDeviceSettings(paused: false, automatic: settings.automatic, media: settings.media)
                        }
                    }
                }
                CloudAttentionSection(collection: collection)
            }
            .formStyle(.grouped)
            Divider()
            VStack {
                SyncNowButton(collection: collection)
                Button(SyncText.value("sync.settings.section"), action: showSettings)
                    .accessibilityIdentifier("sync.status.accountSync")
            }
            .padding()
        }
        .accessibilityIdentifier("sync.status.popover")
    }
    private var mediaLine: String? {
        guard let media = collection.sync.status?.media,
              (media.uploadsRunning ?? 0) > 0 || (media.downloadsRunning ?? 0) > 0,
              let count = SyncStatusCopy.sum(media.uploadFiles, media.downloadFiles) else { return nil }
        return SyncText.format("sync.media.filesLeft", Int64(clamping: count))
    }
    private func showSettings() {
        collection.syncPresentation.statusPresented = false
        #if os(macOS)
        collection.syncPresentation.settingsRequest = UUID()
        openSettings()
        #else
        collection.navigation.openAccountSync()
        #endif
    }
}
