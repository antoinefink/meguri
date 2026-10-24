import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
import UniformTypeIdentifiers

/// System Form content shared by the production Settings shell and render hooks.
public struct ManualTransferSections: View {
    @Bindable private var model: ManualTransferCoordinator
    #if os(iOS)
    private enum Field: Hashable { case endpoint, username, password }
    @FocusState private var focusedField: Field?
    #endif
    public init(model: ManualTransferCoordinator) { self.model = model }
    public var body: some View {
        Group {
            Section("Anki Server Import / Export") {
                Text("Routine sync uses CloudKit. These separate, explicit server transfers replace a collection. Collection Files and local reviews do not require a server login.")
                    .foregroundStyle(.token(.textSecondary))
                TextField("Self-hosted endpoint, including optional prefix", text: $model.endpointDraft)
                    .textContentType(.URL)
                    #if os(iOS)
                    .focused($focusedField, equals: .endpoint)
                    #endif
                    .disabled(model.busy || model.preview != nil || model.mediaPreview != nil)
                    .accessibilityIdentifier("settings.anki.endpoint")
                action("Save Endpoint", id: "saveEndpoint", disabled: model.busy || model.preview != nil) { await model.applyEndpoint() }
                if !model.endpoint.isEmpty { LabeledContent("Configured endpoint", value: model.endpoint) }
                if model.authenticated {
                    LabeledContent("Login", value: model.username)
                        .accessibilityIdentifier("settings.anki.loginIdentity")
                    action("Log Out", id: "logout", disabled: model.busy || model.preview != nil) { await model.logout() }
                } else {
                    TextField("Username", text: $model.username)
                        .textContentType(.username)
                        #if os(iOS)
                        .focused($focusedField, equals: .username)
                        #endif
                        .disabled(model.busy)
                        .accessibilityIdentifier("settings.anki.username")
                    SecureField("Password", text: $model.password)
                        .textContentType(.password)
                        #if os(iOS)
                        .focused($focusedField, equals: .password)
                        #endif
                        .disabled(model.busy)
                        .accessibilityIdentifier("settings.anki.password")
                    action("Log In", id: "login", disabled: model.busy || model.endpoint.isEmpty || model.password.isEmpty) { await model.login() }
                    Text("Login only saves a device-only login key. It does not prepare or transfer a collection. The password is not saved.")
                        .foregroundStyle(.token(.textSecondary))
                }
                Picker("Direction", selection: Binding(get: { model.direction }, set: { model.select($0) })) {
                    Text("Import from Anki").tag(ManualTransferDirection.import)
                    Text("Export to Anki").tag(ManualTransferDirection.export)
                }
                .disabled(model.busy || model.preview != nil || model.mediaPreview != nil)
                .accessibilityIdentifier("settings.anki.direction")
                Picker("Transfer scope", selection: Binding(get: { model.scope }, set: { model.selectScope($0) })) {
                    Text("Collection only").tag(ManualTransferScope.collectionOnly)
                    Text("Collection and media").tag(ManualTransferScope.collectionAndMedia)
                }
                .disabled(model.busy || model.preview != nil || model.mediaPreview != nil)
                .accessibilityIdentifier("settings.anki.scope")
                Text(model.scope == .collectionOnly
                    ? "Collection only. Images, audio and other media stay unchanged on both sides."
                    : "After the collection finishes, a separate decision lets you reconcile media on this device and server. Media can change both sides in either collection direction.")
                if model.direction == .import {
                    Text("Preparation downloads and validates a candidate. Final confirmation replaces this device’s collection after making its required safety copy. It does not merge collections.")
                    action("Download and Prepare Import Preview", id: "prepareImport", disabled: !model.canPrepare) { await model.prepare() }
                } else {
                    Text("Preparation makes a local safety copy, downloads and retains the server’s current collection, and prepares an upload. Downloading changes the server’s bookkeeping. Nothing is uploaded until final confirmation. Export replaces the server collection and activates the prepared version locally.")
                    action("Make Copies and Prepare Export Preview", id: "prepareExport", disabled: !model.canPrepare) { await model.prepare() }
                }
            }
            if let preview = model.preview {
                Section(model.isRecoveryPreview ? "Local Recovery Decision" : "Final Replacement Preview") {
                    recordFacts(preview.record)
                    LabeledContent("Login", value: model.previewLoginLabel)
                    Text("Other clients can change the server after its retained copy was made. Export can discard those later edits; the server does not reserve this preview.")
                    Button(role: .destructive) { model.confirmationPresented = true } label: {
                        Text(model.isRecoveryPreview ? "Confirm Local Recovery…" : (preview.record.direction == .import ? "Replace This Device’s Collection…" : "Replace Server Collection…"))
                            .frame(minWidth: 44, minHeight: 44)
                    }
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
                    if let instant = model.nextEligibleMilliseconds {
                        LabeledContent("Next eligible", value: date(instant))
                            .accessibilityIdentifier("settings.anki.nextEligible")
                    }
                }
            }
            Section("Local Transfer History and Copies") {
                Text("This history loads offline. Restart never resumes a server request. A copy saved below is independent of the retained original; unverified files must be inspected before use.")
                    .foregroundStyle(.token(.textSecondary))
                action("Refresh Local History", id: "history", disabled: model.busy) { await model.refreshOfflineHistory() }
                if model.records.isEmpty { Text("No recorded server transfers.") }
                ForEach(model.records) { entry in
                    VStack(alignment: .leading, spacing: Spacing.small) {
                        Text(entry.record.direction == .import ? "Import from Anki" : "Export to Anki").font(.headline)
                        Text(status(entry.record))
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
                                Text("Save Copy to Folder…").frame(minWidth: 44, minHeight: 44)
                            }
                            .disabled(model.busy)
                            .accessibilityIdentifier("settings.anki.copy.\(entry.id).\(index)")
                        }
                    }
                }
            }
        }
        #if os(iOS)
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                if focusedField != nil {
                    Spacer()
                    Button { focusedField = nil } label: { Text("Done", bundle: .module) }
                        .accessibilityIdentifier("settings.anki.keyboardDone")
                }
            }
        }
        #endif
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
        Button { Task { await run() } } label: { Text(title).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle()) }
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
    func manualTransferPresentation(model: ManualTransferCoordinator) -> some View {
        modifier(ManualTransferPresentation(model: model))
    }
}
private struct ManualTransferPresentation: ViewModifier {
    @Bindable var model: ManualTransferCoordinator
    func body(content: Content) -> some View {
        content
        .destructiveConfirmation(Text("Confirm collection replacement?"), isPresented: $model.confirmationPresented,
            actionTitle: Text(model.isRecoveryPreview ? "Recover Locally" : "Replace Collection"),
            message: Text("\(model.endpoint)\nLogin: \(model.previewLoginLabel)\n\(model.isRecoveryPreview ? "The retained candidate replaces this device’s collection. Nothing is uploaded." : (model.preview?.record.direction == .import ? "The prepared collection replaces this device’s collection." : "The prepared collection replaces the server collection and this device’s active version.")) \(model.preview?.record.mediaSelected == true ? "Media needs a separate decision after collection completion." : "Media is excluded.")")) {
                Task { await model.confirm() }
            }
        .fileImporter(isPresented: $model.copyDestinationPresented, allowedContentTypes: [.folder]) { result in
            Task { await model.copyDestination(result.map { [$0] }) }
        }
        .task { await model.loadOffline() }
        .onDisappear { model.clearPassword() }
    }
 }
