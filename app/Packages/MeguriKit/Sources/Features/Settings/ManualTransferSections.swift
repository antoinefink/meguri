import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
import UniformTypeIdentifiers

/// Informational text only; the destination never becomes a navigation action.
struct ManualTransferRedirectDetails: View {
    let diagnostic: ManualTransferRedirectDiagnostic
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(reason, bundle: .module)
            if let destination = diagnostic.destination {
                Text(verbatim: "Redirect destination: " + destination)
            }
        }
        .textSelection(.enabled)
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityIdentifier("settings.anki.redirectDiagnostic")
    }
    private var reason: LocalizedStringKey {
        switch diagnostic.reason {
        case .notMeta: "Refusal reason: This request does not support redirects."
        case .notPermanent: "Refusal reason: The redirect status is not supported."
        case .missingLocation: "Refusal reason: The server provided no redirect destination."
        case .empty: "Refusal reason: The redirect destination is empty."
        case .unparseable: "Refusal reason: The redirect destination is not an absolute URL."
        case .unsupportedScheme: "Refusal reason: The redirect destination must use HTTP or HTTPS."
        case .missingHost: "Refusal reason: The redirect destination has no host."
        case .credentials: "Refusal reason: The redirect destination contains credentials."
        case .queryOrFragment: "Refusal reason: The redirect destination contains a query or fragment."
        case .lostHttps: "Refusal reason: The redirect would lose HTTPS protection."
        case .host: "Refusal reason: The redirect host is not permitted for this request."
        case .repeated: "Refusal reason: The request already followed one redirect."
        case .notResendable: "Refusal reason: The request body cannot be sent again."
        }
    }
}

