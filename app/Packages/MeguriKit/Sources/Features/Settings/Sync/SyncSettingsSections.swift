import AppModel
import CoreBridge
import SwiftUI

/// N15.2: the Sync section of Settings, as system form groups under one header.
struct SyncSettingsSections: View {
    let collection: ReviewCollection
    var host: AppNavigation.SheetHost = .main
    /// The sign-in fields' text, owned above any layout that rebuilds this section.
    var draft: MeguriSignInDraft
    /// The Mac Settings pane already names the section in its title bar.
    var showsHeading = true
    private var sync: SyncCoordinator { collection.sync }
    private var selection: SyncProviderSelection? {
        switch sync.providerState?.selection {
        case .iCloud: .iCloud
        case .meguri: .meguri
        case .localOnly, nil: nil
        }
    }
    var body: some View {
        if sync.meguriSetupNotFinished { setupForm } else { sections }
    }
    /// N15.7: Meguri Sync before its first sign-in is a setup form. The status,
    /// Sync Automatically and Sync Now appear once a sign-in succeeds.
    @ViewBuilder private var setupForm: some View {
        Section {
            SyncServiceRows(collection: collection, host: host)
        } header: { heading }
        Section {
            MeguriAccountRows(collection: collection, draft: draft, host: host, footnoteInRows: false)
            // The first sign-in's progress: the status row is not shown in this form.
            if sync.mainStatus.titleKey == "sync.meguri.status.signingIn.title" {
                ProgressView(SyncText.value("sync.meguri.status.signingIn.title"))
                    .accessibilityIdentifier("settings.sync.meguri.signingIn")
            }
        } footer: {
            Text(SyncText.value(sync.storedAutomatic ? "sync.meguri.signIn.footer.setup" : "sync.meguri.signIn.footer.setup.manual"))
                .accessibilityIdentifier("settings.sync.meguri.signInFooter")
        }
        troubleshooting
    }
    @ViewBuilder private var heading: some View {
        if showsHeading {
            Text(SyncText.value("sync.settings.sync.section"))
                .accessibilityIdentifier("settings.sync.heading")
        }
    }
    @ViewBuilder private var troubleshooting: some View {
        Section {
            SyncSettingsLink(.troubleshooting, collection: collection, host: host) {
                Text(SyncText.value("sync.settings.troubleshooting"))
            }
                .accessibilityIdentifier("settings.sync.troubleshooting")
        } footer: {
            if selection == .iCloud {
                Text(SyncText.value("sync.settings.privacy"))
                    .accessibilityIdentifier("settings.sync.privacyFooter")
            }
        }
    }
    @ViewBuilder private var sections: some View {
        Section {
            SyncServiceRows(collection: collection, host: host)
            if selection == .meguri { MeguriAccountRows(collection: collection, draft: draft, host: host) }
            if selection != nil || sync.providerNeedsAttention {
                SyncMainStatusRows(collection: collection, host: host)
            }
            // N23.1: the one media fact the status panel leaves out stays in the Sync section.
            if selection == .meguri, MeguriMediaFactRows.isVisible(sync) {
                MeguriMediaFactRows(collection: collection)
                    .font(.footnote).foregroundStyle(.secondary)
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("settings.sync.meguri.mediaFacts")
            }
            if selection != nil {
                Toggle(SyncText.value("sync.settings.automatic"), isOn: Binding(
                    get: { sync.automaticSwitchValue }, set: { sync.setAutomatic($0) }))
                    .disabled(!sync.automaticSwitchAvailable)
                    .accessibilityIdentifier("settings.sync.automatic")
                // Until the service is connected the status row says what is missing; the
                // not-connected reason ("Sync is off…") would contradict the Service row.
                SyncNowButton(collection: collection, identifier: "settings.sync.syncNow",
                    showsUnavailableReason: sync.serviceConnected)
            }
        } header: { heading } footer: {
            Text(SyncText.value(selection == nil ? "sync.service.none.footer" : "sync.settings.automatic.footer"))
                .accessibilityIdentifier("settings.sync.footer")
        }
        if selection != nil { troubleshooting }
    }
}

