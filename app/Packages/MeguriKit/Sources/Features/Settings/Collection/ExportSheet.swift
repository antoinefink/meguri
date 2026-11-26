import AppModel
import DesignSystem
import SwiftUI

/// N17.3: Export Collection prepares the file at once, with progress, then the
/// system save dialog opens over this sheet. The sheet has no steps of its own.
public struct CollectionExportSheet: View {
    @Bindable private var coordinator: CollectionExportCoordinator
    public init(coordinator: CollectionExportCoordinator) { self.coordinator = coordinator }
    public var body: some View {
        if coordinator.showsDeckOptions {
            DeckExportOptionsForm(name: coordinator.deckName ?? "", options: $coordinator.deckOptions,
                canExport: coordinator.canPrepare, message: coordinator.message,
                export: coordinator.exportDeck, cancel: { Task { await coordinator.cancel() } })
        } else {
            CollectionExportContent(presentation: .init(coordinator), choose: coordinator.chooseDestination,
                retry: coordinator.retryCompletion, close: { Task { await coordinator.cancel() } })
        }
    }
}
struct CollectionExportPresentation {
    struct Progress: Equatable {
        var completed: UInt64
        var total: UInt64
    }
    var navigationTitle = "Export Collection"
    var identifierPrefix = "settings.importExport"
    var busy = true
    var canCancel = true
    var saved = false
    var canChooseAgain = false
    var canRetryCompletion = false
    var title = "Preparing Export…"
    var progress: Progress?
    var message: String?
    var summary: String?
    var confirmsEmptyDeck = false
    init() {}
    init(_ coordinator: CollectionExportCoordinator) {
        navigationTitle = coordinator.title; identifierPrefix = coordinator.identifierPrefix
        busy = coordinator.busy || (coordinator.continuous && coordinator.stage == .idle)
        // Not while the system dialog or the save itself owns the file.
        canCancel = !coordinator.destinationPresented && coordinator.stage != .discarding && coordinator.stage != .saving
        saved = coordinator.stage == .saved
        canChooseAgain = coordinator.canChooseAgain
        canRetryCompletion = coordinator.canRetryCompletion
        title = coordinator.plainProgressTitle
        if coordinator.stage == .preparing, let value = coordinator.progress, let total = value.total, total > 0 {
            progress = .init(completed: min(value.completed, total), total: total)
        }
        summary = coordinator.deckExportSummary
        confirmsEmptyDeck = coordinator.confirmsEmptyDeck
        message = coordinator.message ?? coordinator.availabilityMessage
        if confirmsEmptyDeck { message = "This deck contains zero notes. You can save an empty deck package." }
    }
}
/// Production rendering is also used by the registered snapshot fixtures.
struct CollectionExportContent: View {
    let presentation: CollectionExportPresentation
    let choose: () -> Void
    var retry: () -> Void = {}
    let close: () -> Void
    var body: some View {
        NavigationStack {
            Form {
                if presentation.busy {
                    Section {
                        Group {
                            if let progress = presentation.progress {
                                ProgressView(value: Double(progress.completed), total: Double(progress.total)) {
                                    Text(LocalizedStringKey(presentation.title), bundle: .module)
                                }
                            } else {
                                ProgressView { Text(LocalizedStringKey(presentation.title), bundle: .module) }
                            }
                        }
                        .accessibilityLabel(Text(LocalizedStringKey(presentation.title), bundle: .module))
                        .accessibilityValue(Text("In progress", bundle: .module))
                        .accessibilityIdentifier("\(presentation.identifierPrefix).progress")
                    }
                }
                if let summary = presentation.summary {
                    Section {
                        Text(verbatim: summary)
                            .accessibilityIdentifier("\(presentation.identifierPrefix).summary")
                    }
                }
                if let message = presentation.message {
                    Section {
                        Text(LocalizedStringKey(message), bundle: .module)
                            .accessibilityIdentifier("\(presentation.identifierPrefix).message")
                        if presentation.canChooseAgain {
                            Button(action: choose) { Text("Choose Another Location", bundle: .module) }
                                .accessibilityIdentifier("\(presentation.identifierPrefix).choose")
                        }
                        if presentation.canRetryCompletion {
                            Button(action: retry) { Text("Retry Export Cleanup", bundle: .module) }
                                .accessibilityIdentifier("\(presentation.identifierPrefix).retryCleanup")
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .accessibilityIdentifier("\(presentation.identifierPrefix).content")
            .navigationTitle(Text(LocalizedStringKey(presentation.navigationTitle), bundle: .module))
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                if presentation.confirmsEmptyDeck {
                    ToolbarItem(placement: .confirmationAction) {
                        Button(action: choose) { Text("Save…", bundle: .module) }
                            .keyboardShortcut(.defaultAction)
                            .accessibilityIdentifier("\(presentation.identifierPrefix).save")
                    }
                }
                ToolbarItem(placement: .cancellationAction) {
                    // A plain titled button, so the system draws its own bar button.
                    Button(presentation.saved ? String(localized: "Done", bundle: .module) : String(localized: "Cancel", bundle: .module),
                           action: close)
                    .keyboardShortcut(.cancelAction)
                    .disabled(presentation.busy && !presentation.canCancel)
                    .accessibilityIdentifier("\(presentation.identifierPrefix).close")
                }
            }
        }
        .presentationSizing(.form)
        .interactiveDismissDisabled()
    }
}
public enum CollectionExportPreview: PreviewStates {
    public static let previewID = "settings.importExport"
    public static var previewFixtures: [PreviewFixture] {
        PreviewState.allCases.map { state in
            PreviewFixture(state) {
                CollectionExportContent(presentation: presentation(state), choose: {}, close: {})
            }
        }
    }
    private static func presentation(_ state: PreviewState) -> CollectionExportPresentation {
        var value = CollectionExportPresentation()
        switch state {
        case .empty: break
        case .typical: value.progress = .init(completed: 42, total: 100)
        case .overflow:
            value.busy = false; value.canChooseAgain = true
            value.message = "The file could not be saved. The completed export is retained; choose another destination and try again."
        case .error:
            value.busy = false
            value.message = "The collection could not be exported. Check available space and collection access, then try again."
        case .loading: value.title = "Saving…"; value.canCancel = false
        }
        return value
    }
}
