import AppModel
import CoreBridge
import SwiftUI

/// T21.1, N15.7: the service change confirmation, in the platform's form for a
/// destructive choice, as Delete Deck asks: an alert on the Mac's Settings
/// window; on touch the system confirmation dialog from the Service control.
/// Cancel has the cancel role (Esc); the destructive button is never the
/// default, so Return confirms nothing. Choosing it closes the question; the
/// change's progress and failure show under the Service control.
struct ServiceSwitchQuestion: ViewModifier {
    let collection: ReviewCollection
    @Binding var destination: String?
    let source: SyncProviderSelection?
    let confirm: (_ destination: String, _ shown: SyncProviderSelection) -> Void
    private var sync: SyncCoordinator { collection.sync }
    private var question: (confirmation: ServiceSwitchConfirmation, source: SyncProviderSelection)? {
        guard let destination, let source else { return nil }
        return (ServiceSwitchConfirmation(source: source, destination: destination,
                                          facts: sync.serviceSwitchFacts(source: source)), source)
    }
    func body(content: Content) -> some View {
        let question = question
        let title = Text(verbatim: question.map { SyncText.value($0.confirmation.titleKey) } ?? "")
        let message = Text(verbatim: question.map { $0.confirmation.message(source: $0.source) } ?? "")
        let presented = Binding(get: { destination != nil }, set: { if !$0 { destination = nil } })
        #if os(macOS)
        content.alert(title, isPresented: presented) {
            Button(SyncText.value("sync.service.cancel"), role: .cancel) { destination = nil }
                .accessibilityIdentifier("sync.serviceSwitch.cancel")
            confirmButton(question)
        } message: { message }
        #else
        content.confirmationDialog(title, isPresented: presented, titleVisibility: .visible) {
            confirmButton(question)
            Button(SyncText.value("sync.service.cancel"), role: .cancel) { destination = nil }
                .accessibilityIdentifier("sync.serviceSwitch.cancel")
        } message: { message }
        #endif
    }
    private func confirmButton(_ question: (confirmation: ServiceSwitchConfirmation, source: SyncProviderSelection)?) -> some View {
        Button(question.map { SyncText.value($0.confirmation.confirmKey) } ?? "", role: .destructive) {
            guard let destination, let source else { return }
            self.destination = nil
            confirm(destination, source)
        }
        .disabled(!sync.serviceChangeAvailable)
        .accessibilityIdentifier("sync.serviceSwitch.confirm")
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
