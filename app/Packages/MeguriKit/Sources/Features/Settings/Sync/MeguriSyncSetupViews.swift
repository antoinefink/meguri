import AppModel
import CoreBridge
import SwiftUI

struct SyncServiceSwitchSheet: View {
    let collection: ReviewCollection
    let source: SyncProviderSelection
    let destination: String
    let changing: Bool
    let failed: Bool
    let cancel: () -> Void
    let confirm: () -> Void
    private var sync: SyncCoordinator { collection.sync }
    private var oldService: String {
        SyncText.value(source == .meguri ? "sync.service.meguri" : "sync.service.iCloud")
    }
    /// T19.2: the destination's setup follows with no Connect step.
    private var nextKey: String {
        switch destination {
        case "iCloud": "sync.switch.next.iCloud.setup"
        case "meguri": "sync.switch.next.meguri.setup"
        default: "sync.switch.next.localOnly"
        }
    }
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    let facts = sync.serviceSwitchFacts(source: source)
                    switch facts.changes {
                    case .count(let pending):
                        if pending > 0 { Text(SyncText.format("sync.switch.pending", Int64(clamping: pending), oldService)) }
                        if let held = facts.heldBack, held > 0 { Text(SyncText.format("sync.switch.heldBack", Int64(clamping: held), oldService)) }
                        if pending == 0, facts.heldBack == 0 { Text(SyncText.format("sync.switch.pending.none", oldService)) }
                        else if pending == 0, facts.heldBack == nil { Text(SyncText.format("sync.switch.pending.unknown", oldService)) }
                    case .some: Text(SyncText.format("sync.switch.pending.some", oldService))
                    case .unknown: Text(SyncText.format("sync.switch.pending.unknown", oldService))
                    }
                    if let files = facts.media {
                        if files > 0 { Text(SyncText.format("sync.switch.media", Int64(clamping: files), oldService)) }
                    } else { Text(SyncText.format("sync.switch.media.unknown", oldService)) }
                    if facts.acceptanceUnknown {
                        Text(SyncText.value("sync.switch.acceptanceUnknown"))
                    }
                    Text(SyncText.format("sync.switch.kept", oldService))
                    if destination != "localOnly" { Text(SyncText.value("sync.switch.otherDevices")) }
                    if source == .meguri && !failed { Text(SyncText.value("sync.switch.signOut")) }
                    Text(SyncText.value(nextKey))
                    if !changing && !sync.serviceChangeAvailable { Text(SyncText.value(sync.serviceChangeBusyKey)) }
                    if changing { ProgressView(SyncText.value("sync.service.changing")) }
                    if failed { Text(SyncText.value("sync.service.changeFailed")) }
                }
            }
            .formStyle(.grouped)
            .navigationTitle(SyncText.value(destination == "localOnly" ? "sync.switch.title.off" : "sync.switch.title.\(destination)"))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(SyncText.value("sync.service.cancel"), role: .cancel, action: cancel)
                        .disabled(changing).accessibilityIdentifier("sync.serviceSwitch.cancel")
                }
                ToolbarItem(placement: .destructiveAction) {
                    Button(SyncText.value(destination == "localOnly" ? "sync.switch.confirm.off" : "sync.switch.confirm.\(destination)"),
                           role: .destructive, action: confirm)
                        .disabled(changing || !sync.serviceChangeAvailable)
                        .accessibilityIdentifier("sync.serviceSwitch.confirm")
                }
            }
            .interactiveDismissDisabled(changing)
        }
    }
}

struct MeguriSetupPreviewSheet: View {
    let collection: ReviewCollection
    let preview: MeguriBootstrapPreview
    private var sync: SyncCoordinator { collection.sync }
    private var kind: String {
        preview.receipt.direction == .export ? "useDevice" : preview.receipt.local == nil ? "download" : "useServer"
    }
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    LabeledContent(SyncText.value("sync.meguri.preview.server"), value: sync.meguriEndpoint)
                    if kind != "download", let counts = preview.receipt.local {
                        Text(SyncText.value("sync.counts.thisDevice"))
                        ForEach(Array(MeguriSyncStatusPresentation.replacementCountRows(
                            cards: counts.cards, notes: counts.notes, reviews: counts.reviews).enumerated()), id: \.offset) { _, row in
                            LabeledContent(row.label, value: row.value)
                        }
                    }
                    if preview.receipt.remoteHasNoCards == true { Text(SyncText.value("sync.meguri.preview.remoteEmpty")) }
                    Text(SyncText.value("sync.meguri.preview.direction.\(kind)"))
                    if kind == "useServer", let local = preview.receipt.local,
                       local.pendingReviews > 0 || !local.pendingRows.isEmpty { Text(SyncText.value("sync.meguri.preview.unsent")) }
                    if kind == "useDevice" { Text(SyncText.value("sync.meguri.preview.otherClients")) }
                    Text(SyncText.value("sync.meguri.preview.media"))
                    Text(SyncText.value("sync.meguri.preview.automatic"))
                    // Copies are shown by side and actual size; names remain
                    // confined to the affected-files page.
                    ForEach(preview.receipt.copies) { copy in
                        LabeledContent(SyncText.value(copy.server ? "sync.meguri.preview.server" : "sync.counts.thisDevice")) {
                            Text(ByteCountFormatter.string(fromByteCount: Int64(clamping: copy.byteLength), countStyle: .file))
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle(SyncText.value("sync.meguri.preview.title.\(kind)"))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(SyncText.value("sync.meguri.cancel"), role: .cancel) { sync.discardMeguriPreview() }
                        .disabled(sync.meguriBusy).accessibilityIdentifier("sync.meguriSetup.cancel")
                }
                ToolbarItem(placement: .destructiveAction) {
                    Button(SyncText.value("sync.meguri.preview.confirm.\(kind)"), role: kind == "download" ? nil : .destructive) {
                        sync.confirmMeguriPreview()
                    }
                    .disabled(sync.meguriBusy)
                    .accessibilityIdentifier("sync.meguriSetup.confirm")
                }
            }
            .interactiveDismissDisabled()
        }
    }
}

