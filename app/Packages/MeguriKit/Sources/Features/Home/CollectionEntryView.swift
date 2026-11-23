import AppModel
import CoreBridge
import SwiftUI
import UniformTypeIdentifiers
#if os(iOS)
import UIKit
#endif

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
        .modifier(CollectionImportPicker(coordinator: coordinator, inSheet: true))
    }
}

/// N17.2: Import opens the system file picker directly, on the window that
/// asked. While the import sheet shows (a refused file, "Choose Another
/// File"), the sheet presents the picker instead.
public struct CollectionImportPicker: ViewModifier {
    @Environment(\.filesPickerDirectory) private var directory
    private let coordinator: CollectionEntryCoordinator?
    private let hosts: Bool
    public init(coordinator: CollectionEntryCoordinator?, hosts: Bool) {
        self.coordinator = coordinator
        self.hosts = hosts && coordinator?.pickerInSheet == false
    }
    init(coordinator: CollectionEntryCoordinator, inSheet: Bool) {
        self.coordinator = coordinator
        hosts = coordinator.pickerInSheet == inSheet
    }
    public func body(content: Content) -> some View {
        content.fileImporter(
            isPresented: Binding(get: { hosts && coordinator?.pickerPresented == true },
                                 set: { if !$0, hosts { coordinator?.pickerPresented = false } }),
            allowedContentTypes: [UTType(importedAs: "com.antoinefink.meguri.collection-package")],
            allowsMultipleSelection: false) { coordinator?.selection($0) }
            .fileDialogDefaultDirectory(directory)
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
    var fileName: String?

    /// N17.2's confirmation, in plain words.
    var confirmationTitle: String { fileName.map { "Import “\($0)”?" } ?? "Import This Collection?" }
    static func count(_ value: UInt64, _ one: String, _ other: String) -> String {
        "\(value.formatted()) \(value == 1 ? one : other)"
    }
    static func summary(_ facts: Facts) -> String {
        [count(facts.notes, "note", "notes"), count(facts.cards, "card", "cards"),
         count(facts.mediaCount, "media file", "media files")].joined(separator: " · ")
    }
    static var deviceName: String {
        #if os(macOS)
        "Mac"
        #else
        UIDevice.current.userInterfaceIdiom == .pad ? "iPad" : "iPhone"
        #endif
    }
    /// One line, and only when a collection is about to be replaced.
    static func consequence(_ facts: Facts, device: String = deviceName) -> String? {
        facts.replacingExisting ? "This replaces the collection on this \(device). Meguri keeps a backup of the current one." : nil
    }
    /// package/container P16.4 asks for the media size and for what happens to
    /// media already here; one footnote carries both.
    static func mediaNote(_ facts: Facts, device: String = deviceName) -> String? {
        // A file without media has nothing to merge.
        guard facts.mediaCount > 0 else { return nil }
        let size = Int64(clamping: facts.mediaDecodedBytes).formatted(.byteCount(style: .file))
        return "Its media (\(size)) is merged with the media on this \(device); files already here are kept."
    }
    /// Receipts the person must keep; shown with a failure, never in the ordinary flow.
    var showsRetainedDetails: Bool {
        !busy && facts == nil && (requiresRecovery || backup != nil || backupPublication != nil || mediaReference != nil
            || (message != nil && generation != nil))
    }

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
         failure: CollectionEntryFailure? = nil, fileName: String? = nil) {
        self.fileName = fileName
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
        fileName = coordinator.selectedFileName
        progress = coordinator.busy ? .init(title: coordinator.plainProgressTitle,
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
                if let progress = presentation.progress {
                    Section {
                        Group {
                            if let total = progress.total {
                                ProgressView(value: Double(min(progress.completed, total)), total: Double(max(1, total))) {
                                    Text(verbatim: progress.title)
                                }
                            } else { ProgressView { Text(verbatim: progress.title) } }
                        }
                        .accessibilityLabel(progress.title)
                        .accessibilityValue(Text(progress.total.map { "\(progress.completed) of \($0)" } ?? "In progress"))
                        .accessibilityIdentifier("onboarding.collectionImport.progress.value")
                    } footer: {
                        if !presentation.canCancel {
                            Text("Finishing. This step can’t be cancelled.")
                        }
                    }
                } else if let facts = presentation.facts {
                    Section {
                        VStack(alignment: .leading) {
                            Text(verbatim: presentation.confirmationTitle)
                                .font(.headline)
                                .accessibilityAddTraits(.isHeader)
                                .accessibilityIdentifier("onboarding.collectionImport.preview.heading")
                            Text(verbatim: CollectionEntryPresentation.summary(facts))
                                .foregroundStyle(.secondary)
                                .accessibilityValue(Text(verbatim: CollectionEntryPresentation.count(facts.reviews, "review", "reviews")))
                                .accessibilityIdentifier("onboarding.collectionImport.preview.counts")
                            // Inside the row, where secondary text has the row's
                            // background behind it and scales with the text size.
                            if let note = CollectionEntryPresentation.mediaNote(facts) {
                                Text(verbatim: note)
                                    .foregroundStyle(.secondary)
                                    .accessibilityIdentifier("onboarding.collectionImport.preview.media")
                            }
                        }
                        if let consequence = CollectionEntryPresentation.consequence(facts) {
                            Text(verbatim: consequence)
                                .accessibilityIdentifier("onboarding.collectionImport.preview.safety")
                        }
                    }
                }
                if let message = presentation.message {
                    Section {
                        Label {
                            Text(verbatim: message)
                        } icon: {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .symbolRenderingMode(.multicolor)
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("onboarding.collectionImport.error")
                        if presentation.canContinueRecovery {
                            Button("Continue Recovery", action: actions.continueRecovery)
                                .accessibilityIdentifier("onboarding.collectionImport.recover")
                        }
                    }
                }
                if presentation.showsRetainedDetails {
                    Section("Import Details") {
                        if presentation.activated {
                            Text("Collection imported.")
                                .accessibilityIdentifier("onboarding.collectionImport.result.complete")
                        }
                        if let generation = presentation.generation {
                            LabeledContent("Collection generation", value: generation.formatted())
                                .accessibilityIdentifier("onboarding.collectionImport.result.generation")
                        }
                        if !presentation.mediaDetailRows.isEmpty {
                            if let note = presentation.mediaCompletionNote {
                                Text(verbatim: note)
                            }
                            mediaRows(presentation.mediaDetailRows)
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
                if presentation.canChoose, presentation.facts == nil {
                    Section {
                        if presentation.needsReinspection {
                            Button("Inspect Again", action: actions.reinspect)
                                .accessibilityIdentifier("onboarding.collectionImport.reinspect")
                        }
                        Button(presentation.message == nil ? "Choose File" : "Choose Another File", action: actions.chooseFile)
                            .accessibilityIdentifier("onboarding.collectionImport.choose")
                    }
                }
            }
            .formStyle(.grouped)
            .accessibilityIdentifier("onboarding.collectionImport.content")
            .navigationTitle("Import Collection")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: actions.dismiss)
                        .keyboardShortcut(.cancelAction)
                        .disabled((presentation.busy && !presentation.canCancel) || presentation.requiresRecovery)
                        .accessibilityIdentifier("onboarding.collectionImport.cancel")
                }
                if let facts = presentation.facts, !presentation.busy {
                    if facts.replacingExisting {
                        // package/transfer-sheets X3: replacing a collection is a
                        // destructive action and never the default; Return confirms nothing.
                        ToolbarItem(placement: .destructiveAction) {
                            Button("Import", role: .destructive, action: actions.confirm)
                                .accessibilityIdentifier("onboarding.collectionImport.confirm")
                        }
                    } else {
                        ToolbarItem(placement: .confirmationAction) {
                            Button("Import", action: actions.confirm)
                                .buttonStyle(.borderedProminent)
                                .keyboardShortcut(.defaultAction)
                                .accessibilityIdentifier("onboarding.collectionImport.confirm")
                        }
                    }
                }
            }
        }
        .presentationSizing(.form)
        .interactiveDismissDisabled(presentation.busy || presentation.requiresRecovery)
    }

    @ViewBuilder private func mediaRows(_ rows: [CollectionEntryPresentation.MediaRow]) -> some View {
        ForEach(rows, id: \.label) { row in
            LabeledContent { Text(row.count.formatted()) } label: { Text(verbatim: row.label) }
        }
    }
}
