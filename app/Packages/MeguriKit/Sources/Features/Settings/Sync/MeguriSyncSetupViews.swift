import AppModel
import CoreBridge
import SwiftUI

struct SyncServiceSection: View {
    let collection: ReviewCollection
    let host: AppNavigation.SheetHost
    @State private var destination: String?
    @State private var switchSource: SyncProviderSelection?
    private var changing: Bool {
        get { collection.syncPresentation.serviceChanging }
        nonmutating set { collection.syncPresentation.serviceChanging = newValue }
    }
    private var failed: Bool {
        get { collection.syncPresentation.serviceChangeFailed }
        nonmutating set { collection.syncPresentation.serviceChangeFailed = newValue }
    }
    @State private var recovering = false
    private var sync: SyncCoordinator { collection.sync }
    private var selected: String? {
        switch sync.providerState?.selection {
        case .iCloud: "iCloud"
        case .meguri: "meguri"
        default: nil
        }
    }
    var body: some View {
        Section {
            Picker(SyncText.value("sync.service.label"), selection: Binding<String?>(get: { selected }, set: choose)) {
                Text(SyncText.value("sync.service.iCloud")).tag("iCloud" as String?)
                Text(SyncText.value("sync.service.meguri")).tag("meguri" as String?)
            }
            .disabled(changing || sync.providerNeedsAttention)
            .accessibilityIdentifier("settings.accountSync.service")
            if changing { ProgressView(SyncText.value("sync.service.changing")) }
            if failed { Text(SyncText.value("sync.service.changeFailed")) }
            if collection.syncPresentation.meguriSigningOut { ProgressView(SyncText.value("sync.meguri.signingOut")) }
            if collection.syncPresentation.meguriSignOutFailed { Text(SyncText.value("sync.meguri.signOut.failed")) }
            if sync.providerState?.phase == .blocked {
                Text(SyncText.value("sync.service.blocked"))
            } else if sync.providerState?.phase == .handoffRecovery {
                Text(SyncText.value("sync.service.recovery"))
                Button(SyncText.value("sync.service.recovery.action")) { recovering = true }
                    .disabled(changing)
                    .accessibilityIdentifier("settings.accountSync.serviceRecovery")
            }
        } footer: {
            if selected == nil { Text(SyncText.value("sync.service.none.footer")) }
        }
        .sheet(isPresented: Binding(get: { destination != nil }, set: { if !$0 && !changing { destination = nil } })) {
            if let destination, let switchSource {
                SyncServiceSwitchSheet(collection: collection, source: switchSource, destination: destination, changing: changing,
                    failed: failed, cancel: { self.destination = nil }, confirm: { change(destination) })
            }
        }
        .alert(SyncText.value("sync.service.recovery.title"), isPresented: $recovering) {
            Button(SyncText.value("sync.service.recovery.confirm"), role: .destructive) {
                changing = true
                Task { failed = !(await sync.recoverProviderHandoffToLocalOnly()); changing = false }
            }
            Button(SyncText.value("sync.service.cancel"), role: .cancel) {}
        } message: { Text(SyncText.value("sync.service.recovery.message")) }
    }
    private func choose(_ value: String?) {
        guard let value, value != selected else { return }
        if selected != nil { switchSource = sync.providerState?.selection; destination = value }
        else { change(value) }
    }
    private func change(_ value: String) {
        let hadService = selected != nil
        changing = true; failed = false
        Task {
            do {
                try await collection.prepareSyncServiceSelection()
                let current = collection.sync
                let target: SyncProviderSelection = value == "meguri" ? .meguri : value == "iCloud" ? .iCloud : .localOnly
                let succeeded = await current.changeProvider(to: target,
                    removeMeguriCredentials: current.providerState?.selection == .meguri)
                failed = !succeeded
                if succeeded {
                    destination = nil
                    if !hadService, value == "iCloud" { collection.presentCloudSync(.connect, host: host) }
                }
            } catch { failed = true }
            changing = false
        }
    }
}

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
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    if source == .iCloud, sync.providerState?.selection == .iCloud, let status = sync.status {
                        if status.pending > 0 { Text(SyncText.format("sync.switch.pending", Int64(clamping: status.pending), oldService)) }
                        if status.heldBack > 0 { Text(SyncText.format("sync.switch.heldBack", Int64(clamping: status.heldBack), oldService)) }
                        if status.pending == 0 && status.heldBack == 0 { Text(SyncText.format("sync.switch.pending.none", oldService)) }
                        if let files = SyncStatusCopy.sum(status.media.uploadFiles, status.media.downloadFiles) {
                            if files > 0 { Text(SyncText.format("sync.switch.media", Int64(clamping: files), oldService)) }
                        } else { Text(SyncText.format("sync.switch.media.unknown", oldService)) }
                    } else {
                        Text(SyncText.format(source == .meguri && sync.meguriRoutine == .pending ? "sync.switch.pending.some" : "sync.switch.pending.unknown", oldService))
                        if let media = sync.meguriRoundReport?.media, case .media(let facts) = media.outcome,
                           let excluded = facts.excluded, let pending = facts.pending, let awaiting = facts.awaiting,
                           let local = SyncStatusCopy.sum(excluded, pending), let count = SyncStatusCopy.sum(local, awaiting) {
                            if count > 0 { Text(SyncText.format("sync.switch.media", Int64(clamping: count), oldService)) }
                        } else if let accounting = sync.meguriAccounting {
                            let local = UInt64(accounting.files.filter { $0.state != .transferred }.count)
                            if let count = SyncStatusCopy.sum(local, accounting.remotePending), count > 0 {
                                Text(SyncText.format("sync.switch.media", Int64(clamping: count), oldService))
                            }
                        } else { Text(SyncText.format("sync.switch.media.unknown", oldService)) }
                    }
                    if sync.meguriStage == .acceptanceUnknown || sync.meguriRoutine == .acceptanceUnknown {
                        Text(SyncText.value("sync.switch.acceptanceUnknown"))
                    }
                    Text(SyncText.format("sync.switch.kept", oldService))
                    if destination != "localOnly" { Text(SyncText.value("sync.switch.otherDevices")) }
                    if source == .meguri && !failed { Text(SyncText.value("sync.switch.signOut")) }
                    Text(SyncText.value("sync.switch.next.\(destination)"))
                    if !changing && !sync.serviceChangeAvailable { Text(SyncText.value("sync.switch.busy")) }
                    if changing { ProgressView(SyncText.value("sync.service.changing")) }
                    if failed { Text(SyncText.value("sync.service.changeFailed")) }
                }
            }
            .formStyle(.grouped)
            .navigationTitle(SyncText.value("sync.switch.title.\(destination)"))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(SyncText.value("sync.service.cancel"), role: .cancel, action: cancel)
                        .disabled(changing).accessibilityIdentifier("sync.serviceSwitch.cancel")
                }
                ToolbarItem(placement: .destructiveAction) {
                    Button(SyncText.value("sync.switch.confirm.\(destination)"), role: .destructive, action: confirm)
                        .disabled(changing || !sync.serviceChangeAvailable)
                        .accessibilityIdentifier("sync.serviceSwitch.confirm")
                }
            }
            .interactiveDismissDisabled(changing)
        }
    }
}

