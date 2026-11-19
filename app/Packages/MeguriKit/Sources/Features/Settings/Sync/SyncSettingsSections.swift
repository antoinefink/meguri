import AppModel
import CoreBridge
import SwiftUI

/// N15.2: the Sync section of Settings, as system form groups under one header.
struct SyncSettingsSections: View {
    let collection: ReviewCollection
    var host: AppNavigation.SheetHost = .main
    /// The sign-in fields' text, owned above any layout that rebuilds this section.
    var draft: MeguriSignInDraft
    private var sync: SyncCoordinator { collection.sync }
    private var selection: SyncProviderSelection? {
        switch sync.providerState?.selection {
        case .iCloud: .iCloud
        case .meguri: .meguri
        case .localOnly, nil: nil
        }
    }
    var body: some View {
        Section {
            SyncServiceRows(collection: collection, host: host)
            if selection == .meguri { MeguriAccountRows(collection: collection, draft: draft) }
            if selection != nil || sync.providerNeedsAttention {
                SyncMainStatusRows(collection: collection, host: host)
            }
            if selection != nil {
                Toggle(SyncText.value("sync.settings.automatic"), isOn: Binding(
                    get: { sync.automaticSwitchValue }, set: { sync.setAutomatic($0) }))
                    .disabled(!sync.automaticSwitchAvailable)
                    .accessibilityIdentifier("settings.sync.automatic")
                SyncNowButton(collection: collection, identifier: "settings.sync.syncNow")
            }
        } header: {
            Text(SyncText.value("sync.settings.sync.section"))
                .accessibilityIdentifier("settings.sync.heading")
        } footer: {
            Text(SyncText.value(selection == nil ? "sync.service.none.footer" : "sync.settings.automatic.footer"))
                .accessibilityIdentifier("settings.sync.footer")
        }
        if selection != nil {
            Section {
                NavigationLink {
                    SyncTroubleshootingPage(collection: collection, host: host)
                } label: { Text(SyncText.value("sync.settings.troubleshooting")) }
                    .accessibilityIdentifier("settings.sync.troubleshooting")
            } footer: {
                if selection == .iCloud {
                    Text(SyncText.value("sync.settings.privacy"))
                        .accessibilityIdentifier("settings.sync.privacyFooter")
                }
            }
        }
    }
}

/// What the person typed into the Meguri Sync sign-in fields. The Settings page
/// owns it, so a layout change that rebuilds the section keeps the text. The
/// password leaves it at submission and when the page goes away.
@Observable public final class MeguriSignInDraft {
    public var address = ""
    public var username = ""
    public var password = ""
    public init() {}
}

