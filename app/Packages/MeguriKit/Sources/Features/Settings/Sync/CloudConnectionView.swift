import AppModel
import CoreBridge
import SwiftUI

/// Every confirmation consumes the coordinator's retained preview authority.
/// No view creates a connection service or maps an unsupported choice to publish.
public struct CloudConnectionView: View {
    let collection: ReviewCollection
    let entry: CloudSyncEntry
    @State private var preparing = false
    @State private var confirming = false
    @State private var cancelling = false
    @State private var submitted = false
    @State private var errorKey: String?
    public init(collection: ReviewCollection, entry: CloudSyncEntry) {
        self.collection = collection; self.entry = entry
    }
    private var sync: SyncCoordinator { collection.sync }
    private var summary: CloudConnectionSummary? {
        guard case .ready(_, let summary) = sync.preview else { return nil }
        return summary
    }
    private var page: CloudSyncEntry { collection.syncPresentation.connectionPath.last ?? entry }
    public var body: some View {
        NavigationStack(path: Binding(get: { collection.syncPresentation.connectionPath },
            set: { collection.syncPresentation.connectionPath = $0 })) {
            content(entry)
                .navigationDestination(for: CloudSyncEntry.self) { content($0) }
        }
        .interactiveDismissDisabled(confirming || preparing || cancelling)
        .task { await prepare() }
        .onChange(of: sync.settings.paused) { _, paused in
            if paused, !submitted { Task { await sync.cancelConnectionPreview() } }
        }
        .onDisappear { if !submitted { Task { await sync.cancelConnectionPreview() } } }
    }
    private func content(_ page: CloudSyncEntry) -> some View {
        Form {
            if let errorKey { Text(SyncText.value(errorKey)) }
            if sync.operationFailed { Text(SyncText.value("sync.recovery.error.other")) }
            if confirming {
                ProgressView(SyncText.value("sync.publish.preparing"))
            } else if submitted {
                if let recovery = sync.recovery { CloudRecoveryContent(collection: collection, recovery: recovery) }
                else { Text(SyncText.value("sync.counts.unknown")) }
            } else if preparing || sync.connectionPreviewActive || cancelling {
                ProgressView(SyncText.value("sync.connect.reading"))
            } else if page == .publicationChoice {
                publicationChoice
            } else if let summary {
                preview(summary, page: page)
            } else {
                Group {
                    Text(SyncText.value("sync.counts.unknown"))
                    Button(SyncText.value("sync.recovery.tryAgain")) { Task { await prepare() } }
                        .disabled(sync.settings.paused || sync.connectionOperationActive)
                    if sync.connectionOperationActive { Text(SyncText.value("sync.recovery.error.busy")) }
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle(title(page))
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button {
                    Task { await close() }
                } label: {
                    if submitted { Text("Close", bundle: .module) }
                    else { Text(SyncText.value(page == .publicationChoice ? "sync.decision.notNow" : "sync.now.cancel")) }
                }
                .disabled(confirming || preparing || cancelling || sync.connectionPreviewCancelling)
                .accessibilityIdentifier(page == .publicationChoice ? "sync.publicationChoice.notNow" : "sync.connection.cancel")
            }
            if !submitted, !confirming, !cancelling, !sync.connectionPreviewActive, let summary {
                if canAdopt(summary, page: page) {
                    ToolbarItem(placement: .destructiveAction) {
                        Button(SyncText.value(page == .connect ? "sync.connect.useICloud.confirm" : "sync.adopt.confirm"), role: .destructive) {
                            Task { await confirm(adopting: true) }
                        }
                        .disabled(sync.settings.paused)
                        .accessibilityIdentifier("sync.replaceLocal.replace")
                    }
                }
                if page == .replaceCloud, summary.decision == .chooseCollection {
                    ToolbarItem(placement: .destructiveAction) {
                        Button(SyncText.value("sync.publish.confirm"), role: .destructive) { Task { await confirm(adopting: false) } }
                            .disabled(sync.settings.paused)
                            .accessibilityIdentifier("sync.replaceCloud.replace")
                    }
                }
            }
        }
    }
    @ViewBuilder private func preview(_ summary: CloudConnectionSummary, page: CloudSyncEntry) -> some View {
        if page == .replaceLocal {
            Text(SyncText.value("sync.adopt.direction"))
        } else if page == .replaceCloud {
            Text(SyncText.value("sync.publish.direction"))
        } else {
            switch summary.decision {
            case .noCollection: Text(SyncText.value("sync.now.unavailable.collectionUnavailable"))
            case .confirmUpload: Text(SyncText.value("sync.connect.upload.message"))
            case .confirmAdoption: Text(SyncText.value("sync.connect.useICloud.message"))
            case .chooseCollection: Text(SyncText.value("sync.connect.two.message"))
            case .cloudUnreadable: Text(SyncText.value("sync.attention.rootUnreadable"))
            }
        }
        if summary.localCollectionPresent { CloudCountSection(counts: summary.localCounts, cloud: false) }
        if let counts = summary.manifestCounts {
            CloudCountSection(counts: counts, cloud: true, publishedAt: summary.publishedAtMilliseconds)
        }
        Section {
            ForEach(Self.kindKeys, id: \.0) { kind, key in
                if let count = summary.unpublishedCounts[kind], count > 0 {
                    Text(SyncText.format(key, Int64(clamping: count)))
                }
            }
            Text(SyncText.value("sync.preview.otherDevicesUnknown"))
        }
        if page == .connect {
            switch summary.decision {
            case .noCollection: Text(SyncText.value("sync.recovery.error.notOffered"))
            case .confirmUpload:
                Button(SyncText.value("sync.connect.upload.confirm")) { Task { await confirm(adopting: false) } }
                    .disabled(sync.settings.paused)
                    .accessibilityIdentifier("sync.connect.upload")
            case .confirmAdoption: EmptyView() // destructive toolbar confirmation, never Return
            case .chooseCollection:
                Section {
                    choice("sync.connect.two.useICloud", caption: "sync.connect.two.useICloud.caption", destination: .replaceLocal)
                    choice("sync.connect.two.replace", caption: "sync.connect.two.replace.caption", destination: .replaceCloud)
                    Button(SyncText.value("sync.connect.two.keepLocal")) {}.disabled(true)
                        .accessibilityIdentifier("sync.connect.choice.keepLocal")
                    Text(SyncText.value("sync.recovery.error.notOffered"))
                }
            case .cloudUnreadable: Text(SyncText.value("sync.recovery.error.notOffered"))
            }
        } else if page == .replaceLocal {
            if SyncStatusPresentation.permitsAdoption(summary.decision) {
                Text(SyncText.value("sync.adopt.backup"))
            } else { Text(SyncText.value("sync.recovery.error.notOffered")) }
        } else if page == .replaceCloud {
            if summary.decision == .chooseCollection { Text(SyncText.value("sync.publish.otherDevices")) }
            else { Text(SyncText.value("sync.recovery.error.notOffered")) }
        }
    }
    private var publicationChoice: some View {
        Group {
            Text(SyncText.value("sync.decision.title.none"))
            Text(SyncText.value("sync.decision.message"))
            if let summary {
                CloudCountSection(counts: summary.localCounts, cloud: false)
                if !sync.settings.paused, let counts = summary.manifestCounts {
                    CloudCountSection(counts: counts, cloud: true, publishedAt: summary.publishedAtMilliseconds)
                }
            }
            Section {
                // Publication-fence reasons and keep_local_only are not in the
                // public coordinator surface. Do not publish with reason 2.
                if sync.settings.paused || sync.status?.accountStatus == .available {
                    Button(SyncText.value("sync.decision.replace")) {}.disabled(true)
                        .accessibilityIdentifier("sync.publicationChoice.replaceICloud")
                    Text(SyncText.value("sync.decision.replace.caption"))
                    if sync.settings.paused { Text(SyncText.value("sync.status.paused.user")) }
                }
                Button(SyncText.value("sync.decision.keepLocal")) {}.disabled(true)
                    .accessibilityIdentifier("sync.publicationChoice.keepLocal")
                Text(SyncText.value("sync.decision.keepLocal.caption"))
                Text(SyncText.value("sync.recovery.error.notOffered"))
            }
        }
    }
    private func choice(_ key: String, caption: String, destination: CloudSyncEntry) -> some View {
        NavigationLink(value: destination) {
            VStack(alignment: .leading) {
                Text(SyncText.value(key))
                Text(SyncText.value(caption)).foregroundStyle(.secondary)
            }
        }
        .disabled(sync.settings.paused)
        .accessibilityIdentifier(destination == .replaceLocal ? "sync.connect.choice.useICloud" : "sync.connect.choice.replaceICloud")
    }
    private func canAdopt(_ summary: CloudConnectionSummary, page: CloudSyncEntry) -> Bool {
        (page == .replaceLocal && SyncStatusPresentation.permitsAdoption(summary.decision))
            || (page == .connect && summary.decision == .confirmAdoption)
    }
    private func title(_ page: CloudSyncEntry) -> String {
        let key: String
        switch page {
        case .replaceLocal: key = "sync.adopt.title"
        case .replaceCloud: key = "sync.publish.title"
        case .publicationChoice: key = "sync.decision.sheetTitle"
        case .connect:
            key = switch summary?.decision {
            case .confirmUpload: "sync.connect.upload.title"
            case .confirmAdoption: "sync.connect.useICloud.title"
            case .chooseCollection: "sync.connect.two.title"
            case .noCollection, .cloudUnreadable, nil: "sync.button.label"
            }
        }
        return SyncText.value(key)
    }
    private func prepare() async {
        guard !preparing, !cancelling, !sync.connectionPreviewActive, !sync.connectionOperationActive,
              !sync.settings.paused, sync.preview == nil, !submitted else { return }
        preparing = true; errorKey = nil
        defer { preparing = false }
        do { try await collection.prepareCloudConnectionPreview(allowDuringReview: entry == .replaceCloud || entry == .publicationChoice) }
        catch BridgeError.sessionActive { errorKey = "sync.recovery.error.sessionActive" }
        catch BridgeError.unavailable(let reason) where reason == "collectionUnavailable" {
            errorKey = "sync.now.unavailable.collectionUnavailable"
        }
        catch { errorKey = sync.setupState == .configurationUnavailable ? "sync.status.unavailable.setupNeeded" : "sync.recovery.error.other" }
    }
    private func confirm(adopting: Bool) async {
        guard !confirming, !submitted, !sync.settings.paused, let summary else { return }
        if adopting, collection.review != nil { errorKey = "sync.recovery.error.sessionActive"; return }
        if adopting { guard canAdopt(summary, page: page) else { return } }
        else {
            guard (page == .connect && summary.decision == .confirmUpload)
                || (page == .replaceCloud && summary.decision == .chooseCollection) else { return }
        }
        confirming = true; submitted = true
        if adopting { await sync.confirmAdoption() } else { await sync.confirmPublication() }
        confirming = false
    }
    private func close() async {
        guard !confirming, !preparing, !cancelling else { return }
        cancelling = true
        defer { cancelling = false }
        if !submitted { await sync.cancelConnectionPreview() }
        collection.navigation.dismissSheet()
    }
    private static let kindKeys: [(CloudCountKind, String)] = [(.note, "sync.kind.note"), (.card, "sync.kind.card"),
        (.revlog, "sync.kind.review"), (.deck, "sync.kind.deck"), (.preset, "sync.kind.preset"),
        (.notetype, "sync.kind.noteType"), (.tag, "sync.kind.tag"), (.config, "sync.kind.setting"), (.mediaName, "sync.kind.mediaFile")]
}

private struct CloudCountSection: View {
    let counts: [CloudCountKind: UInt64]
    let cloud: Bool
    var publishedAt: Int64?
    @Environment(\.appClock) private var clock
    var body: some View {
        Section {
            ForEach(Self.rows, id: \.0) { kind, key in
                LabeledContent(SyncText.value(key)) {
                    if let count = counts[kind] { Text(count, format: .number) }
                    else { Text(SyncText.value("sync.counts.unknown")) }
                }
            }
            LabeledContent(SyncText.value("sync.counts.lastReview"), value: SyncText.value("sync.counts.unknown"))
            LabeledContent(SyncText.value("sync.counts.lastChange"), value: SyncText.value("sync.counts.unknown"))
            if cloud, let publishedAt {
                Text(SyncText.format("sync.counts.asOf", SyncText.instant(publishedAt, now: clock.now())))
            }
        } header: { Text(SyncText.value(cloud ? "sync.counts.iCloud" : "sync.counts.thisDevice")) }
    }
    private static let rows: [(CloudCountKind, String)] = [(.note, "sync.counts.notes"), (.card, "sync.counts.cards"),
        (.revlog, "sync.counts.reviews"), (.deck, "sync.counts.decks"), (.notetype, "sync.counts.noteTypes"), (.mediaName, "sync.counts.mediaFiles")]
}