/// What the person typed into the Meguri Sync sign-in fields. The Settings page
/// owns it, so a layout change that rebuilds the section keeps the text. The
/// password leaves it at submission and when the page goes away.
@Observable public final class MeguriSignInDraft {
    public var address = ""
    public var username = ""
    public var password = ""
    /// A Sign In made in the fields continues into setup (MP6.3). Kept here, not
    /// in the view, so a layout change between Sign In and setup does not lose it.
    public var setupAfterSignIn = false
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
    /// The Account destination offers the same guarded change to Off.
    var accountSignOut = false
    @Environment(\.dismiss) private var dismiss
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
        control
        // MP6.5: the control stays enabled while the provider is blocked.
        .disabled(changing || sync.providerState?.phase == .handoffRecovery || collection.meguriSignInPreparing || sync.meguriBusy)
        // Synthetic examples only: open the confirmation the example names.
        .task {
            guard !accountSignOut, let request = sync.renderServiceSwitch else { return }
            // The window must be on screen before an alert can attach to it.
            try? await Task.sleep(for: .milliseconds(800))
            choose(request.destination)
        }
        .modifier(ServiceSwitchQuestion(collection: collection, destination: $destination, source: switchSource) { value, shown in
            change(value, shown: shown)
        })
        feedback
    }
    @ViewBuilder private var control: some View {
        if accountSignOut {
            Button(SyncText.value("sync.settings.account.signOut"), role: .destructive) { choose("localOnly") }
                .accessibilityIdentifier("settings.sync.meguri.signOut")
        } else {
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
            .accessibilityIdentifier("settings.sync.service")
        }
    }
    @ViewBuilder private var feedback: some View {
        if changing {
            ProgressView(SyncText.value("sync.service.changing"))
                .accessibilityIdentifier("settings.sync.serviceChanging")
        }
        if failed {
            // iCloud refused on an installation that cannot use it says so (T19.4's `setupNeeded` text).
            Text(SyncText.value(sync.serviceChangeFailure?.reason == .iCloudSetupNeeded
                    ? "sync.status.unavailable.setupNeeded" : "sync.service.changeFailed")).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.serviceChangeFailed")
        }
        if refusedForOp, opBlocksChange {
            Text(SyncText.value("sync.service.opFirst")).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.serviceRefused")
        } else if !changing, !sync.serviceChangeAvailable, selected != "localOnly" {
            Text(SyncText.value(sync.serviceChangeBusyKey)).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.serviceBusy")
        }
        if sync.providerState?.phase == .blocked {
            Text(SyncText.value("sync.service.blocked")).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.sync.serviceBlocked")
        }
    }
    private func choose(_ value: String) {
        guard !accountSignOut || selected == "meguri" else { return }
        guard !collection.meguriSignInPreparing, !sync.meguriBusy, value != selected else { return }
        refusedForOp = false
        if opBlocksChange { refusedForOp = true; return }
        // T21.2: before its first sign-in Meguri Sync holds nothing to lose, so leaving it asks nothing.
        if selected != "localOnly", !sync.meguriSetupNotFinished {
            // T21.1: the question opens only while the change can run; the reason shows under the control.
            guard sync.serviceChangeAvailable else { return }
            switchSource = sync.providerState?.selection; destination = value
        }
        else { change(value) }
    }
    /// `shown`: the service the confirmation named. When the selection is
    /// no longer that one, nothing changes.
    private func change(_ value: String, shown: SyncProviderSelection? = nil) {
        let source = sync.providerState?.selection
        if let shown, shown != source { destination = nil; return }
        changing = true; failed = false
        Task {
            let target: SyncProviderSelection = value == "meguri" ? .meguri : value == "iCloud" ? .iCloud : .localOnly
            switch await collection.changeSyncService(to: target, from: source) {
            case .changed:
                destination = nil
                if accountSignOut { dismiss() }
                if value == "iCloud" { collection.presentCloudSync(.connect, host: host) }
            case .superseded: destination = nil
            case .failed: failed = true
            }
            changing = false
        }
    }
}

