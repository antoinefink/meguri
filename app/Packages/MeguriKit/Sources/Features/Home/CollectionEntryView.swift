import AppModel
import CoreBridge
import SwiftUI
import UniformTypeIdentifiers

public struct CollectionEntryView: View {
    @Bindable private var coordinator: CollectionEntryCoordinator
    public init(coordinator: CollectionEntryCoordinator) { self.coordinator = coordinator }
    public var body: some View {
        CollectionEntryContent(presentation: CollectionEntryPresentation(coordinator: coordinator), actions: .init(
            confirm: { coordinator.confirm() },
            chooseFile: { coordinator.chooseFile() },
            reinspect: { coordinator.reinspect() },
            continueRecovery: { Task { await coordinator.continueAfterActivation() } },
            dismiss: { Task { await coordinator.dismiss() } }
        ))
        .fileImporter(isPresented: $coordinator.pickerPresented,
                      allowedContentTypes: [UTType(importedAs: "com.antoinefink.meguri.collection-package")],
                      allowsMultipleSelection: false, onCompletion: coordinator.selection)
    }
}

/// Only values the sheet renders; no inspection handle or source ownership.
struct CollectionEntryPresentation {
    struct Facts {
        let notes: UInt64
        let cards: UInt64
        let reviews: UInt64
        let replacingExisting: Bool
        let mediaCount: UInt64
        let mediaDecodedBytes: UInt64
        init(notes: UInt64, cards: UInt64, reviews: UInt64, replacingExisting: Bool,
             mediaCount: UInt64 = 0, mediaDecodedBytes: UInt64 = 0) {
            self.notes = notes; self.cards = cards; self.reviews = reviews; self.replacingExisting = replacingExisting
            self.mediaCount = mediaCount; self.mediaDecodedBytes = mediaDecodedBytes
        }
    }
    struct Progress {
        let title: String
        let completed: UInt64
        let total: UInt64?
        let canCancel: Bool
    }
    let facts: Facts?
    let progress: Progress?
    let message: String?
    let candidateActive: Bool
    let canChoose: Bool
    let needsReinspection: Bool
    let activated: Bool
    let requiresRecovery: Bool
    let canContinueRecovery: Bool
    let preparedMedia: CollectionImportMedia?
    let finalMedia: CollectionImportMedia?
    let generation: UInt64?
    let backup: ImportBackupReceipt?
    let backupPublication: ImportBackupPublication?
    let mediaReference: String?

    var busy: Bool { progress != nil }
    var canCancel: Bool { progress?.canCancel == true }
    struct MediaRow {
        let label: String
        let count: UInt64
    }
    var mediaCompletionNote: String? {
        finalMedia == nil && preparedMedia != nil
            ? "Prepared media plan. Completion and the live media state need verification." : nil
    }
    var mediaDetailRows: [MediaRow] {
        guard let media = finalMedia ?? preparedMedia else { return [] }
        return Self.mediaRows(media, completed: finalMedia != nil)
    }
    static func mediaRows(_ media: CollectionImportMedia, completed: Bool) -> [MediaRow] {
        [MediaRow(label: "Incoming media files", count: media.incoming),
         MediaRow(label: "Decoded media bytes", count: media.decodedBytes),
         MediaRow(label: completed ? "Reused files" : "Planned reused files", count: media.reused),
         MediaRow(label: completed ? "Added files" : "Planned added files", count: media.added),
         MediaRow(label: completed ? "Files moved to trash" : "Planned files to move to trash", count: media.trashed)]
    }

    init(facts: Facts? = nil, progress: Progress? = nil, message: String? = nil,
         candidateActive: Bool = false, canChoose: Bool = true, needsReinspection: Bool = false,
         activated: Bool = false, requiresRecovery: Bool = false, canContinueRecovery: Bool = false,
         preparedMedia: CollectionImportMedia? = nil, finalMedia: CollectionImportMedia? = nil,
         generation: UInt64? = nil, backup: ImportBackupReceipt? = nil,
         backupPublication: ImportBackupPublication? = nil, mediaReference: String? = nil,
         failure: CollectionEntryFailure? = nil) {
        self.facts = facts; self.progress = progress; self.message = message
        self.candidateActive = candidateActive; self.canChoose = canChoose && !requiresRecovery && !candidateActive
        self.needsReinspection = needsReinspection
        self.activated = activated
        self.requiresRecovery = requiresRecovery || candidateActive; self.canContinueRecovery = canContinueRecovery
        self.preparedMedia = failure?.preparedMedia ?? failure?.media ?? preparedMedia
        self.finalMedia = failure == nil ? finalMedia : failure?.completedActivation?.media
        self.generation = generation
        self.backup = backup; self.backupPublication = backupPublication; self.mediaReference = mediaReference
    }