struct MeguriSyncSections: View {
    let collection: ReviewCollection
    @State private var address = ""
    @State private var username = ""
    @State private var password = ""
    @State private var signOut = false
    private var signingOut: Bool {
        get { collection.syncPresentation.meguriSigningOut }
        nonmutating set { collection.syncPresentation.meguriSigningOut = newValue }
    }
    private var signOutFailed: Bool {
        get { collection.syncPresentation.meguriSignOutFailed }
        nonmutating set { collection.syncPresentation.meguriSignOutFailed = newValue }
    }
    @State private var stopping = false
    private var changing: Bool {
        get { collection.syncPresentation.serviceChanging }
        nonmutating set { collection.syncPresentation.serviceChanging = newValue }
    }
    private var changeFailed: Bool {
        get { collection.syncPresentation.serviceChangeFailed }
        nonmutating set { collection.syncPresentation.serviceChangeFailed = newValue }
    }
    private var sync: SyncCoordinator { collection.sync }
    private var signedIn: Bool { sync.meguriPhase == .setupRequired && sync.meguriRoutine != .authentication }
    var body: some View {
        Section {
            SyncStatusRows(collection: collection)
            if sync.meguriBusy {
                if let progress = sync.meguriProgress, let total = progress.total, total > 0 {
                    ProgressView(value: Double(progress.completed), total: Double(total))
                } else { ProgressView() }
            }
            if signedIn {
                LabeledContent(SyncText.value("sync.meguri.address"), value: sync.meguriEndpoint)
                Button(SyncText.value("sync.meguri.signOut")) { signOut = true }
                    .disabled(sync.meguriBusy || signingOut)
                    .accessibilityIdentifier("settings.accountSync.meguri.signOut")
            } else {
                TextField(SyncText.value("sync.meguri.address"), text: Binding(get: { address }, set: {
                    address = $0; password = ""; sync.meguriAddressEdited()
                }))
                    #if os(iOS)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    #endif
                    .autocorrectionDisabled()
                    .accessibilityIdentifier("settings.accountSync.meguri.address")
                TextField(SyncText.value("sync.meguri.username"), text: $username)
                    .textContentType(.username)
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    #endif
                    .autocorrectionDisabled()
                    .accessibilityIdentifier("settings.accountSync.meguri.username")
                SecureField(SyncText.value("sync.meguri.password"), text: $password)
                    .textContentType(.password)
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    #endif
                    .autocorrectionDisabled()
                    .onSubmit(signInAction)
                    .accessibilityIdentifier("settings.accountSync.meguri.password")
                Button(SyncText.value("sync.meguri.signIn"), action: signInAction)
                    .disabled(sync.meguriBusy || sync.settings.paused || address.isEmpty || username.isEmpty || password.isEmpty)
                    .accessibilityIdentifier("settings.accountSync.meguri.signIn")
            }
            if let key = sync.meguriSignInErrorKey { Text(SyncText.value(key)) }
            if sync.meguriPhase == .blocked {
                Button(SyncText.value("sync.meguri.connection.retry")) { sync.loadMeguriConnection() }
                    .disabled(sync.meguriBusy)
                    .accessibilityIdentifier("settings.accountSync.meguri.retryConnection")
                Button(SyncText.value("sync.meguri.signOut")) { signOut = true }
                    .disabled(sync.meguriBusy || signingOut)
                    .accessibilityIdentifier("settings.accountSync.meguri.signOut")
            }
        } header: { Text(SyncText.value("sync.service.meguri")) }
          footer: { if !signedIn { Text(SyncText.value("sync.meguri.signIn.footer")) } }
        .disabled(signingOut)
        .onAppear { address = sync.meguriEndpoint }
        .onChange(of: sync.meguriEndpoint) { _, endpoint in address = endpoint; password = "" }
        .onDisappear { password = "" }
        .alert(SyncText.value("sync.meguri.signOut.title"), isPresented: $signOut) {
            Button(SyncText.value("sync.meguri.signOut.confirm"), role: .destructive) {
                password = ""; signingOut = true; signOutFailed = false
                Task { signOutFailed = !(await sync.removeMeguriConnection()); signingOut = false }
            }
            Button(SyncText.value("sync.service.cancel"), role: .cancel) {}
        } message: { Text(SyncText.value("sync.meguri.signOut.message")) }
        Section {
            if sync.meguriCanChoose {
                if collection.lifecycle == .noCollection {
                    Button(SyncText.value("sync.meguri.setup.download")) { sync.prepareMeguri(.useServerCollection) }
                        .accessibilityIdentifier("settings.accountSync.meguri.download")
                } else {
                    Button(SyncText.value("sync.meguri.setup.useDevice")) { sync.prepareMeguri(.useDeviceCollection) }
                        .accessibilityIdentifier("settings.accountSync.meguri.useDevice")
                    Button(SyncText.value("sync.meguri.setup.useServer")) { sync.prepareMeguri(.useServerCollection) }
                        .accessibilityIdentifier("settings.accountSync.meguri.useServer")
                }
            }
            if sync.meguriCanFinish {
                Button(SyncText.value("sync.meguri.setup.finish")) { sync.finishMeguriSetup() }
                    .accessibilityIdentifier("settings.accountSync.meguri.finishSetup")
            }
            if sync.meguriStage == .preparing && sync.meguriBusy {
                Button(SyncText.value("sync.meguri.cancel"), role: .cancel) { sync.cancelMeguriPreparation() }
                    .accessibilityIdentifier("settings.accountSync.meguri.cancelSetup")
            }
            if MeguriMediaFactRows.isVisible(sync) {
                MeguriMediaRows(collection: collection)
            }
            if sync.meguriCanRetryMedia {
                Button(SyncText.value("sync.meguri.media.retry")) { sync.retryMeguriMedia() }
                    .accessibilityIdentifier("settings.accountSync.meguri.retryMedia")
            }
            if sync.meguriRoutine != nil || sync.settings.paused {
                Button(SyncText.value(sync.settings.paused ? "sync.meguri.resume" : "sync.meguri.pause")) {
                    let settings = sync.settings
                    sync.setDeviceSettings(paused: !settings.paused, automatic: settings.automatic, media: settings.media)
                }
                .accessibilityIdentifier("settings.accountSync.meguri.pause")
                if sync.meguriRoutine == .failed,
                   sync.meguriRoundReport?.collection.finalization != nil || sync.meguriRoundReport?.media?.finalization != nil {
                    Button(SyncText.value("sync.meguri.media.retry")) { sync.retryMeguriFinalization() }
                        .disabled(sync.meguriBusy)
                }
            }
            switch sync.meguriStatus.route {
            case .account: EmptyView()
            case .round, .disabled: SyncNowButton(collection: collection, identifier: "settings.accountSync.meguri.syncNow")
            }
            if !sync.providerNeedsAttention && sync.meguriPhase != .blocked {
                Button(SyncText.value("sync.meguri.stop")) { stopping = true }
                    .disabled(sync.meguriBusy || changing)
                    .accessibilityIdentifier("settings.accountSync.meguri.stop")
            }
        }
        .sheet(isPresented: Binding(get: { sync.meguriPreview != nil }, set: { if !$0 { sync.discardMeguriPreview() } })) {
            if let preview = sync.meguriPreview {
                MeguriSetupPreviewSheet(collection: collection, preview: preview)
            }
        }
        .sheet(isPresented: $stopping) {
            SyncServiceSwitchSheet(collection: collection, source: .meguri, destination: "localOnly", changing: changing,
                failed: changeFailed, cancel: { stopping = false }, confirm: {
                    changing = true; changeFailed = false
                    Task {
                        let succeeded = await sync.removeMeguriConnection()
                        changeFailed = !succeeded; changing = false
                        if succeeded { stopping = false }
                    }
                })
        }
    }
    private func signInAction() {
        let secret = password; password = ""
        guard !sync.meguriBusy, !sync.settings.paused, !address.isEmpty, !username.isEmpty, !secret.isEmpty else { return }
        sync.signInMeguri(endpoint: address, username: username, password: secret)
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
                    .disabled(sync.meguriBusy || sync.settings.paused)
                    .accessibilityIdentifier("sync.meguriSetup.confirm")
                }
            }
            .interactiveDismissDisabled()
        }
    }
}