/// N15.2 row 2 and MP6.1: the sign-in fields until signed in, then one Account row.
struct MeguriAccountRows: View {
    let collection: ReviewCollection
    let draft: MeguriSignInDraft
    var host: AppNavigation.SheetHost = .main
    /// The setup footnote is a row here, or the group's footer (N15.7).
    var footnoteInRows = true
    private enum Field: Hashable { case address, username, password }
    @FocusState private var focus: Field?
    /// A Sign In made here continues into setup (MP6.3). Setup that was left
    /// unfinished earlier continues only from a user action.
    private var setupAfterSignIn: Bool {
        get { draft.setupAfterSignIn } nonmutating set { draft.setupAfterSignIn = newValue }
    }
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
                    field("sync.meguri.address") {
                        TextField(SyncText.value("sync.meguri.address"), text: Binding(get: { address }, set: {
                            guard !processing else { return }
                            address = $0; password = ""; sync.meguriAddressEdited()
                        }), prompt: Text(SyncText.value("sync.meguri.address.prompt")))
                            .textContentType(.URL)
                            .focused($focus, equals: .address)
                            .onSubmit { focus = .username }
                            #if os(iOS)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            #endif
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("settings.sync.meguri.address")
                    }
                    field("sync.meguri.username") {
                        TextField(SyncText.value("sync.meguri.username"), text: Binding(get: { username }, set: {
                            if !processing { username = $0 }
                        }), prompt: Text(SyncText.value("sync.meguri.required.prompt")))
                            .textContentType(.username)
                            .focused($focus, equals: .username)
                            .onSubmit { focus = .password }
                            #if os(iOS)
                            .textInputAutocapitalization(.never)
                            #endif
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("settings.sync.meguri.username")
                    }
                    field("sync.meguri.password") {
                        SecureField(SyncText.value("sync.meguri.password"), text: Binding(get: { password }, set: {
                            if !processing { password = $0 }
                        }), prompt: Text(SyncText.value("sync.meguri.required.prompt")))
                            .textContentType(.password)
                            .focused($focus, equals: .password)
                            .onSubmit(signIn)
                            .accessibilityIdentifier("settings.sync.meguri.password")
                    }
                }
                .disabled(processing || changing)
                signInButton
                if footnoteInRows {
                    Text(SyncText.value(sync.storedAutomatic ? "sync.meguri.signIn.footer.setup" : "sync.meguri.signIn.footer.setup.manual"))
                        .font(.footnote).foregroundStyle(.secondary)
                        .accessibilityIdentifier("settings.sync.meguri.signInFooter")
                }
                if let key = sync.meguriSignInErrorKey {
                    Label(SyncText.value(key), systemImage: "exclamationmark.circle")
                        .accessibilityIdentifier("settings.sync.meguri.signInError")
                }
            } else {
                // N15.2 row 2: the username is the row's value; the page has both facts.
                SyncSettingsLink(.account, collection: collection, host: host) {
                    LabeledContent(SyncText.value("sync.settings.account"), value: sync.meguriUsername ?? "")
                }
                .accessibilityIdentifier("settings.sync.meguri.account")
                // Setup left unfinished in an earlier session continues only from here.
                if sync.meguriStage == .required, sync.meguriCanChoose, !setupAfterSignIn, sync.meguriSetupDecision == nil {
                    Button(SyncText.value("sync.recovery.tryAgain")) { sync.startMeguriSetup() }
                        .accessibilityIdentifier("settings.sync.meguri.continueSetup")
                }
            }
            // The work's bar is in the status row (MP6.5, resume fix); preparing a sign-in is this row's own.
            if collection.meguriSignInPreparing {
                ProgressView(SyncText.value("sync.meguri.signIn.preparing"))
                    .accessibilityIdentifier("settings.sync.meguri.signInProgress")
            }
        }
        .onAppear {
            if address.isEmpty { address = sync.meguriEndpoint }
            startSetupIfAsked()
            #if os(macOS)
            // N15.7: the form starts in its first empty field (the Settings window; on
            // touch the page also holds Collection, and a keyboard would cover it).
            if showsFields { focus = address.isEmpty ? .address : username.isEmpty ? .username : .password }
            #endif
        }
        .onChange(of: sync.meguriEndpoint) { _, endpoint in address = endpoint; password = "" }
        .onChange(of: sync.meguriCanChoose) { _, _ in startSetupIfAsked() }
        .onChange(of: sync.meguriCanSettle) { _, _ in startSetupIfAsked() }
        .onChange(of: collection.syncPresentation.keepCollectionRequested) { _, _ in startSetupIfAsked() }
        // The one presentation of the keep-collection sheet (N15.3). The core's
        // decision opens it; no view state can.
        .sheet(isPresented: Binding(get: { sync.meguriSetupDecision != nil }, set: { _ in })) {
            if let decision = sync.meguriSetupDecision {
                MeguriKeepCollectionSheet(collection: collection, facts: decision.facts, account: decision.account)
            }
        }
        .sheet(isPresented: Binding(get: { sync.meguriPreview != nil }, set: { _ in })) {
            if let preview = sync.meguriPreview { MeguriSetupPreviewSheet(collection: collection, preview: preview) }
        }
    }
    private var canSignIn: Bool {
        !processing && !changing && !address.isEmpty && !username.isEmpty && !password.isEmpty
    }
    /// N15.7: never a disabled prominent button. On the Mac it sits at the row's
    /// trailing edge, prominent only once it can sign in; on touch it is a
    /// button row of the form, as the system's sign-in forms have.
    @ViewBuilder private var signInButton: some View {
        let button = Button(SyncText.value("sync.meguri.signIn"), action: signIn)
            .disabled(!canSignIn)
            .accessibilityIdentifier("settings.sync.meguri.signIn")
        #if os(macOS)
        HStack {
            Spacer()
            if canSignIn { button.buttonStyle(.borderedProminent) } else { button.buttonStyle(.bordered) }
        }
        #else
        button
        #endif
    }
    @ViewBuilder private func field(_ key: String, @ViewBuilder content: () -> some View) -> some View {
        #if os(macOS)
        content()
        #else
        // The field carries the same name for VoiceOver, so the visible label is not read twice.
        HStack {
            Text(SyncText.value(key)).accessibilityHidden(true)
            content().multilineTextAlignment(.trailing)
        }
        #endif
    }
    private func signIn() {
        // All three values are those of the tap: an edit made while the hold
        // clears never sends this password to another address or account.
        let endpoint = address, account = username, secret = password
        password = ""
        guard !processing, !changing, !endpoint.isEmpty, !account.isEmpty, !secret.isEmpty else { return }
        setupAfterSignIn = true
        // A request left by an earlier status action is not this sign-in's:
        // Cancel during this setup leaves the service Off (MP6.3).
        collection.syncPresentation.keepCollectionRequested = false
        Task {
            // Signing in to set the service up is a service choice: it clears the hold.
            guard await sync.clearUpdateHold(), address == endpoint, username == account else { return }
            collection.signInMeguri(endpoint: endpoint, username: account, password: secret)
        }
    }
    /// MP6.3: signing in is the consent to set the service up. The core reads
    /// both sides and asks only when both hold a collection. MP6.9: an unknown
    /// acceptance is settled before anything else of a setup attempt.
    private func startSetupIfAsked() {
        let requested = collection.syncPresentation.keepCollectionRequested
        guard setupAfterSignIn || requested else { return }
        if sync.meguriCanSettle {
            setupAfterSignIn = false
            // The request is used up here too: what follows the settling needs its own tap.
            collection.syncPresentation.keepCollectionRequested = false
            sync.settleMeguriSetup()
        } else if sync.meguriCanChoose, sync.meguriSetupDecision == nil, sync.meguriPreview == nil,
                  sync.meguriStage == .required || requested {
            setupAfterSignIn = false
            collection.syncPresentation.keepCollectionRequested = false
            // A sheet opened by the status action keeps the service on Cancel (V27.4).
            sync.startMeguriSetup(keepingService: requested)
        }
    }
}