/// System Form content shared by the production Settings shell and render hooks.
public struct ManualTransferSections: View {
    @Bindable private var model: ManualTransferCoordinator
    #if os(iOS)
    private enum Field: Hashable { case endpoint, username, password }
    @FocusState private var focusedField: Field?
    #endif
    enum Area: Equatable { case all, connectionAndTransfers, connection, transfers, history }
    private let area: Area
    private let inlineHistory: Bool
    public init(model: ManualTransferCoordinator) { self.init(model: model, area: .all) }
    init(model: ManualTransferCoordinator, area: Area, inlineHistory: Bool = false) {
        self.model = model; self.area = area; self.inlineHistory = inlineHistory
    }
    public var body: some View {
        Group {
            if area == .all || area == .connectionAndTransfers || area == .connection {
                Section {
                    HStack(alignment: .center) {
                        Text("Server URL", bundle: .module)
                        endpointField
                        testConnectionButton
                    }
                    if let result = model.connectionTestResult {
                        Text(verbatim: result)
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("settings.anki.testResult")
                    }
                    if model.endpointDraft.trimmingCharacters(in: .whitespacesAndNewlines).lowercased().hasPrefix("http:") {
                        Text("HTTP sends data without encryption. Use it only for a server and network you trust.", bundle: .module)
                            .foregroundStyle(.token(.textSecondary))
                    }
                    if model.authenticated, model.endpointDraft.trimmingCharacters(in: .whitespacesAndNewlines) != model.endpoint {
                        Button { Task { await model.applyEndpoint() } } label: {
                            Text("Save Endpoint", bundle: .module)
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.regular)
                        .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                        .contentShape(Rectangle())
                        .disabled(model.busy || model.preview != nil)
                        .accessibilityIdentifier("settings.anki.saveEndpoint")
                    }
                } header: {
                    Text("Server Address", bundle: .module)
                        .accessibilityIdentifier("settings.anki.sectionTitle")
                }
                Section {
                    if model.authenticated {
                        #if os(macOS)
                        HStack(alignment: .center) {
                            Text("Login", bundle: .module)
                            Spacer()
                            accountIdentity
                            signOutButton
                        }
                        #else
                        LabeledContent {
                            HStack(spacing: Spacing.small) {
                                accountIdentity
                                Spacer()
                                signOutButton
                            }
                        } label: { Text("Login", bundle: .module) }
                        #endif
                    } else {
                        TextField("Username", text: $model.username)
                            .textContentType(.username)
                            #if os(macOS)
                            .textFieldStyle(.roundedBorder)
                            #endif
                            #if os(iOS)
                            .focused($focusedField, equals: .username)
                            #endif
                            .disabled(model.busy)
                            .accessibilityIdentifier("settings.anki.username")
                        SecureField("Password", text: $model.password)
                            .textContentType(.password)
                            #if os(macOS)
                            .textFieldStyle(.roundedBorder)
                            #endif
                            #if os(iOS)
                            .focused($focusedField, equals: .password)
                            #endif
                            .disabled(model.busy)
                            .accessibilityIdentifier("settings.anki.password")
                        Text("Login only saves a device-only login key. It does not prepare or transfer a collection. The password is not saved.", bundle: .module)
                            .foregroundStyle(.token(.textSecondary))
                            .accessibilityIdentifier("settings.anki.loginNotice")
                        action("Log In", id: "login", disabled: !model.canLogin) { await model.login() }
                    }
                } header: {
                    Text("Account", bundle: .module)
                }
            }
            if area == .all || area == .connectionAndTransfers || area == .transfers {
                Section {
                    Text("Server transfers replace a collection. Collection Files and local reviews do not require a server login.", bundle: .module)
                        .foregroundStyle(.token(.textSecondary))
                        .accessibilityIdentifier("settings.anki.intro")
                    Picker("Direction", selection: Binding(get: { model.direction }, set: { model.select($0) })) {
                        Text("Import from Anki").tag(ManualTransferDirection.import)
                        Text("Export to Anki").tag(ManualTransferDirection.export)
                    }
                    .font(.body)
                    .disabled(model.busy || model.preview != nil || model.mediaPreview != nil)
                    .accessibilityIdentifier("settings.anki.direction")
                    Text("Imports and exports include collection and media. After the collection finishes, media needs a separate decision and can change both sides. Unfinished media leaves the operation partial.", bundle: .module)
                        .foregroundStyle(.token(.textSecondary))
                        .accessibilityIdentifier("settings.anki.mediaIncluded")
                    if model.direction == .import {
                        Text("Preparation downloads and validates a candidate. Final confirmation replaces this device’s collection after making its required safety copy. It does not merge collections.")
                        action("Download and Prepare Import Preview", id: "prepareImport", disabled: !model.canPrepare) { await model.prepare() }
                    } else {
                        Text("Preparation makes a local safety copy, downloads and retains the server’s current collection, and prepares an upload. Downloading changes the server’s bookkeeping. Nothing is uploaded until final confirmation. Export replaces the server collection and activates the prepared version locally.")
                        action("Make Copies and Prepare Export Preview", id: "prepareExport", disabled: !model.canPrepare) { await model.prepare() }
                    }
                } header: { Text("Server Transfers", bundle: .module) }
            }
            // Pending decisions and their remedies remain available in either native pane.
            if area != .history {
                if let preview = model.preview {
                    Section(model.isRecoveryPreview ? "Local Recovery Decision" : "Final Replacement Preview") {
                        recordFacts(preview.record)
                        LabeledContent("Login", value: model.previewLoginLabel)
                        Text("Other clients can change the server after its retained copy was made. Export can discard those later edits; the server does not reserve this preview.")
                        Button(role: .destructive) { model.confirmationPresented = true } label: {
                            Text(model.isRecoveryPreview ? "Confirm Local Recovery…" : (preview.record.direction == .import ? "Replace This Device’s Collection…" : "Replace Server Collection…"))
                                .font(.body)
                                .foregroundStyle(.token(model.canConfirm ? .destructive : .textSecondary))
                                .fixedSize(horizontal: false, vertical: true)
                                .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget, alignment: .leading)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(!model.canConfirm)
                        .accessibilityIdentifier("settings.anki.confirm")
                        action("Close Preview", id: "closePreview", disabled: model.busy) { await model.discardPreview() }
                    }
                }
                if let media = model.mediaPreview {
                    Section("Reconcile Media on This Device and Server") {
                        recordFacts(media.record)
                        Text("Import and export both reconcile media in both directions. Server files are downloaded and pending local files are uploaded, even when the collection does not reference them. Counts become available after the media check.")
                        Text("A server file can overwrite a pending local edit, moving the previous local file to trash. A server deletion can move an unchanged local file to trash. Files marked for local deletion may return through download; local deletions may also be sent to the server.")
                        Text("Repairing names and resolving case conflicts may rename files or remove server names. References in collection fields are not rewritten. Files that are empty or larger than 100 MiB are left out.")
                        Text("Allowing media applies these effects across every supported file and batch, including changes that occur during the run. Stopping keeps saved changes and does not restore either side. Reviews remain available during media transfer.")
                        mediaDecisionActions
                    }
                }
                operationProgress
                if let message = model.message {
                    Section("Result") {
                        Text(message).accessibilityIdentifier("settings.anki.result")
                        if let diagnostic = model.redirectDiagnostic {
                            ManualTransferRedirectDetails(diagnostic: diagnostic)
                        }
                        if let instant = model.nextEligibleMilliseconds {
                            LabeledContent("Next eligible", value: date(instant))
                                .accessibilityIdentifier("settings.anki.nextEligible")
                        }
                    }
                }
            }
            if area == .all || area == .history {
                if inlineHistory {
                    VStack(alignment: .leading, spacing: Spacing.small) {
                        Text("Sync Logs", bundle: .module).font(.headline)
                        historyBody
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    Section {
                        historyBody
                    } header: { Text("Sync Logs", bundle: .module) }
                }
            }
        }
        .font(.body)
        #if os(iOS)
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                if focusedField != nil {
                    Spacer()
                    Button { focusedField = nil } label: {
                        Text("Done", bundle: .module)
                            .font(.body)
                            .foregroundStyle(.token(.statusAccent))
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                            .contentShape(Rectangle())
                    }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("settings.anki.keyboardDone")
                }
            }
        }
        #endif
    }
    @ViewBuilder private var historyBody: some View {
        Text("This history loads offline. Restart never resumes a server request. Safety copies contain collection data, without media. A copy saved below is independent of the retained original; unverified files must be inspected before use.", bundle: .module)
            .foregroundStyle(.token(.textSecondary))
        action("Refresh Local History", id: "history", disabled: model.busy) { await model.refreshOfflineHistory() }
        if model.records.isEmpty { Text("No recorded server transfers.") }
        ForEach(model.records) { entry in
            VStack(alignment: .leading, spacing: Spacing.small) {
                Text(entry.record.direction == .import ? "Import from Anki" : "Export to Anki").font(.headline)
                Text(status(entry.record))
                LabeledContent("Last persisted phase", value: entry.record.phase.rawValue)
                LabeledContent("Recorded boundary", value: entry.record.boundary?.rawValue ?? "Not recorded")
                LabeledContent("Attempt time", value: "Not recorded")
                LabeledContent("Completion time", value: "Not recorded")
                recordFacts(entry.record)
                if entry.record.canResumeMedia {
                    action("Review Media Decision / Resume…", id: "resumeMedia.\(entry.id)", disabled: model.busy || model.preview != nil || model.mediaPreview != nil || !model.authenticated) { await model.prepareMedia(entry) }
                }
                if entry.record.acceptance == .possiblyAccepted {
                    Text("Upload completion is unknown. Another transfer remains blocked. A retry or risk-acceptance clear is not available.")
                } else if entry.record.canInspectRecovery {
                    action("Inspect Local Recovery", id: "recover.\(entry.id)", disabled: model.busy || model.preview != nil) { await model.recover(entry) }
                }
                if !entry.record.collectionComplete, [.preparing, .preview, .needsDecision].contains(entry.record.phase), entry.record.acceptance == .notSent {
                    action("Cancel Unfinished Preparation", id: "cancelPreparation.\(entry.id)", disabled: model.busy || model.preview != nil) { await model.cancelPreparation(entry) }
                }
                ForEach(Array(entry.record.copies.enumerated()), id: \.offset) { index, copy in
                    Text("Save a separate copy of \(copy.name)")
                    Button { model.chooseCopy(entry, index: index) } label: {
                        Text("Save Copy to Folder…")
                            .font(.body)
                            .foregroundStyle(.token(model.busy ? .textSecondary : .statusAccent))
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget, alignment: .leading)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .disabled(model.busy)
                    .accessibilityIdentifier("settings.anki.copy.\(entry.id)-\(index)")
                }
            }
        }
    }
    private var endpointField: some View {
        Group {
            #if os(macOS)
            TextField("Server URL", text: $model.endpointDraft, prompt: Text(verbatim: "https://…"))
                .textFieldStyle(.roundedBorder)
                .controlSize(.regular)
            #else
            TextField("Server URL", text: $model.endpointDraft, prompt: Text(verbatim: "https://…"), axis: .vertical)
            #endif
        }
        .labelsHidden()
        .frame(maxWidth: .infinity, alignment: .leading)
        .multilineTextAlignment(.leading)
        .font(.body)
        .textContentType(.URL)
        #if os(iOS)
        .keyboardType(.URL)
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .frame(minHeight: ControlMetrics.minimumTarget)
        .contentShape(Rectangle())
        .focused($focusedField, equals: .endpoint)
        #endif
        .disabled(model.busy || model.preview != nil || model.mediaPreview != nil)
        .accessibilityIdentifier("settings.anki.endpoint")
        .accessibilityLabel(Text("Server URL", bundle: .module))
        .accessibilityHint("Self-hosted server URL, including an optional path prefix.")
    }
    private var testConnectionButton: some View {
        Button { Task { await model.testConnection() } } label: {
            Text("Test", bundle: .module)
        }
        .buttonStyle(.bordered)
        .controlSize(.regular)
        .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
        .contentShape(Rectangle())
        .disabled(!model.canTestConnection)
        .accessibilityLabel(Text("Test Connection", bundle: .module))
        .accessibilityIdentifier("settings.anki.testConnection")
    }
    private var accountIdentity: some View {
        Text(verbatim: model.username)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(verbatim: model.username))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier("settings.anki.loginIdentity")
    }
    private var signOutButton: some View {
        Button { Task { await model.logout() } } label: {
            Text("Sign Out", bundle: .module)
        }
        .buttonStyle(.bordered)
        .controlSize(.regular)
        .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
        .contentShape(Rectangle())
        .disabled(model.busy || model.preview != nil)
        .accessibilityIdentifier("settings.anki.logout")
    }
    @ViewBuilder private var operationProgress: some View {
        if model.busy {
            Section("Progress") {
                ProgressView().accessibilityLabel("Server operation in progress")
                Text(operationDescription)
                    .accessibilityIdentifier("settings.anki.progress")
                if let media = model.mediaProgress, model.stage == .transferringMedia {
                    Text(mediaDescription(media.phase))
                    LabeledContent("Entries checked", value: "\(media.counters.checked)")
                    LabeledContent("Server changes listed", value: "\(media.listed)")
                    LabeledContent("Files received", value: "\(media.counters.downloadedFiles)")
                    LabeledContent("Files sent", value: "\(media.counters.uploadedFiles)")
                    LabeledContent("File bytes received / sent", value: "\(media.counters.bytesDownloaded) / \(media.counters.bytesUploaded)")
                    if media.phase == .downloading {
                        LabeledContent("Files remaining", value: "\(media.filesLeft)")
                        if let seconds = media.estimateSeconds { LabeledContent("Estimated seconds remaining", value: "\(Int(seconds.rounded(.up)))") }
                    }
                }
                if model.stage == .transferringMedia || model.progress?.cancellable != false {
                    action("Cancel Operation", id: "cancel", disabled: false) {
                        await model.cancel()
                    }
                }
            }
        }
    }
    private var mediaDecisionActions: some View {
        Group {
            action("Allow Both-Side Media Changes and Start", id: "startMedia", disabled: !model.canRunMedia) {
                await model.startMedia()
            }
            action("Keep Media for Later", id: "cancelPreparedMedia", disabled: model.busy, run: model.cancelPreparedMedia)
        }
    }
    private func mediaDescription(_ phase: ManualMediaPhase) -> String {
        switch phase {
        case .checking: "Checking local media…"
        case .listing: "Finding server media changes…"
        case .downloading: "Receiving media files…"
        case .uploading: "Sending pending media changes…"
        case .confirming: "Checking media completion…"
        case .resyncing: "Checking media again…"
        }
    }
    private var operationDescription: String {
        switch (model.stage, model.progress?.cancellable) {
        case (.applyingDecision, .some(false)): "Activating the confirmed collection…"
        case (.applyingDecision, _): "Transferring the collection…"
        case (.testingConnection, _): "Testing the entered address without credentials…"
        case (.authenticating, _): "Getting a login key for this device…"
        case (.buildingDecision, _): "Saving safety copies and preparing the preview…"
        case (.inspectingCandidate, _): "Inspecting the local recovery collection…"
        case (.preparingMedia, _): "Preparing the saved operation’s media decision…"
        case (.awaitingMediaConsent, _): "Waiting for a media decision…"
        case (.transferringMedia, _): "Reconciling media on this device and server. Reviews remain available."
        case (.writingCopy, _): "Verifying the retained file before saving a copy…"
        case (.readingRecords, _), (.ready, _), (.awaitingConfirmation, _): "Loading saved transfer history…"
        }
    }
    private func action(_ title: String, id: String, disabled: Bool, run: @escaping @MainActor () async -> Void) -> some View {
        Button { Task { await run() } } label: {
            Text(title)
                .fixedSize(horizontal: false, vertical: true)
        }
            .buttonStyle(.automatic)
            .controlSize(.regular)
            .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget, alignment: .leading)
            .contentShape(Rectangle())
            .disabled(disabled).accessibilityIdentifier("settings.anki.\(id)")
    }

    @ViewBuilder private func recordFacts(_ record: ManualTransferRecord) -> some View {
        ForEach(record.decisionFacts) { fact in
            LabeledContent(fact.label, value: fact.value)
        }
    }
    private func date(_ recordedInstant: Int64) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: Date(timeIntervalSince1970: Double(recordedInstant) / 1_000))
    }
    private func status(_ record: ManualTransferRecord) -> String {
        if record.phase == .superseded { return "Partial operation superseded by a verified local collection replacement. Media Resume is unavailable; saved effects and retained copies remain." }
        if record.phase == .complete { return record.mediaSelected ? "Collection and supported media complete. Excluded files were not transferred." : "Collection complete. Media was excluded." }
        if record.collectionComplete { return "Collection complete. The whole selected operation is incomplete." }
        switch record.phase {
        case .preparing: return "Preparation was interrupted. Inspect recovery before continuing."
        case .preview: return "Prepared; replacement is not confirmed."
        case .cancelled: return model.cancellationDescription(for: record)
        case .failed: return "Failed. Retained copies remain available."
        case .needsDecision: return "A new recovery decision is required."
        case .remoteAccepted: return "Server accepted the collection. Local activation is incomplete."
        case .activating: return "Local activation needs inspection."
        default: return "Interrupted operation; inspect its local recovery state."
        }
    }
}

