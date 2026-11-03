import AppModel
import CoreBridge
import SwiftUI

/// Every confirmation consumes the coordinator's retained preview authority.
/// No view creates a connection service or maps an unsupported choice to publish.
public struct CloudConnectionView: View {
    let collection: ReviewCollection
    let entry: CloudSyncEntry
    @State private var preparing = false
    @State private var otherConfirming = false
    @State private var cancelling = false
    @State private var otherSubmitted = false
    @State private var publication = CloudPublicationConfirmation()
    @State private var confirmation: CloudConnectionConfirmation?
    @State private var errorKey: String?
    public init(collection: ReviewCollection, entry: CloudSyncEntry) {
        self.collection = collection; self.entry = entry
    }
    private var sync: SyncCoordinator { collection.sync }
    private var confirming: Bool { otherConfirming || publication.confirming }
    private var submitted: Bool { otherSubmitted || publication.submitted }
    private var publicationPage: Bool {
        page == .replaceCloud || page == .publicationChoice || confirmation == .publication
            || (page == .connect && summary?.decision == .confirmUpload)
    }
    private var summary: CloudConnectionSummary? {
        guard case .ready(_, let summary) = sync.preview else { return nil }
        return summary
    }
    private var page: CloudSyncEntry { collection.syncPresentation.connectionPath.last ?? entry }
    private var publicationReady: Bool {
        SyncStatusPresentation.publicationReady(summary?.decision,
            accountAvailable: sync.status?.accountStatus == .available, paused: sync.settings.paused,
            busy: preparing || confirming || cancelling || submitted
                || sync.connectionPreviewActive || sync.connectionOperationActive)
    }
    public var body: some View {
        NavigationStack(path: Binding(get: { collection.syncPresentation.connectionPath },
            set: { collection.syncPresentation.connectionPath = $0 })) {
            content(entry)
                .navigationDestination(for: CloudSyncEntry.self) { content($0) }
        }
        .interactiveDismissDisabled(confirming || preparing || cancelling || sync.initialAdoptionActive)
        .task { await prepare() }
        .onChange(of: sync.settings.paused) { _, paused in
            if paused, !submitted { Task { await sync.cancelConnectionPreview() } }
        }
        .onChange(of: sync.generation) { _, _ in publication.invalidate() }
        .onDisappear {
            publication.invalidate()
            if !submitted { Task { await sync.cancelConnectionPreview() } }
        }
    }
    private func content(_ page: CloudSyncEntry) -> some View {
        Form {
            if let errorKey { Text(SyncText.value(errorKey)) }
            if publicationPage, publication.refused {
                Text(SyncText.value(publication.failure.map(SyncStatusPresentation.connectionErrorKey) ?? "sync.recovery.error.other"))
            } else if publicationPage {
                if let key = publication.backgroundErrorKey {
                    Text(SyncText.value(key))
                        .accessibilityIdentifier("sync.publish.error")
                } else if !submitted, !confirming, let failure = sync.connectionFailure {
                    Text(SyncText.value(SyncStatusPresentation.connectionErrorKey(failure)))
                }
            } else if !submitted || confirmation != .resume {
                if let failure = sync.connectionFailure {
                    Text(SyncText.value(SyncStatusPresentation.connectionErrorKey(failure)))
                } else if sync.operationFailed { Text(SyncText.value("sync.recovery.error.other")) }
            }
            if sync.initialAdoptionActive {
                ProgressView(SyncText.value("sync.connect.downloading"))
                    .accessibilityIdentifier("sync.connect.download")
            } else if confirming {
                ProgressView(SyncText.value("sync.publish.preparing"))
            } else if submitted {
                if confirmation == .resume { SyncStatusRows(collection: collection) }
                else if publication.accepted, sync.connectionOperationActive,
                        sync.recovery == nil || sync.recovery?.step == .confirmed || sync.recovery?.step == .bound {
                    ProgressView(SyncText.value("sync.publish.uploading"))
                }
                else if let recovery = sync.recovery { CloudRecoveryContent(collection: collection, recovery: recovery) }
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
                    if submitted && !confirming { Text("Close", bundle: .module) }
                    else { Text(SyncText.value(page == .publicationChoice ? "sync.decision.notNow" : "sync.now.cancel")) }
                }
                .disabled(confirming || preparing || cancelling || sync.connectionPreviewCancelling || sync.connectionActivationActive)
                .accessibilityIdentifier(page == .publicationChoice ? "sync.publicationChoice.notNow" : "sync.connection.cancel")
            }
            if page == .connect, (summary.map { SyncStatusPresentation.permitsResume($0.decision) } == true
                || (confirming && confirmation == .resume)) {
                ToolbarItem(placement: .confirmationAction) {
                    Button(SyncText.value("sync.connect.continue.confirm")) { Task { await confirm(.resume) } }
                        .disabled(confirming || preparing || cancelling || submitted || sync.settings.paused
                            || sync.connectionPreviewActive || sync.connectionOperationActive)
                        .accessibilityIdentifier("sync.connect.continue")
                }
            }
            if !submitted, !confirming, !cancelling, !sync.connectionPreviewActive, let summary {
                if canAdopt(summary, page: page) {
                    ToolbarItem(placement: .destructiveAction) {
                        Button(SyncText.value(page == .connect ? "sync.connect.useICloud.confirm" : "sync.adopt.confirm"), role: .destructive) {
                            Task { await confirm(.adoption) }
                        }
                        .disabled(sync.settings.paused)
                        .accessibilityIdentifier("sync.replaceLocal.replace")
                    }
                }
                if page == .replaceCloud, SyncStatusPresentation.permitsPublication(summary.decision, on: page) {
                    ToolbarItem(placement: .destructiveAction) {
                        Button(SyncText.value("sync.publish.confirm"), role: .destructive) { Task { await confirm(.publication) } }
                            .disabled(!publicationReady)
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
            case .confirmResume:
                Text(SyncText.format("sync.connect.continue.message", Int64(clamping: summary.unsentChangeCount)))
            case .chooseCollection: Text(SyncText.value("sync.connect.two.message"))
            case .cloudUnreadable: Text(SyncText.value("sync.attention.rootUnreadable"))
            }
        }
        if page == .connect, SyncStatusPresentation.permitsResume(summary.decision) {
            Section {
                ForEach(Self.kindKeys, id: \.0) { kind, key in
                    if let count = summary.unsentCounts[kind], count > 0 {
                        Text(SyncText.format(key, Int64(clamping: count)))
                    }
                }
                Text(SyncText.value("sync.preview.otherDevicesUnknown"))
            }
        } else {
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
        }
        if page == .connect {
            switch summary.decision {
            case .noCollection: Text(SyncText.value("sync.recovery.error.notOffered"))
            case .confirmUpload:
                Button(SyncText.value("sync.connect.upload.confirm")) { Task { await confirm(.publication) } }
                    .disabled(!publicationReady)
                    .accessibilityIdentifier("sync.connect.upload")
            case .confirmAdoption: EmptyView() // destructive toolbar confirmation, never Return
            case .confirmResume: EmptyView() // native confirmation toolbar action
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
            if SyncStatusPresentation.permitsPublication(summary.decision, on: page) {
                Text(SyncText.value("sync.publish.otherDevices"))
            } else { Text(SyncText.value("sync.recovery.error.notOffered")) }
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
                if sync.settings.paused || sync.status?.accountStatus == .available {
                    NavigationLink(value: CloudSyncEntry.replaceCloud) {
                        Text(SyncText.value("sync.decision.replace"))
                    }
                    .disabled(!publicationReady)
                    .accessibilityIdentifier("sync.publicationChoice.replaceICloud")
                    Text(SyncText.value("sync.decision.replace.caption"))
                    if sync.settings.paused { Text(SyncText.value("sync.status.paused.user")) }
                }
                // keep_local_only still needs the Core/coordinator operation surface.
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
            key = switch (confirmation == .resume ? CloudConnectionDecision.confirmResume : summary?.decision) {
            case .confirmUpload: "sync.connect.upload.title"
            case .confirmAdoption: "sync.connect.useICloud.title"
            case .confirmResume: "sync.connect.continue.title"
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
        publication.clearRefusal()
        defer { preparing = false }
        do { try await collection.prepareCloudConnectionPreview(allowDuringReview: entry == .replaceCloud || entry == .publicationChoice) }
        catch BridgeError.sessionActive { errorKey = "sync.recovery.error.sessionActive" }
        catch BridgeError.unavailable(let reason) where reason == "collectionUnavailable" {
            errorKey = "sync.now.unavailable.collectionUnavailable"
        }
        catch { errorKey = sync.setupState == .configurationUnavailable ? "sync.status.unavailable.setupNeeded" : "sync.recovery.error.other" }
    }
    private func confirm(_ intent: CloudConnectionConfirmation) async {
        guard !confirming, !submitted, !cancelling, !preparing, !sync.connectionOperationActive,
              !sync.settings.paused, let summary else { return }
        if intent != .publication, collection.review != nil { errorKey = "sync.recovery.error.sessionActive"; return }
        switch intent {
        case .adoption: guard canAdopt(summary, page: page) else { return }
        case .publication:
            guard publicationReady, SyncStatusPresentation.permitsPublication(summary.decision, on: page) else { return }
        case .resume:
            guard page == .connect, SyncStatusPresentation.permitsResume(summary.decision) else { return }
        }
        confirmation = intent; errorKey = nil
        if intent == .publication {
            await publication.confirm(using: sync)
            return
        }
        otherConfirming = true; otherSubmitted = true
        switch intent {
        case .adoption: await sync.confirmAdoption()
        case .publication: break // handled by the durable publication result above
        case .resume: await sync.confirmResume()
        }
        otherConfirming = false
    }
    private func close() async {
        guard !confirming, !preparing, !cancelling else { return }
        guard publication.close() else { return }
        cancelling = true
        defer { cancelling = false }
        if sync.initialAdoptionActive { await sync.cancelConnectionOperation() }
        else if !submitted { await sync.cancelConnectionPreview() }
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