/// The service's name; iCloud carries its Beta label as separate text (T19.1).
struct SyncServiceName: View {
    let selection: SyncProviderSelection?
    var body: some View {
        switch selection {
        case .iCloud:
            HStack(alignment: .firstTextBaseline) {
                Text(SyncText.value("sync.service.iCloud.name"))
                Text(SyncText.value("sync.service.beta")).font(.caption).foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(SyncText.value("sync.service.iCloud.accessibility"))
        case .meguri: Text(SyncText.value("sync.service.meguri"))
        case .localOnly, nil: Text(SyncText.value("sync.service.off"))
        }
    }
}

/// N15.2 row 1. The picker shows the service the core reports, never the value just tapped.
struct SyncServiceRows: View {
    let collection: ReviewCollection
    let host: AppNavigation.SheetHost
    @State private var destination: String?
    @State private var switchSource: SyncProviderSelection?
    @State private var refusedForOp = false
    private var changing: Bool {
        get { collection.syncPresentation.serviceChanging }
        nonmutating set { collection.syncPresentation.serviceChanging = newValue }
    }
    private var failed: Bool {
        get { collection.syncPresentation.serviceChangeFailed }
        nonmutating set { collection.syncPresentation.serviceChangeFailed = newValue }
    }
    private var sync: SyncCoordinator { collection.sync }
    private var selected: String {
        switch sync.providerState?.selection {
        case .iCloud: "iCloud"
        case .meguri: "meguri"
        case .localOnly, nil: "localOnly"
        }
    }
    /// V27.5: refused while an op could restore the binding before, or could not be read.
    private var opBlocksChange: Bool { sync.serviceChangeRefusedForOperation }
    var body: some View {
        Picker(SyncText.value("sync.service.control"), selection: Binding(get: { selected }, set: choose)) {
            Text(SyncText.value("sync.service.off")).tag("localOnly")
                .accessibilityIdentifier("settings.sync.service.off")
            Text(SyncText.value("sync.service.meguri")).tag("meguri")
                .accessibilityIdentifier("settings.sync.service.meguri")
            // A menu item is one line of text: the label follows the name.
            Text(verbatim: "\(SyncText.value("sync.service.iCloud.name")) (\(SyncText.value("sync.service.beta")))")
                .accessibilityLabel(SyncText.value("sync.service.iCloud.accessibility"))
                .accessibilityIdentifier("settings.sync.service.iCloud")
                .tag("iCloud")
        }
        // MP6.5: the control stays enabled while the provider is blocked.
        .disabled(changing || sync.providerState?.phase == .handoffRecovery || collection.meguriSignInPreparing || sync.meguriBusy)
        .accessibilityIdentifier("settings.sync.service")
        .sheet(isPresented: Binding(get: { destination != nil }, set: { if !$0 && !changing { destination = nil } })) {
            if let destination, let switchSource {
                SyncServiceSwitchSheet(collection: collection, source: switchSource, destination: destination, changing: changing,
                    failed: failed, cancel: { self.destination = nil }, confirm: { change(destination) })
            }
        }
        if changing {
            ProgressView(SyncText.value("sync.service.changing"))
                .accessibilityIdentifier("settings.sync.serviceChanging")
        }
        if failed {
            Text(SyncText.value("sync.service.changeFailed")).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.serviceChangeFailed")
        }
        if refusedForOp, opBlocksChange {
            Text(SyncText.value("sync.service.opFirst")).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.serviceRefused")
        } else if !changing, !sync.serviceChangeAvailable, selected != "localOnly" {
            Text(SyncText.value("sync.switch.busy")).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.serviceBusy")
        }
        if sync.providerState?.phase == .blocked {
            Text(SyncText.value("sync.service.blocked")).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.serviceBlocked")
        }
    }
    private func choose(_ value: String) {
        guard !collection.meguriSignInPreparing, !sync.meguriBusy, value != selected else { return }
        refusedForOp = false
        if opBlocksChange { refusedForOp = true; return }
        if selected != "localOnly" { switchSource = sync.providerState?.selection; destination = value }
        else { change(value) }
    }
    private func change(_ value: String) {
        let source = sync.providerState?.selection
        changing = true; failed = false
        Task {
            do {
                try await collection.prepareSyncServiceSelection()
                let current = collection.sync
                // A choice in the Service control clears the update hold, by writing
                // `paused: false` before the change itself is asked for (P25.2).
                guard await current.clearUpdateHold() else { failed = true; changing = false; return }
                let succeeded: Bool
                if value == "localOnly", source == .iCloud {
                    // V27.5's rows, through the retained operations.
                    succeeded = await current.turnOffICloud()
                } else {
                    let target: SyncProviderSelection = value == "meguri" ? .meguri : value == "iCloud" ? .iCloud : .localOnly
                    succeeded = await current.changeProvider(to: target, removeMeguriCredentials: source == .meguri)
                }
                failed = !succeeded
                if succeeded {
                    destination = nil
                    if value == "iCloud" { collection.presentCloudSync(.connect, host: host) }
                }
            } catch { failed = true }
            changing = false
        }
    }
}

/// N15.2 row 2 and MP6.1: the sign-in fields until signed in, then one Account row.
struct MeguriAccountRows: View {
    let collection: ReviewCollection
    let draft: MeguriSignInDraft
    @State private var choosing = false
    private var address: String { get { draft.address } nonmutating set { draft.address = newValue } }
    private var username: String { get { draft.username } nonmutating set { draft.username = newValue } }
    private var password: String { get { draft.password } nonmutating set { draft.password = newValue } }
    private var sync: SyncCoordinator { collection.sync }
    private var changing: Bool { collection.syncPresentation.serviceChanging }
    private var signedIn: Bool { sync.meguriPhase == .setupRequired && sync.meguriRoutine != .authentication }
    private var processing: Bool { collection.meguriSignInPreparing || sync.meguriBusy }
    private var showsFields: Bool { !signedIn || sync.mainStatus.action == .meguriSignIn }
    var body: some View {
        Group {
            if showsFields {
                Group {
                    TextField(SyncText.value("sync.meguri.address"), text: Binding(get: { address }, set: {
                        guard !processing else { return }
                        address = $0; password = ""; sync.meguriAddressEdited()
                    }))
                        #if os(iOS)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        #endif
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("settings.sync.meguri.address")
                    TextField(SyncText.value("sync.meguri.username"), text: Binding(get: { username }, set: {
                        if !processing { username = $0 }
                    }))
                        .textContentType(.username)
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        #endif
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("settings.sync.meguri.username")
                    SecureField(SyncText.value("sync.meguri.password"), text: Binding(get: { password }, set: {
                        if !processing { password = $0 }
                    }))
                        .textContentType(.password)
                        .onSubmit(signIn)
                        .accessibilityIdentifier("settings.sync.meguri.password")
                }
                .disabled(processing || changing)
                Button(SyncText.value("sync.meguri.signIn"), action: signIn)
                    .buttonStyle(.borderedProminent)
                    .disabled(processing || changing || address.isEmpty || username.isEmpty || password.isEmpty)
                    .accessibilityIdentifier("settings.sync.meguri.signIn")
                Text(SyncText.value(sync.storedAutomatic ? "sync.meguri.signIn.footer.setup" : "sync.meguri.signIn.footer.setup.manual"))
                    .font(.footnote).foregroundStyle(.secondary)
                    .accessibilityIdentifier("settings.sync.meguri.signInFooter")
                if let key = sync.meguriSignInErrorKey {
                    Label(SyncText.value(key), systemImage: "exclamationmark.circle")
                        .accessibilityIdentifier("settings.sync.meguri.signInError")
                }
            } else {
                // N15.2 row 2: the username is the row's value; the page has both facts.
                NavigationLink {
                    Form {
                        LabeledContent(SyncText.value("sync.meguri.address"), value: sync.meguriEndpoint)
                            .accessibilityIdentifier("settings.sync.meguri.account.address")
                        LabeledContent(SyncText.value("sync.meguri.username"), value: sync.meguriUsername ?? "")
                            .accessibilityIdentifier("settings.sync.meguri.account.username")
                    }
                    .formStyle(.grouped)
                    .navigationTitle(SyncText.value("sync.settings.account"))
                    .accessibilityIdentifier("settings.sync.meguri.account.content")
                } label: {
                    LabeledContent(SyncText.value("sync.settings.account"), value: sync.meguriUsername ?? "")
                }
                .accessibilityIdentifier("settings.sync.meguri.account")
            }
            if collection.meguriSignInPreparing {
                ProgressView(SyncText.value("sync.meguri.signIn.preparing"))
                    .accessibilityIdentifier("settings.sync.meguri.signInProgress")
            } else if sync.meguriBusy {
                if let progress = sync.meguriProgress, let total = progress.total, total > 0 {
                    ProgressView(value: Double(progress.completed), total: Double(total))
                } else { ProgressView() }
            }
        }
        .onAppear { if address.isEmpty { address = sync.meguriEndpoint }; startSetupIfAsked() }
        .onChange(of: sync.meguriEndpoint) { _, endpoint in address = endpoint; password = "" }
        .onChange(of: sync.meguriCanChoose) { _, _ in startSetupIfAsked() }
        .sheet(isPresented: Binding(
            get: { choosing || collection.syncPresentation.keepCollectionRequested },
            set: { choosing = $0; if !$0 { collection.syncPresentation.keepCollectionRequested = false } })) {
            MeguriKeepCollectionSheet(collection: collection) {
                choosing = false; collection.syncPresentation.keepCollectionRequested = false
            }
        }
        .sheet(isPresented: Binding(get: { sync.meguriPreview != nil && !choosing }, set: { _ in })) {
            if let preview = sync.meguriPreview { MeguriSetupPreviewSheet(collection: collection, preview: preview) }
        }
    }
    private func signIn() {
        let secret = password; password = ""
        guard !processing, !changing, !address.isEmpty, !username.isEmpty, !secret.isEmpty else { return }
        Task {
            // Signing in to set the service up is a service choice: it clears the hold.
            guard await sync.clearUpdateHold() else { return }
            collection.signInMeguri(endpoint: address, username: username, password: secret)
        }
    }
    /// MP6.3: signing in is the consent to set the service up. With no
    /// collection on this device only a download can run; otherwise Meguri asks.
    private func startSetupIfAsked() {
        guard sync.meguriCanChoose, sync.meguriStage == .required, sync.meguriPreview == nil else { return }
        if collection.lifecycle == .noCollection { sync.prepareMeguri(.useServerCollection) }
        else { choosing = true }
    }
}

/// N15.3 for Meguri Sync. The core's preview of the chosen direction follows
/// and keeps the bound confirmation (MP4), so nothing is replaced by this sheet alone.
struct MeguriKeepCollectionSheet: View {
    let collection: ReviewCollection
    let close: () -> Void
    @State private var cancelling = false
    private var sync: SyncCoordinator { collection.sync }
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text(SyncText.value("sync.keep.message"))
                        .accessibilityIdentifier("sync.keepCollection.message")
                    LabeledContent(SyncText.value("sync.meguri.preview.server"), value: sync.meguriEndpoint)
                }
                Section {
                    Button(SyncText.value("sync.keep.device"), role: .destructive) {
                        close(); sync.prepareMeguri(.useDeviceCollection)
                    }
                    .accessibilityIdentifier("sync.keepCollection.keepDevice")
                } footer: {
                    VStack(alignment: .leading) {
                        Text(SyncText.value("sync.keep.device.effect.meguri"))
                        Text(SyncText.value("sync.meguri.preview.otherClients"))
                    }
                }
                Section {
                    Button(SyncText.value("sync.keep.service.meguri"), role: .destructive) {
                        close(); sync.prepareMeguri(.useServerCollection)
                    }
                    .accessibilityIdentifier("sync.keepCollection.keepService")
                } footer: { Text(SyncText.value("sync.keep.service.effect")) }
                Section {} footer: {
                    VStack(alignment: .leading) {
                        Text(SyncText.value("sync.meguri.preview.media"))
                        if sync.storedAutomatic { Text(SyncText.value("sync.meguri.preview.automatic")) }
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle(SyncText.value("sync.keep.title"))
            #if !os(macOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(SyncText.value("sync.service.cancel"), role: .cancel) {
                        // MP6.3: Cancel while no binding is ready leaves the service Off.
                        guard !sync.serviceConnected else { close(); return }
                        cancelling = true
                        Task {
                            let removed = await sync.removeMeguriConnection()
                            collection.syncPresentation.serviceChangeFailed = !removed
                            cancelling = false; close()
                        }
                    }
                    .disabled(cancelling)
                    .accessibilityIdentifier("sync.keepCollection.cancel")
                }
            }
            .disabled(cancelling || !sync.meguriCanChoose)
            .interactiveDismissDisabled()
        }
        .accessibilityIdentifier("sync.keepCollection.sheet")
    }
}

