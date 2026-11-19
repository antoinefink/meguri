import AppModel
import DesignSystem
import SwiftUI

/// N17.3: Export Collection prepares the file at once, with progress, then the
/// system save dialog opens over this sheet. The sheet has no steps of its own.
public struct CollectionExportSheet: View {
    @Bindable private var coordinator: CollectionExportCoordinator
    public init(coordinator: CollectionExportCoordinator) { self.coordinator = coordinator }
    public var body: some View {
        CollectionExportContent(presentation: .init(coordinator), choose: coordinator.chooseDestination,
            retry: coordinator.retryCompletion, close: { Task { await coordinator.cancel() } })
    }
}
struct CollectionExportPresentation {
    struct Progress: Equatable {
        var completed: UInt64
        var total: UInt64
    }
    var busy = true
    var canCancel = true
    var saved = false
    var canChooseAgain = false
    var canRetryCompletion = false
    var title = "Preparing Export…"
    var progress: Progress?
    var message: String?
    init() {}
    init(_ coordinator: CollectionExportCoordinator) {
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
        message = coordinator.message ?? coordinator.availabilityMessage
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
                        .accessibilityIdentifier("settings.importExport.progress")
                    }
                }
                if let message = presentation.message {
                    Section {
                        Text(LocalizedStringKey(message), bundle: .module)
                            .accessibilityIdentifier("settings.importExport.message")
                        if presentation.canChooseAgain {
                            Button(action: choose) { Text("Choose Another Location", bundle: .module) }
                                .accessibilityIdentifier("settings.importExport.choose")
                        }
                        if presentation.canRetryCompletion {
                            Button(action: retry) { Text("Retry Export Cleanup", bundle: .module) }
                                .accessibilityIdentifier("settings.importExport.retryCleanup")
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .accessibilityIdentifier("settings.importExport.content")
            .navigationTitle(Text("Export Collection", bundle: .module))
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    // A plain titled button, so the system draws its own bar button.
                    Button(presentation.saved ? String(localized: "Done", bundle: .module) : String(localized: "Cancel", bundle: .module),
                           action: close)
                    .keyboardShortcut(.cancelAction)
                    .disabled(presentation.busy && !presentation.canCancel)
                    .accessibilityIdentifier("settings.importExport.close")
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
