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
                    .disabled(model.busy || model.preview != nil)
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
                .disabled(model.busy || model.preview != nil)
                .accessibilityIdentifier("settings.anki.direction")
                Text("Collection only. Images, audio and other media stay unchanged on both sides. Media reconciliation is not available in this flow.")
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
                        if entry.record.acceptance == .possiblyAccepted {
                            Text("Upload completion is unknown. Another transfer remains blocked. A retry or risk-acceptance clear is not available.")
                        } else if entry.record.canInspectRecovery {
                            action("Inspect Local Recovery", id: "recover.\(entry.id)", disabled: model.busy || model.preview != nil) { await model.recover(entry) }
                        }
                        if [.preparing, .preview, .needsDecision].contains(entry.record.phase), entry.record.acceptance == .notSent {
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
                if model.progress?.cancellable != false {
                    action("Cancel Operation", id: "cancel", disabled: false) {
                        await model.cancel()
                    }
                }
            }
        }
    }
    private var operationDescription: String {
        switch (model.stage, model.progress?.cancellable) {
        case (.applyingDecision, .some(false)): "Activating the confirmed collection…"
        case (.applyingDecision, _): "Transferring the collection…"
        case (.authenticating, _): "Getting a login key for this device…"
        case (.buildingDecision, _): "Saving safety copies and preparing the preview…"
        case (.inspectingCandidate, _): "Inspecting the local recovery collection…"
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
        if record.phase == .complete { return "Collection complete. Media was excluded." }
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
            message: Text("\(model.endpoint)\nLogin: \(model.previewLoginLabel)\n\(model.isRecoveryPreview ? "The retained candidate replaces this device’s collection. Nothing is uploaded." : (model.preview?.record.direction == .import ? "The prepared collection replaces this device’s collection." : "The prepared collection replaces the server collection and this device’s active version.")) Media is excluded.")) {
                Task { await model.confirm() }
            }
        .fileImporter(isPresented: $model.copyDestinationPresented, allowedContentTypes: [.folder]) { result in
            Task { await model.copyDestination(result.map { [$0] }) }
        }
        .task { await model.loadOffline() }
        .onDisappear { model.clearPassword() }
    }
 }