/// Actual media facts shared by Settings and the status popover; contains no actions.
struct MeguriMediaFactRows: View {
    let collection: ReviewCollection
    private var sync: SyncCoordinator { collection.sync }
    static func isVisible(_ sync: SyncCoordinator) -> Bool {
        if sync.meguriStage == .mediaBlocked || sync.meguriRoutine == .mediaBlocked { return true }
        if sync.meguriAccounting?.files.contains(where: { $0.state != .transferred }) == true { return true }
        if let result = sync.meguriRoundReport?.media, case .media(let media) = result.outcome { return !media.affected.isEmpty }
        return false
    }
    var body: some View {
        if let accounting = sync.meguriAccounting {
            let affected = accounting.files.filter { $0.state != .transferred }
            if !affected.isEmpty { Text(SyncText.format("sync.meguri.media.notTransferred", Int64(affected.count))) }
            if accounting.remotePending > 0 { Text(SyncText.format("sync.meguri.media.remotePending", Int64(clamping: accounting.remotePending))) }
        } else if let result = sync.meguriRoundReport?.media, case .media(let media) = result.outcome {
            if let pending = media.pending, let excluded = media.excluded, let count = SyncStatusCopy.sum(pending, excluded), count > 0 {
                Text(SyncText.format("sync.meguri.media.notTransferred", Int64(clamping: count)))
            }
            if let awaiting = media.awaiting, awaiting > 0 { Text(SyncText.format("sync.meguri.media.remotePending", Int64(clamping: awaiting))) }
            if media.accountingFailure != nil { Text(SyncText.value("sync.meguri.media.unverified")) }
        } else { Text(SyncText.value("sync.meguri.media.unverified")) }
        if sync.meguriStage != nil { Text(SyncText.value("sync.meguri.media.setupWaits")) }
        Text(SyncText.value("sync.meguri.media.study"))
    }
}

struct MeguriMediaRows: View {
    let collection: ReviewCollection
    private var sync: SyncCoordinator { collection.sync }
    var body: some View {
        MeguriMediaFactRows(collection: collection)
        if sync.meguriAccounting?.files.contains(where: { $0.state != .transferred }) == true
            || !(sync.meguriRoundReport?.media.map { result in
                if case .media(let media) = result.outcome { return media.affected.isEmpty }
                return true
            } ?? true) {
            Button(SyncText.value("sync.meguri.media.showAffected")) { collection.syncPresentation.meguriAffectedFilesPresented = true }
                .accessibilityIdentifier("settings.accountSync.meguri.showAffected")
        }
        if sync.meguriRoutine == .mediaBlocked {
            Button(SyncText.value("sync.meguri.media.retry")) { sync.retryMeguriRoundMedia() }
                .disabled(sync.meguriBusy || sync.settings.paused)
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
        }
        .navigationTitle(SyncText.value("sync.meguri.media.list.title"))
        .accessibilityIdentifier("settings.accountSync.meguri.affectedList")
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