/// N15.2 row 3 and N15.4: the status, its one action, and the issues row.
struct SyncMainStatusRows: View {
    let collection: ReviewCollection
    var host: AppNavigation.SheetHost = .main
    /// The compact status sheet hands its destination over after it dismisses.
    var openEntry: ((CloudSyncEntry, AppNavigation.SheetHost) -> Void)? = nil
    /// Set outside Settings (the status popover): Meguri's setup sheets belong to
    /// the Sync section, so its actions open that section, asking for N15.3's sheet or not.
    var showInSettings: ((_ keepCollection: Bool) -> Void)? = nil
    @Environment(\.appClock) private var clock
    @State private var recovering = false
    private var sync: SyncCoordinator { collection.sync }
    private var status: SyncMainStatus { sync.mainStatus }
    private var meguri: Bool { sync.providerState?.selection == .meguri }
    private var detail: String? {
        let status = status
        if let key = status.detailKey {
            if let count = status.detailCount { return SyncText.format(key, Int64(clamping: count)) }
            return SyncText.value(key)
        }
        // Both services use "Last synced" for a completed exchange (MP6.6).
        if status.titleKey == "sync.status.upToDate.title" {
            let instant: Int64? = meguri ? sync.meguriLastExchange : {
                if case .upToDate(let at) = sync.status?.state { return at }
                return nil
            }()
            if let instant { return SyncText.format("sync.status.upToDate.detail", SyncText.instant(instant, now: clock.now())) }
        }
        return nil
    }
    var body: some View {
        let status = status
        VStack(alignment: .leading) {
            Label(SyncText.value(status.titleKey), systemImage: status.symbol)
            if let detail { Text(detail).font(.footnote).foregroundStyle(.secondary) }
            if !meguri, let cloud = sync.status {
                ForEach(Array(SyncStatusCopy.lines(cloud, now: clock.now()).enumerated()), id: \.offset) { _, line in
                    Text(line).font(.footnote).foregroundStyle(.secondary)
                }
            }
            if meguri, sync.meguriStage == .media, let progress = sync.meguriMediaProgress {
                Text(SyncText.format("sync.meguri.media.filesLeft", Int64(clamping: progress.awaiting)))
                    .font(.footnote).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("settings.sync.status")
        #if DEBUG
        .accessibilityValue([DebugCloudDiagnostics.processFetchValue, sync.debugPolicyStatusValue, DebugCloudDiagnostics.latestValue]
            .compactMap { $0 }.joined(separator: " "))
        #endif
        if let failure = sync.connectionFailure {
            Text(SyncText.value(SyncStatusPresentation.connectionErrorKey(failure))).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.connectionError")
        }
        if let action = status.action { actionButton(action) }
        if status.hasIssues {
            NavigationLink {
                if meguri { MeguriAffectedFilesPage(collection: collection) }
                else { SyncIssuesPage(collection: collection) }
            } label: {
                if let key = status.issuesRowKey, let count = meguriAffectedCount {
                    Text(SyncText.format(key, Int64(clamping: count)))
                } else {
                    Text(SyncText.format("sync.issues.row", Int64(status.issues.count)))
                }
            }
            .accessibilityIdentifier("settings.sync.issues")
        }
    }
    private var meguriAffectedCount: UInt64? {
        if let accounting = sync.meguriAccounting {
            return UInt64(accounting.files.filter { $0.state != .transferred }.count)
        }
        if let result = sync.meguriRoundReport?.media, case .media(let media) = result.outcome { return UInt64(media.affected.count) }
        return nil
    }
    /// One button, only for an action that can run now (N15.2).
    @ViewBuilder private func actionButton(_ action: SyncStatusAction) -> some View {
        let title = SyncText.value(action.titleKey)
        let id = "settings.sync.statusAction"
        switch action {
        case .chooseCollection:
            if meguri {
                if sync.meguriCanChoose {
                    Button(title) { askKeepCollection() }
                        .accessibilityIdentifier(id)
                }
            } else {
                Button(title) {
                    open(sync.status?.controls.decisionRequired == true ? .publicationChoice : .connect)
                }
                .accessibilityLabel(SyncText.value("sync.decision.action.label"))
                .disabled(collection.cloudPreviewEntryUnavailableReason(.connect) != nil)
                .accessibilityIdentifier(id)
            }
        case .setUpAgain, .resume:
            Button(title) { open(.connect) }
                .disabled(collection.cloudPreviewEntryUnavailableReason(.connect) != nil)
                .accessibilityIdentifier(id)
        case .tryAgain:
            Button(title) { collection.requestSyncNow() }
                .disabled(collection.syncNowAvailability() != .available || collection.navigation.sheet != nil)
                .accessibilityIdentifier(id)
        case .cancelOp:
            if let recovery = sync.recovery {
                CloudRecoveryCancelButton(collection: collection, recovery: recovery, label: title, identifier: id)
            }
        case .resumeOp:
            if let recovery = sync.recovery {
                // `resume_recovery_op` clears the update hold first (P25.2).
                Button(title) { Task { await sync.resumeRecoveryOperation() } }
                    .disabled(recovery.switchConflict || recovery.step == .cancelling)
                    .accessibilityIdentifier(id)
            }
        case .copyDebugInfo, .repair:
            // The damaged-collection screen (V19) is not built: Copy Debug Info
            // is the one action that can run for that row.
            if let callbacks = collection.configuration.fieldDiagnostics {
                SyncCopyDebugInfoButton(callbacks: callbacks, identifier: id)
            }
        case .serviceRecovery:
            Button(title) { recovering = true }
                .disabled(collection.syncPresentation.serviceChanging)
                .accessibilityIdentifier(id)
                .alert(SyncText.value("sync.service.recovery.title"), isPresented: $recovering) {
                    Button(SyncText.value("sync.service.recovery.confirm"), role: .destructive) {
                        collection.syncPresentation.serviceChanging = true
                        Task {
                            collection.syncPresentation.serviceChangeFailed = !(await sync.recoverProviderHandoffToLocalOnly())
                            collection.syncPresentation.serviceChanging = false
                        }
                    }
                    Button(SyncText.value("sync.service.cancel"), role: .cancel) {}
                } message: { Text(SyncText.value("sync.service.recovery.message")) }
        case .meguriConnectionRetry:
            Button(title) { sync.loadMeguriConnection() }
                .disabled(sync.meguriBusy).accessibilityIdentifier(id)
        case .meguriCancel:
            if sync.meguriBusy {
                Button(title, role: .cancel) { sync.cancelMeguriPreparation() }.accessibilityIdentifier(id)
            }
        case .meguriTryAgain:
            // Setup runs again from preparation; an unknown acceptance has no
            // settling step in the core yet, so no button is shown for it.
            if sync.meguriCanChoose {
                Button(title) {
                    if collection.lifecycle == .noCollection {
                        sync.prepareMeguri(.useServerCollection)
                        showInSettings?(false)
                    } else { askKeepCollection() }
                }
                .accessibilityIdentifier(id)
            }
        case .meguriFinishSetup:
            if sync.meguriCanFinish {
                Button(title) { sync.finishMeguriSetup() }.accessibilityIdentifier(id)
            }
        case .meguriSignIn, .meguriRetryMedia, .abandon, .checkDatabase:
            // The sign-in fields are in the section; the others have no entry here.
            EmptyView()
        }
    }
    private func askKeepCollection() {
        if let showInSettings { showInSettings(true) }
        else { collection.syncPresentation.keepCollectionRequested = true }
    }
    private func open(_ entry: CloudSyncEntry) {
        if let openEntry { openEntry(entry, host) } else { collection.presentCloudSync(entry, host: host) }
    }
}

/// Copy Debug Info (T9.2): the safe export, with no secrets or card content (N7).
struct SyncCopyDebugInfoButton: View {
    let callbacks: FieldDiagnosticsCallbacks
    var identifier = "settings.sync.copyDebugInfo"
    @State private var copying = false
    var body: some View {
        Button(SyncText.value("sync.action.copyDebugInfo")) {
            copying = true
            Task {
                if let data = try? await callbacks.safeExport(), let text = String(data: data, encoding: .utf8) {
                    _ = callbacks.writeClipboard(text)
                }
                copying = false
            }
        }
        .disabled(copying)
        .accessibilityIdentifier(identifier)
    }
}

/// N15.4: the issues list. Each row has its text; the only button is Check Database's.
struct SyncIssuesPage: View {
    let collection: ReviewCollection
    var body: some View {
        List {
            Section {
                ForEach(Array(collection.sync.mainStatus.issues.enumerated()), id: \.offset) { offset, row in
                    if let key = row.textKey {
                        Text(row.count.map { SyncText.format(key, Int64(clamping: $0)) } ?? SyncText.value(key))
                            .accessibilityIdentifier("sync.issues.row.\(offset)")
                    }
                }
            } footer: { Text(SyncText.value("sync.issues.footer")) }
        }
        .navigationTitle(SyncText.value("sync.issues.title"))
        .accessibilityIdentifier("sync.issues.list")
    }
}

/// N15.5: the Troubleshooting page of the Sync section.
struct SyncTroubleshootingPage: View {
    let collection: ReviewCollection
    var host: AppNavigation.SheetHost = .main
    private var sync: SyncCoordinator { collection.sync }
    private var meguri: Bool { sync.providerState?.selection == .meguri }
    var body: some View {
        Form {
            if meguri {
                if sync.meguriCanChoose || sync.serviceConnected {
                    Section {
                        Button(SyncText.value("sync.trouble.replaceDevice")) { sync.prepareMeguri(.useServerCollection) }
                            .disabled(!sync.meguriCanChoose)
                            .accessibilityIdentifier("sync.trouble.replaceDevice")
                        Button(SyncText.value("sync.trouble.replaceService.meguri")) { sync.prepareMeguri(.useDeviceCollection) }
                            .disabled(!sync.meguriCanChoose || collection.lifecycle == .noCollection)
                            .accessibilityIdentifier("sync.trouble.replaceService")
                    } footer: {
                        if !sync.meguriCanChoose { Text(SyncText.value("sync.switch.busy")) }
                    }
                }
                if sync.meguriCanRetryMedia || sync.meguriRoutine == .mediaBlocked {
                    Section {
                        Button(SyncText.value("sync.meguri.media.retry")) {
                            if sync.meguriRoutine == .mediaBlocked { sync.retryMeguriRoundMedia() } else { sync.retryMeguriMedia() }
                        }
                        .disabled(sync.meguriBusy)
                        .accessibilityIdentifier("sync.trouble.retryMedia")
                    }
                }
            } else if sync.status?.accountStatus == .available {
                Section {
                    CloudPreviewEntryButton(collection: collection, entry: .replaceLocal, host: host,
                        title: "sync.trouble.replaceDevice", identifier: "sync.trouble.replaceDevice")
                    CloudPreviewEntryButton(collection: collection, entry: .replaceCloud, host: host,
                        title: "sync.trouble.replaceService.iCloud", identifier: "sync.trouble.replaceService")
                    if let recovery = sync.recovery, sync.mainStatus.action != .cancelOp,
                       recovery.adoptionCancellable || recovery.publicationCancellable {
                        CloudRecoveryCancelButton(collection: collection, recovery: recovery, identifier: "sync.trouble.cancelOp")
                    }
                }
            }
            if let callbacks = collection.configuration.fieldDiagnostics {
                Section {
                    NavigationLink {
                        Form {
                            SyncDiagnosticsContent(snapshot: { try await callbacks.snapshot() },
                                safeExport: { try await callbacks.safeExport() }, writeClipboard: callbacks.writeClipboard)
                        }
                        .formStyle(.grouped)
                        .navigationTitle(SyncText.value("sync.trouble.log"))
                    } label: { Text(SyncText.value("sync.trouble.log")) }
                        .accessibilityIdentifier("sync.trouble.log")
                    SyncCopyDebugInfoButton(callbacks: callbacks, identifier: "sync.trouble.copyDebugInfo")
                }
            }
            if !meguri, let copies = SyncKinds.conflictCopies(sync.status?.attention ?? [:]) {
                Section {
                    Text(SyncText.format("sync.trouble.conflictCopies", Int64(clamping: copies)))
                        .accessibilityIdentifier("sync.trouble.conflictCopies")
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle(SyncText.value("sync.settings.troubleshooting"))
        .accessibilityIdentifier("sync.trouble.content")
    }
}