    @MainActor init(coordinator: CollectionEntryCoordinator) {
        facts = coordinator.preview.map { .init(notes: $0.facts.notes, cards: $0.facts.cards,
            reviews: $0.facts.reviews, replacingExisting: $0.facts.replacingExisting,
            mediaCount: $0.facts.mediaCount, mediaDecodedBytes: $0.facts.mediaDecodedBytes) }
        progress = coordinator.busy ? .init(title: coordinator.progressTitle,
            completed: coordinator.progress?.completed ?? 0, total: coordinator.progress?.total,
            canCancel: coordinator.canCancel) : nil
        message = coordinator.message
        candidateActive = coordinator.failure?.candidateActive == true
        canChoose = coordinator.canChoose
        needsReinspection = coordinator.needsReinspection
        activated = !coordinator.requiresRecovery && (coordinator.lastActivation != nil || coordinator.failure?.candidateActive == true)
        requiresRecovery = coordinator.requiresRecovery
        canContinueRecovery = coordinator.canContinueRecovery
        let mediaPresentation = Self(preparedMedia: coordinator.preparedMedia,
            finalMedia: coordinator.lastActivation?.media, failure: coordinator.failure)
        preparedMedia = mediaPresentation.preparedMedia
        finalMedia = mediaPresentation.finalMedia
        generation = coordinator.failure?.generation ?? coordinator.lastActivation?.generation
        let receipt = coordinator.failure?.backup ?? coordinator.lastActivation?.backup
        if case .safetyBackup(_, let publication)? = coordinator.failure?.issue {
            backupPublication = publication
            if case .durable(let durable) = publication { backup = receipt ?? durable }
            else { backup = receipt }
        } else { backupPublication = nil; backup = receipt }
        mediaReference = coordinator.failure?.mediaRecoveryReference ?? coordinator.lastActivation?.mediaRetentionReference
    }
}

struct CollectionEntryActions {
    let confirm: () -> Void
    let chooseFile: () -> Void
    let reinspect: () -> Void
    let continueRecovery: () -> Void
    let dismiss: () -> Void
}