/// N15.3 for Meguri Sync: both sides' counts, then one button per collection.
/// A button is the confirmation, bound by the core to the facts shown here
/// (MP6.3); changed facts reload the sheet and need a new choice.
struct MeguriKeepCollectionSheet: View {
    let collection: ReviewCollection
    let facts: MeguriSetupFacts
    /// The account the core decided for; the one-direction forms name it (MP6.3).
    var account = ""
    @Environment(\.appClock) private var clock
    @State private var cancelling = false
    #if os(macOS)
    /// Keyboard focus starts on Cancel: neither replacement reads as the default (N15.3).
    @FocusState private var cancelFocused: Bool
    #endif
    private var sync: SyncCoordinator { collection.sync }
    private var choices: [MeguriSetupChoice] { SyncCoordinator.meguriSetupChoices(facts.row) }
    private var working: Bool { cancelling || sync.meguriBusy }
    /// The one-direction forms (MP6.3): a changed endpoint or account whose
    /// server is empty, or media files on a device with no collection.
    private var oneDirection: Bool { facts.row == .askUploadOnly || facts.row == .askDownloadOnly }
    private var titleKey: String {
        switch facts.row {
        case .askUploadOnly: "sync.keep.uploadOnly.title"
        case .askDownloadOnly: "sync.keep.downloadOnly.title.meguri"
        default: "sync.keep.title"
        }
    }
    private var messageKey: String {
        switch facts.row {
        case .askUploadOnly: "sync.keep.uploadOnly.message.meguri"
        case .askDownloadOnly: "sync.keep.downloadOnly.message.meguri"
        default: "sync.keep.message"
        }
    }
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    #if !os(macOS)
                    // The title is a question longer than a navigation bar holds: it wraps here.
                    Text(SyncText.value(titleKey)).font(.headline)
                        .accessibilityAddTraits(.isHeader)
                        .accessibilityIdentifier("sync.keepCollection.title")
                    #endif
                    if let key = sync.meguriSetupNoticeKey {
                        Text(SyncText.value(key)).accessibilityIdentifier("sync.keepCollection.changed")
                    }
                    Text(SyncText.value(messageKey))
                        .accessibilityIdentifier("sync.keepCollection.message")
                }
                if let device = facts.device {
                    Section(SyncText.value("sync.counts.thisDevice")) {
                        if facts.row == .askDownloadOnly {
                            // No collection on this device: only its media files are counted.
                            if let media = device.mediaFiles {
                                LabeledContent(SyncText.value("sync.counts.mediaFiles"), value: media.formatted(.number))
                            }
                        } else { rows(device) }
                    }
                }
                if facts.server != nil || oneDirection {
                    Section(SyncText.value("sync.service.meguri")) {
                        LabeledContent(SyncText.value("sync.meguri.preview.server"), value: sync.meguriEndpoint)
                        if oneDirection, !account.isEmpty {
                            LabeledContent(SyncText.value("sync.settings.account"), value: account)
                                .accessibilityIdentifier("sync.keepCollection.account")
                        }
                        if let server = facts.server { rows(server) }
                    }
                }
                if choices.contains(.keepDevice) {
                    Section {
                        Button(SyncText.value(facts.row == .askUploadOnly ? "sync.keep.uploadOnly.confirm" : "sync.keep.device"),
                               role: .destructive) { sync.answerMeguriSetup(.keepDevice) }
                            .disabled(working)
                            .accessibilityIdentifier("sync.keepCollection.keepDevice")
                    } footer: {
                        VStack(alignment: .leading) {
                            // Upload only: the server holds no user content, so nothing there is replaced (T19.3).
                            if facts.row != .askUploadOnly { Text(SyncText.value("sync.keep.device.effect.meguri")) }
                            Text(SyncText.value("sync.meguri.preview.otherClients"))
                        }
                    }
                }
                if choices.contains(.keepServer) {
                    Section {
                        Button(SyncText.value(facts.row == .askDownloadOnly ? "sync.keep.downloadOnly.confirm" : "sync.keep.service.meguri"),
                               role: .destructive) { sync.answerMeguriSetup(.keepServer) }
                            .disabled(working)
                            .accessibilityIdentifier("sync.keepCollection.keepService")
                    } footer: {
                        VStack(alignment: .leading) {
                            // With no collection on this device nothing is replaced or backed up.
                            if facts.row != .askDownloadOnly { Text(SyncText.value("sync.keep.service.effect")) }
                            if facts.unsent { Text(SyncText.value("sync.meguri.preview.unsent")) }
                        }
                    }
                }
                Section {} footer: {
                    VStack(alignment: .leading) {
                        Text(SyncText.value("sync.meguri.preview.media"))
                        if sync.storedAutomatic { Text(SyncText.value("sync.meguri.preview.automatic")) }
                    }
                }
            }
            .formStyle(.grouped)
            #if os(macOS)
            .navigationTitle(SyncText.value(titleKey))
            #else
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    // Cancel is always available: only its own run disables it.
                    Button(SyncText.value("sync.service.cancel"), role: .cancel, action: cancel)
                        .disabled(cancelling)
                        #if os(macOS)
                        .focused($cancelFocused)
                        #endif
                        .accessibilityIdentifier("sync.keepCollection.cancel")
                }
            }
            .interactiveDismissDisabled()
        }
        #if os(macOS)
        // On the stack, so that the toolbar's Cancel is inside the focus scope.
        // The toolbar joins the key view loop after the form, so focus is also asked for once shown.
        .defaultFocus($cancelFocused, true)
        .task { cancelFocused = true }
        #endif
        .accessibilityIdentifier("sync.keepCollection.sheet")
    }
    @ViewBuilder private func rows(_ side: MeguriSetupSide) -> some View {
        LabeledContent(SyncText.value("sync.counts.notes"), value: side.notes.formatted(.number))
        LabeledContent(SyncText.value("sync.counts.cards"), value: side.cards.formatted(.number))
        LabeledContent(SyncText.value("sync.counts.reviews"), value: side.reviews.formatted(.number))
        if let media = side.mediaFiles {
            LabeledContent(SyncText.value("sync.counts.mediaFiles"), value: media.formatted(.number))
        }
        if let change = side.lastChangeMs {
            LabeledContent(SyncText.value("sync.counts.lastChange"), value: SyncText.instant(change, now: clock.now()))
        }
    }
    /// MP6.3, V27.4: during setup Cancel leaves the service Off, which removes
    /// the sign-in; from a status action it changes nothing but the sheet.
    private func cancel() {
        cancelling = true
        Task {
            if sync.meguriBusy { sync.cancelMeguriPreparation() }
            if await sync.cancelMeguriSetup() {
                let removed = await sync.removeMeguriConnection()
                collection.syncPresentation.serviceChangeFailed = !removed
            }
            cancelling = false
        }
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
    private var sync: SyncCoordinator { collection.sync }
    private var status: SyncMainStatus { sync.mainStatus }
    private var meguri: Bool { sync.providerState?.selection == .meguri }
    private var detail: String? { SyncStatusCopy.detail(sync, now: clock.now()) }
    var body: some View {
        let status = status
        // The detail lines sit under the title, beside the symbol, as a system row's subtitle does.
        Label {
            VStack(alignment: .leading) {
                Text(SyncText.value(status.titleKey))
                if let detail { Text(detail).font(.footnote).foregroundStyle(.secondary) }
                if let progress = sync.panelProgress {
                    // N15.2 row 3 (resume fix): the status's own bar, under its text, with the
                    // panel's value. One indicator: a value that becomes unknown stays in place.
                    // Inside the combined status element: the title is already read; the bar adds its value.
                    ProgressView(value: progress.fraction, total: 1)
                        .progressViewStyle(.linear)
                        .accessibilityIdentifier("settings.sync.status.progress")
                }
                if !meguri, let cloud = sync.status {
                    ForEach(Array(SyncStatusCopy.lines(cloud, now: clock.now()).enumerated()), id: \.offset) { _, line in
                        Text(line).font(.footnote).foregroundStyle(.secondary)
                    }
                }
                if meguri, let done = sync.meguriSetupDone {
                    // T19.3: what setup did when it asked nothing.
                    Group {
                        switch done {
                        case .uploaded: Text(SyncText.value("sync.setup.done.uploaded.meguri"))
                        case .downloaded(let backup):
                            Text(SyncText.value("sync.setup.done.downloaded.meguri"))
                            if backup { Text(SyncText.value("sync.setup.done.backup")) }
                        }
                    }
                    .font(.footnote).foregroundStyle(.secondary)
                }
            }
        } icon: {
            SyncStatusSymbol(status: status, iCloud: sync.symbolsAreICloud, secondary: true)
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
        if let action = status.action {
            SyncStatusActionButton(collection: collection, action: action, host: host,
                openEntry: openEntry, showInSettings: showInSettings, identifier: "settings.sync.statusAction")
        }
        // An issues row only for an issue beside the state itself (resume fix):
        // Meguri Sync's media-blocked row needs the files it lists.
        if SyncStatusCopy.showsIssues(sync) {
            SyncSettingsLink(meguri ? .meguriAffectedFiles : .issues, collection: collection, host: host) {
                if let key = status.issuesRowKey, let count = MeguriAffectedCount.value(sync) {
                    Text(SyncText.format(key, Int64(clamping: count)))
                } else {
                    Text(SyncText.format("sync.issues.row", Int64(status.issues.count)))
                }
            }
            .accessibilityIdentifier("settings.sync.issues")
        }
    }
}

/// Copy Debug Info (T9.2): the safe export, with no secrets or card content (N7).
struct SyncCopyDebugInfoButton: View {
    let callbacks: FieldDiagnosticsCallbacks
    let sync: SyncCoordinator
    var identifier = "settings.sync.copyDebugInfo"
    @State private var copying = false
    var body: some View {
        Button(SyncText.value("sync.action.copyDebugInfo")) {
            copying = true
            Task {
                if let data = try? await callbacks.readableExport(sync: sync.debugFacts), let text = String(data: data, encoding: .utf8) {
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
                        // A connected device has a proved baseline, and the core starts no
                        // full replacement over one (MP4): the entries wait for a setup.
                        if sync.serviceConnected { Text(SyncText.value("sync.trouble.replace.connected.meguri")) }
                        else if !sync.meguriCanChoose { Text(SyncText.value("sync.switch.busy")) }
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
                    SyncSettingsLink(.syncLog, collection: collection, host: host) {
                        Text(SyncText.value("sync.trouble.log"))
                    }
                        .accessibilityIdentifier("sync.trouble.log")
                    SyncCopyDebugInfoButton(callbacks: callbacks, sync: sync, identifier: "sync.trouble.copyDebugInfo")
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