/// The one media fact the status row does not say, for media blocked: files
/// on the server that have not reached this device (T18.6). The status row
/// already gives the state, its sentence and its action; the issues row the
/// files not transferred (resume fix: one explanation, not three).
struct MeguriMediaFactRows: View {
    let collection: ReviewCollection
    static func remotePending(_ sync: SyncCoordinator) -> UInt64? {
        guard sync.meguriStage == .mediaBlocked || sync.meguriRoutine == .mediaBlocked else { return nil }
        if let accounting = sync.meguriAccounting { return accounting.remotePending > 0 ? accounting.remotePending : nil }
        if let result = sync.meguriRoundReport?.media, case .media(let media) = result.outcome,
           let awaiting = media.awaiting, awaiting > 0 { return awaiting }
        return nil
    }
    static func isVisible(_ sync: SyncCoordinator) -> Bool { remotePending(sync) != nil }
    var body: some View {
        if let pending = Self.remotePending(collection.sync) {
            Text(SyncText.format("sync.meguri.media.remotePending", Int64(clamping: pending)))
        }
    }
}

struct MeguriAffectedFilesPage: View {
    let collection: ReviewCollection
    var body: some View {
        List {
            Section {
                if let accounting = collection.sync.meguriAccounting {
                    ForEach(Array(accounting.files.filter { $0.state != .transferred }.sorted { $0.name < $1.name }.enumerated()), id: \.offset) { _, file in
                        VStack(alignment: .leading) {
                            Text(file.name)
                            Text(SyncText.value("sync.meguri.media.state.\(reason(file.exclusion))"))
                            if file.lossyName { Text(SyncText.value("sync.meguri.media.lossyName")) }
                        }
                    }
                } else if let result = collection.sync.meguriRoundReport?.media, case .media(let media) = result.outcome {
                    ForEach(Array(media.affected.sorted { $0.name < $1.name }.enumerated()), id: \.offset) { _, file in
                        LabeledContent(file.name, value: SyncText.value("sync.meguri.media.state.\(routineReason(file.kind))"))
                    }
                }
            } footer: {
                Text(SyncText.value("sync.meguri.media.list.footer"))
                if hasExcluded { Text(SyncText.value("sync.meguri.media.list.excluded")) }
            }
            // T19.5: an issues row's action is in its list (T18.6: checks the
            // media folder again and recomputes the list).
            if collection.sync.meguriRoutine == .mediaBlocked || collection.sync.meguriCanRetryMedia {
                Section {
                    Button(SyncText.value("sync.meguri.media.retry")) {
                        if collection.sync.meguriRoutine == .mediaBlocked { collection.sync.retryMeguriRoundMedia() }
                        else { collection.sync.retryMeguriMedia() }
                    }
                    .disabled(collection.sync.meguriBusy)
                    .accessibilityIdentifier("settings.sync.meguri.affectedList.retry")
                }
            }
        }
        .navigationTitle(SyncText.value("sync.meguri.media.list.title"))
        .accessibilityIdentifier("settings.sync.meguri.affectedList")
    }
    private var hasExcluded: Bool {
        if let accounting = collection.sync.meguriAccounting { return accounting.files.contains { $0.exclusion != nil } }
        if let result = collection.sync.meguriRoundReport?.media, case .media(let media) = result.outcome {
            return media.affected.contains {
                switch $0.kind { case .pending, .awaiting: false; default: true }
            }
        }
        return false
    }
    private func reason(_ exclusion: MeguriBootstrapMediaExclusion?) -> String {
        switch exclusion {
        case .empty: "empty"
        case .overLimit: "overLimit"
        case .invalidName: "invalidName"
        case .notRegularFile: "notRegularFile"
        case nil: "pending"
        }
    }
    private func routineReason(_ kind: MeguriAffectedMediaKind) -> String {
        switch kind {
        case .empty: "empty"
        case .overLimit: "overLimit"
        case .invalidName: "invalidName"
        case .notRegularFile: "notRegularFile"
        case .pending, .awaiting: "pending"
        }
    }
}