/// The actual import sheet, shared by the live wrapper and deterministic fixtures.
struct CollectionEntryContent: View {
    let presentation: CollectionEntryPresentation
    let actions: CollectionEntryActions

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("Import a latest-format, collection-only .colpkg file. Packages containing media and older formats are not supported yet. No account or network connection is needed.")
                        .accessibilityIdentifier("onboarding.collectionImport.explanation")
                }
                if let facts = presentation.facts {
                    Section {
                        Text("\(facts.notes) notes · \(facts.cards) cards · \(facts.reviews) reviews")
                            .accessibilityIdentifier("onboarding.collectionImport.preview.counts")
                        LabeledContent("Destination", value: "this device")
                            .accessibilityIdentifier("onboarding.collectionImport.preview.destination")
                        LabeledContent("Validated media files", value: facts.mediaCount.formatted())
                            .accessibilityIdentifier("onboarding.collectionImport.preview.media.files")
                        LabeledContent("Decoded media bytes", value: facts.mediaDecodedBytes.formatted())
                            .accessibilityIdentifier("onboarding.collectionImport.preview.media.bytes")
                        Text(facts.replacingExisting
                            ? "This will replace the local collection on this device. Meguri must finish a safety backup of your current collection, including saved answers, before replacing it. If that backup fails, import stops."
                            : "There is no local collection file to replace. A pre-import safety backup is not needed.")
                            .accessibilityIdentifier("onboarding.collectionImport.preview.safety")
                        Text("Existing media is kept. Supplied media is merged by name and content; replaced versions are saved in trash. Restoring a safety backup’s media needs those saved files.")
                            .accessibilityIdentifier("onboarding.collectionImport.preview.media.retention")
                        Button(facts.replacingExisting ? "Replace Collection" : "Import Collection", action: actions.confirm)
                            .keyboardShortcut(.defaultAction)
                            .accessibilityIdentifier("onboarding.collectionImport.confirm")
                    } header: {
                        Text("Collection Preview")
                            .accessibilityIdentifier("onboarding.collectionImport.preview.heading")
                    }
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("onboarding.collectionImport.preview")
                }
                if let progress = presentation.progress {
                    Section {
                        Text(verbatim: progress.title)
                            .accessibilityIdentifier("onboarding.collectionImport.progress.stage")
                        Group {
                            if let total = progress.total {
                                ProgressView(value: Double(progress.completed), total: Double(max(1, total)))
                            } else { ProgressView() }
                        }
                        .accessibilityLabel(progress.title)
                        .accessibilityValue(Text(progress.total.map { "\(progress.completed) of \($0)" } ?? "In progress"))
                        .accessibilityIdentifier("onboarding.collectionImport.progress.value")
                        if let media = presentation.preparedMedia {
                            Text("Prepared media plan")
                            mediaRows(CollectionEntryPresentation.mediaRows(media, completed: false))
                                .accessibilityIdentifier("onboarding.collectionImport.progress.media")
                        }
                        if !presentation.canCancel {
                            Text("Finishing activation. Cancellation is no longer available.")
                        }
                    }
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("onboarding.collectionImport.progress")
                }
                if let message = presentation.message {
                    Section {
                        Text(verbatim: message)
                            .accessibilityIdentifier("onboarding.collectionImport.error")
                        if presentation.canContinueRecovery {
                            Button("Continue Recovery", action: actions.continueRecovery)
                                .accessibilityIdentifier("onboarding.collectionImport.recover")
                        }
                    }
                }
                if presentation.finalMedia != nil || (!presentation.busy && presentation.preparedMedia != nil) || presentation.generation != nil || presentation.backup != nil || presentation.backupPublication != nil || presentation.mediaReference != nil {
                    Section("Import Details") {
                        if presentation.activated {
                            Text("Collection imported.")
                                .accessibilityIdentifier("onboarding.collectionImport.result.complete")
                        }
                        if let generation = presentation.generation {
                            LabeledContent("Collection generation", value: generation.formatted())
                                .accessibilityIdentifier("onboarding.collectionImport.result.generation")
                        }
                        if !presentation.busy && !presentation.mediaDetailRows.isEmpty {
                            if let note = presentation.mediaCompletionNote {
                                Text(verbatim: note)
                            }
                            mediaRows(presentation.mediaDetailRows)
                                .accessibilityIdentifier("onboarding.collectionImport.result.media")
                        }
                        if let backup = presentation.backup {
                            LabeledContent("Safety backup", value: backup.name)
                            LabeledContent("Backup bytes", value: backup.sizeBytes.formatted())
                                .accessibilityIdentifier("onboarding.collectionImport.result.backup")
                        }
                        if case .uncertain? = presentation.backupPublication {
                            Text("A safety backup may have been saved. Its publication needs verification.")
                        }
                        if let reference = presentation.mediaReference {
                            LabeledContent("Retained media reference", value: reference)
                                .accessibilityIdentifier("onboarding.collectionImport.result.media.reference")
                            Text("Keep the saved media files with the safety backup. They are needed to restore its media.")
                        }
                    }
                }
                Section {
                    if presentation.canChoose {
                        if presentation.needsReinspection {
                            Button("Inspect Again", action: actions.reinspect)
                                .accessibilityIdentifier("onboarding.collectionImport.reinspect")
                        }
                        Button("Choose File", action: actions.chooseFile)
                            .accessibilityIdentifier("onboarding.collectionImport.choose")
                    }
                    Button(presentation.busy ? "Cancel Import" : "Cancel", action: actions.dismiss)
                        .keyboardShortcut(.cancelAction)
                        .disabled((presentation.busy && !presentation.canCancel) || presentation.requiresRecovery)
                        .accessibilityIdentifier("onboarding.collectionImport.cancel")
                }
            }
            .accessibilityIdentifier("onboarding.collectionImport.content")
            .navigationTitle("Import Collection")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
        }
        .interactiveDismissDisabled(presentation.busy || presentation.requiresRecovery)
    }

    @ViewBuilder private func mediaRows(_ rows: [CollectionEntryPresentation.MediaRow]) -> some View {
        ForEach(rows, id: \.label) { row in
            LabeledContent { Text(row.count.formatted()) } label: { Text(verbatim: row.label) }
        }
    }
}