public extension View {
    func manualTransferPresentation(model: ManualTransferCoordinator, isActive: Bool = true) -> some View {
        modifier(ManualTransferPresentation(model: model, isActive: isActive))
    }
}
private struct ManualTransferPresentation: ViewModifier {
    @Bindable var model: ManualTransferCoordinator
    let isActive: Bool
    private func presentation(_ value: ReferenceWritableKeyPath<ManualTransferCoordinator, Bool>) -> Binding<Bool> {
        Binding(get: { isActive && model[keyPath: value] }, set: { if isActive { model[keyPath: value] = $0 } })
    }
    func body(content: Content) -> some View {
        content
        .destructiveConfirmation(Text("Confirm collection replacement?"), isPresented: presentation(\.confirmationPresented),
            actionTitle: Text(model.isRecoveryPreview ? "Recover Locally" : "Replace Collection"),
            message: Text("\(model.endpoint)\nLogin: \(model.previewLoginLabel)\n\(model.isRecoveryPreview ? "The retained candidate replaces this device’s collection. Nothing is uploaded." : (model.preview?.record.direction == .import ? "The prepared collection replaces this device’s collection." : "The prepared collection replaces the server collection and this device’s active version.")) \(model.preview?.record.mediaSelected == true ? "Media needs a separate decision after collection completion." : "Media is excluded.")")) {
                Task { await model.confirm() }
            }
        .fileImporter(isPresented: presentation(\.copyDestinationPresented), allowedContentTypes: [.folder]) { result in
            Task { await model.copyDestination(result.map { [$0] }) }
        }
        .task { await model.loadOffline() }
        .onDisappear { model.clearPassword() }
    }
 }
