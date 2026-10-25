import AppModel
import DesignSystem
import SwiftUI

public struct CollectionExportSheet: View {
    @Bindable private var coordinator: CollectionExportCoordinator
    public init(coordinator: CollectionExportCoordinator) { self.coordinator = coordinator }
    public var body: some View {
        CollectionExportContent(presentation: .init(coordinator), prepare: coordinator.prepare,
            choose: coordinator.chooseDestination, retry: coordinator.retryCompletion, discard: { Task { await coordinator.discard() } },
            close: { Task { await coordinator.dismiss() } })
    }
}
struct CollectionExportPresentation {
    var busy = false
    var saved = false
    var ready = false
    var canPrepare = true
    var canSave = false
    var canDiscard = false
    var canRetryCompletion = false
    var title = "Preparing collection export…"
    var message: String?
    init() {}
    init(_ coordinator: CollectionExportCoordinator) {
        busy = coordinator.busy; saved = coordinator.stage == .saved; ready = coordinator.canSave
        canPrepare = coordinator.canPrepare; canSave = coordinator.canSave
        canDiscard = !busy && coordinator.artifact != nil && coordinator.providerLease == nil
        canRetryCompletion = coordinator.canRetryCompletion
        title = coordinator.progressTitle
        message = coordinator.message ?? coordinator.availabilityMessage
    }
}
/// Production rendering is also used by the registered snapshot fixtures.
struct CollectionExportContent: View {
    let presentation: CollectionExportPresentation
    let prepare: () -> Void
    let choose: () -> Void
    var retry: () -> Void = {}
    let discard: () -> Void
    let close: () -> Void
    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.large) {
                    Text("The .colpkg file includes every deck, note, card and saved review, plus images, audio and other media.", bundle: .module)
                        .accessibilityIdentifier("settings.importExport.explanation")
                    if presentation.busy {
                        ProgressView { Text(LocalizedStringKey(presentation.title), bundle: .module) }
                            .frame(minWidth: 44, minHeight: 44)
                            .accessibilityLabel(Text(LocalizedStringKey(presentation.title), bundle: .module))
                            .accessibilityValue(Text("In progress", bundle: .module))
                            .accessibilityIdentifier("settings.importExport.progress")
                    } else if presentation.saved {
                        Label { Text("Saved", bundle: .module) } icon: { Image(systemName: "checkmark.circle") }
                            .font(.headline)
                            .accessibilityIdentifier("settings.importExport.saved")
                        Text("The destination has received the file. A file provider may continue uploading its copy.", bundle: .module)
                    } else if presentation.ready {
                        Text("Ready to Save", bundle: .module).font(.headline)
                            .accessibilityIdentifier("settings.importExport.ready")
                        Text("This file captures the collection during preparation. Later reviews do not change this copy. Choose a destination to finish saving.", bundle: .module)
                            .accessibilityIdentifier("settings.importExport.readyDetail")
                    }
                    if let message = presentation.message {
                        Text(LocalizedStringKey(message), bundle: .module)
                            .accessibilityIdentifier("settings.importExport.message")
                    }
                    if presentation.canPrepare {
                        Button(action: prepare) {
                            Text("Prepare Export", bundle: .module).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                        }
                            .accessibilityIdentifier("settings.importExport.prepare")
                    }
                    if presentation.canSave {
                        Button(action: choose) {
                            Text("Choose Save Location", bundle: .module).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                        }
                            .accessibilityIdentifier("settings.importExport.choose")
                    }
                    if presentation.canRetryCompletion {
                        Button(action: retry) {
                            Text("Retry Export Cleanup", bundle: .module).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                        }
                            .accessibilityIdentifier("settings.importExport.retryCleanup")
                    }
                    if presentation.canDiscard {
                        Button(action: discard) {
                            Text("Discard Prepared Export", bundle: .module).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                        }
                            .accessibilityIdentifier("settings.importExport.discard")
                    }
                }
                .foregroundStyle(.token(.textPrimary))
                .padding(Spacing.large)
                .frame(maxWidth: ContentWidth.readable, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .accessibilityIdentifier("settings.importExport.content")
            .background(.token(.backgroundPrimary))
            .safeAreaInset(edge: .bottom) {
                Button(action: close) {
                    Text(presentation.busy ? LocalizedStringKey("Cancel Export") : LocalizedStringKey("Done"), bundle: .module)
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("settings.importExport.close")
                    .padding(Spacing.large)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.token(.backgroundPrimary))
            }
            .buttonStyle(.bordered)
            .tint(ColorToken.textPrimary.color)
            .navigationTitle(Text("Export Collection", bundle: .module))
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
        }
        .interactiveDismissDisabled(presentation.busy)
    }
}
public enum CollectionExportPreview: PreviewStates {
    public static let previewID = "settings.importExport"
    public static var previewFixtures: [PreviewFixture] {
        PreviewState.allCases.map { state in
            PreviewFixture(state) {
                CollectionExportContent(presentation: presentation(state), prepare: {}, choose: {}, discard: {}, close: {})
            }
        }
    }
    private static func presentation(_ state: PreviewState) -> CollectionExportPresentation {
        var value = CollectionExportPresentation()
        switch state {
        case .empty: break
        case .typical: value.ready = true; value.canSave = true; value.canDiscard = true; value.canPrepare = false
        case .overflow:
            value.ready = true; value.canDiscard = true; value.canPrepare = false
            value.message = "The new file may already be saved. Check the destination before trying again. The completed export is retained."
        case .error:
            value.message = "The collection could not be exported. Check available space and collection access, then try again."
        case .loading: value.busy = true; value.canPrepare = false
        }
        return value
    }
}
