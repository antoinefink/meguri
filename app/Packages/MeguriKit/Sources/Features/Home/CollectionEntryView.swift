import AppModel
import DesignSystem
import SwiftUI
import UniformTypeIdentifiers

public struct CollectionEntryView: View {
    @Bindable private var coordinator: CollectionEntryCoordinator
    public init(coordinator: CollectionEntryCoordinator) { self.coordinator = coordinator }
    public var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.large) {
                    Text("Import a latest-format, collection-only .colpkg file. Packages containing media and older formats are not supported yet. No account or network connection is needed.")
                    if let facts = coordinator.preview?.facts {
                        VStack(alignment: .leading, spacing: Spacing.small) {
                            Text("Collection Preview").font(.headline)
                            Text("\(facts.notes) notes · \(facts.cards) cards · \(facts.reviews) reviews")
                                .accessibilityIdentifier("entry.preview.counts")
                            Text("Destination: this device")
                            Text(facts.replacingExisting
                                ? "This will replace the local collection on this device. Meguri must finish a safety backup of your current collection, including saved answers, before replacing it. If that backup fails, import stops."
                                : "There is no local collection file to replace. A pre-import safety backup is not needed.")
                                .accessibilityIdentifier("entry.preview.safety")
                            Button(facts.replacingExisting ? "Replace Collection" : "Import Collection") { coordinator.confirm() }
                                .buttonStyle(.borderedProminent)
                                .keyboardShortcut(.defaultAction)
                                .frame(minHeight: 44)
                                .accessibilityIdentifier("entry.confirm")
                        }
                        .accessibilityIdentifier("entry.preview")
                    }
                    if coordinator.busy {
                        VStack(alignment: .leading, spacing: Spacing.small) {
                            Text(verbatim: coordinator.progressTitle)
                                .accessibilityIdentifier("entry.progress.stage")
                            if let update = coordinator.progress, let total = update.total {
                                ProgressView(value: Double(update.completed), total: Double(max(1, total)))
                                    .accessibilityLabel(coordinator.progressTitle)
                            } else { ProgressView().accessibilityLabel(coordinator.progressTitle) }
                            if !coordinator.canCancel { Text("Finishing activation. The new collection will remain active.") }
                        }
                        .accessibilityIdentifier("entry.progress")
                    }
                    if let message = coordinator.message {
                        Text(verbatim: message).foregroundStyle(.token(.statusError))
                            .accessibilityIdentifier("entry.error")
                    }
                    if coordinator.failure?.candidateActive == true {
                        Button("Continue Recovery") { Task { await coordinator.continueAfterActivation() } }
                            .frame(minHeight: 44).accessibilityIdentifier("entry.recover")
                    } else if coordinator.canChoose {
                        if coordinator.needsReinspection {
                            Button("Inspect Again") { coordinator.reinspect() }
                                .frame(minHeight: 44).accessibilityIdentifier("entry.reinspect")
                        }
                        Button("Choose File") { coordinator.chooseFile() }
                            .frame(minHeight: 44).accessibilityIdentifier("entry.choose")
                    }
                }
                .padding(Spacing.large)
                .frame(maxWidth: ContentWidth.readable, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .navigationTitle("Import Collection")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(coordinator.busy ? "Cancel Import" : "Cancel") { Task { await coordinator.dismiss() } }
                        .keyboardShortcut(.cancelAction)
                        .disabled((coordinator.busy && !coordinator.canCancel) || coordinator.failure?.candidateActive == true)
                        .accessibilityIdentifier("entry.cancel")
                }
            }
        }
        .fileImporter(isPresented: $coordinator.pickerPresented,
                      allowedContentTypes: [UTType(importedAs: "com.antoinefink.meguri.collection-package")],
                      allowsMultipleSelection: false, onCompletion: coordinator.selection)
        .interactiveDismissDisabled(coordinator.busy || coordinator.failure?.candidateActive == true)
        .accessibilityIdentifier("entry.sheet")
    }
}
