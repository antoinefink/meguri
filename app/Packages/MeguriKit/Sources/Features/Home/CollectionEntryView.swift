import AppModel
import DesignSystem
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

    var busy: Bool { progress != nil }
    var canCancel: Bool { progress?.canCancel == true }

    init(facts: Facts? = nil, progress: Progress? = nil, message: String? = nil,
         candidateActive: Bool = false, canChoose: Bool = true, needsReinspection: Bool = false) {
        self.facts = facts; self.progress = progress; self.message = message
        self.candidateActive = candidateActive; self.canChoose = canChoose; self.needsReinspection = needsReinspection
    }

    @MainActor init(coordinator: CollectionEntryCoordinator) {
        facts = coordinator.preview.map { .init(notes: $0.facts.notes, cards: $0.facts.cards,
            reviews: $0.facts.reviews, replacingExisting: $0.facts.replacingExisting) }
        progress = coordinator.busy ? .init(title: coordinator.progressTitle,
            completed: coordinator.progress?.completed ?? 0, total: coordinator.progress?.total,
            canCancel: coordinator.canCancel) : nil
        message = coordinator.message
        candidateActive = coordinator.failure?.candidateActive == true
        canChoose = coordinator.canChoose
        needsReinspection = coordinator.needsReinspection
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
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.large) {
                    Text("Import a latest-format, collection-only .colpkg file. Packages containing media and older formats are not supported yet. No account or network connection is needed.")
                        .foregroundStyle(.token(.textPrimary))
                        .accessibilityIdentifier("onboarding.collectionImport.explanation")
                    if let facts = presentation.facts {
                        VStack(alignment: .leading, spacing: Spacing.small) {
                            Text("Collection Preview").font(.headline)
                            Text("\(facts.notes) notes · \(facts.cards) cards · \(facts.reviews) reviews")
                                .accessibilityIdentifier("onboarding.collectionImport.preview.counts")
                            Text("Destination: this device")
                            Text(facts.replacingExisting
                                ? "This will replace the local collection on this device. Meguri must finish a safety backup of your current collection, including saved answers, before replacing it. If that backup fails, import stops."
                                : "There is no local collection file to replace. A pre-import safety backup is not needed.")
                                .accessibilityIdentifier("onboarding.collectionImport.preview.safety")
                            Button(action: actions.confirm) {
                                Text(facts.replacingExisting ? "Replace Collection" : "Import Collection")
                                    .font(.headline)
                                    .frame(minWidth: 44, minHeight: 44)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.bordered)
                            .tint(ColorToken.textPrimary.color)
                            .keyboardShortcut(.defaultAction)
                            .accessibilityIdentifier("onboarding.collectionImport.confirm")
                        }
                        .accessibilityElement(children: .contain)
                        .accessibilityIdentifier("onboarding.collectionImport.preview")
                    }
                    if let progress = presentation.progress {
                        VStack(alignment: .leading, spacing: Spacing.small) {
                            Text(verbatim: progress.title)
                                .accessibilityIdentifier("onboarding.collectionImport.progress.stage")
                            VStack {
                                if let total = progress.total {
                                    ProgressView(value: Double(progress.completed), total: Double(max(1, total)))
                                } else { ProgressView() }
                            }
                            .frame(minWidth: 44, minHeight: 44)
                            .contentShape(.accessibility, Rectangle())
                            .accessibilityElement(children: .ignore)
                            .accessibilityLabel(progress.title)
                            .accessibilityValue(Text(progress.total.map { "\(progress.completed) of \($0)" } ?? "In progress"))
                            .accessibilityIdentifier("onboarding.collectionImport.progress.value")
                            if !presentation.canCancel { Text("Finishing activation. The new collection will remain active.") }
                        }
                        .accessibilityElement(children: .contain)
                        .accessibilityIdentifier("onboarding.collectionImport.progress")
                    }
                    if let message = presentation.message {
                        Text(verbatim: message).foregroundStyle(.token(.statusError))
                            .accessibilityIdentifier("onboarding.collectionImport.error")
                    }
                    if presentation.candidateActive {
                        Button(action: actions.continueRecovery) {
                            Text("Continue Recovery")
                                .frame(minWidth: 44, minHeight: 44)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.bordered)
                        .tint(ColorToken.textPrimary.color)
                        .accessibilityIdentifier("onboarding.collectionImport.recover")
                    } else if presentation.canChoose {
                        if presentation.needsReinspection {
                            Button(action: actions.reinspect) {
                                Text("Inspect Again")
                                    .frame(minWidth: 44, minHeight: 44)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.bordered)
                            .tint(ColorToken.textPrimary.color)
                            .accessibilityIdentifier("onboarding.collectionImport.reinspect")
                        }
                        Button(action: actions.chooseFile) {
                            Text("Choose File")
                                .frame(minWidth: 44, minHeight: 44)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.bordered)
                        .tint(ColorToken.textPrimary.color)
                        .accessibilityIdentifier("onboarding.collectionImport.choose")
                    }
                }
                .padding(Spacing.large)
                .frame(maxWidth: ContentWidth.readable, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .accessibilityIdentifier("onboarding.collectionImport.content")
            .safeAreaInset(edge: .bottom) {
                Button(action: actions.dismiss) {
                    Text(presentation.busy ? "Cancel Import" : "Cancel")
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.bordered)
                .tint(ColorToken.textPrimary.color)
                .keyboardShortcut(.cancelAction)
                .disabled((presentation.busy && !presentation.canCancel) || presentation.candidateActive)
                .accessibilityIdentifier("onboarding.collectionImport.cancel")
                .padding(Spacing.large)
                .frame(maxWidth: ContentWidth.readable, alignment: .leading)
                .frame(maxWidth: .infinity)
                .background(.token(.backgroundPrimary))
            }
            .navigationTitle("Import Collection")
        }
        .interactiveDismissDisabled(presentation.busy || presentation.candidateActive)
    }
}
