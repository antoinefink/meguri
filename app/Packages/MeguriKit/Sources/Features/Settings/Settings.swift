import AppModel
import DesignSystem
import SwiftUI

/// Independent Files actions and explicit manual server transfers.
public struct SettingsView: View {
    private let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        Form {
            ManualTransferSections(model: collection.manualTransfers, area: .connectionAndTransfers)
            SettingsCollectionFilesSection(collection: collection)
            ManualTransferSections(model: collection.manualTransfers, area: .history)
        }
        #if os(macOS)
        .formStyle(.grouped)
        #endif
        .frame(maxWidth: ContentWidth.readable)
        .frame(maxWidth: .infinity)
        .background(.token(.backgroundPrimary))
        .manualTransferPresentation(model: collection.manualTransfers)
        .navigationTitle(Text("Settings", bundle: .module))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("settings.root.content")
    }
}

/// The same collection-only Files actions appear in Settings render fixtures.
struct SettingsCollectionFilesSection: View {
    let collection: ReviewCollection?
    var present: ((SheetRoute) -> Void)? = nil
    private var fileActionsAvailable: Bool {
        guard let collection else { return false }
        return collection.navigation.sheet == nil && !collection.manualTransfers.busy
            && collection.manualTransfers.preview == nil && collection.manualTransfers.mediaPreview == nil
    }
    var body: some View {
        Section {
            Button { if let present { present(.collectionEntry) } else { collection?.entry.present() } } label: {
                Text("Import Collection", bundle: .module).fixedSize(horizontal: false, vertical: true).frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget).contentShape(Rectangle())
            }
                .disabled(!fileActionsAvailable || collection?.entry.canPresent != true)
                .accessibilityIdentifier("settings.root.importCollection")
            Button { if let present { present(.collectionExport) } else { collection?.collectionExport.present() } } label: {
                Text("Export Collection", bundle: .module).fixedSize(horizontal: false, vertical: true).frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget).contentShape(Rectangle())
            }
                .disabled(!fileActionsAvailable || collection?.collectionExport.canPresent != true)
                .accessibilityIdentifier("settings.root.exportCollection")
            Text("Save a latest-format .colpkg containing every deck, note, card and saved review. Images, audio and other media are excluded. No account or connection is needed.", bundle: .module)
                .foregroundStyle(.token(.textSecondary))
                .fixedSize(horizontal: false, vertical: true)
        } header: { Text("Collection Files", bundle: .module) }
    }

}

#if os(macOS)
/// Native Settings shares the collection. Only this device-local section is restored.
public struct MacSettingsView: View {
    private let collection: ReviewCollection
    private let present: (SheetRoute) -> Void
    @AppStorage("settings.selectedSection") private var selectedSection = "connection"
    public init(collection: ReviewCollection, present: @escaping (SheetRoute) -> Void) {
        self.collection = collection; self.present = present
    }
    public var body: some View {
        NavigationSplitView(columnVisibility: .constant(.all)) {
            List(selection: Binding<String?>(get: { selectedSection }, set: { if let value = $0, value != selectedSection { selectedSection = value } })) {
                Label { Text("Connection", bundle: .module) } icon: { Image(systemName: "network") }
                    .tag("connection")
                    .accessibilityIdentifier("settings.root.connection")
                Label { Text("Collection", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
                    .tag("collection")
                    .accessibilityIdentifier("settings.root.collection")
            }
            .sidebarColumnChrome()
        } detail: {
            Form {
                if selectedSection == "collection" {
                    SettingsCollectionFilesSection(collection: collection, present: present)
                    ManualTransferSections(model: collection.manualTransfers, area: .transfers)
                } else {
                    ManualTransferSections(model: collection.manualTransfers, area: .connection)
                    Section {
                        DisclosureGroup {
                            ConnectionMetadata(model: collection.manualTransfers)
                            ManualTransferSections(model: collection.manualTransfers, area: .history)
                        } label: { Text("Connection details", bundle: .module) }
                        .accessibilityIdentifier("settings.anki.details")
                    }
                }
            }
            .formStyle(.grouped)
            .frame(maxWidth: ContentWidth.readable)
            .frame(maxWidth: .infinity)
            .detailColumnChrome()
        }
        .toolbar(removing: .sidebarToggle)
        .manualTransferPresentation(model: collection.manualTransfers)
        .accessibilityIdentifier("settings.root.content")
    }
}
#endif

private struct ConnectionMetadata: View {
    let model: ManualTransferCoordinator
    var body: some View {
        LabeledContent {
            Text(verbatim: sanitizedTransferEndpoint(model.endpoint))
                .accessibilityIdentifier("settings.anki.configuredEndpointValue")
        } label: {
            Text("Configured endpoint", bundle: .module)
                .accessibilityIdentifier("settings.anki.configuredEndpointLabel")
        }
        .accessibilityIdentifier("settings.anki.configuredEndpoint")
        LabeledContent("Current observed stage", value: model.stage.description)
        LabeledContent("Account", value: model.authenticated ? model.username : "No saved login")
        Text("Remote freshness is unknown. A prior transfer or saved login does not establish the server’s current state.", bundle: .module)
            .foregroundStyle(.token(.textSecondary))
        if let progress = model.progress {
            LabeledContent("Current progress", value: progress.total.map { "\(progress.completed) / \($0)" } ?? "\(progress.completed)")
        }
        ForEach(model.connectionEvents) { event in
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(event.instant, format: .dateTime.year().month().day().hour().minute().second())
                    .font(.caption)
                Text(verbatim: event.detail)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        LabeledContent("Historical attempt time", value: "Not recorded")
        LabeledContent("Historical completion time", value: "Not recorded")
        LabeledContent("Historical stage timeline", value: "Not recorded")
    }
}

/// Injected by the shell into Review; the Review feature never imports Settings.
public struct ConnectionStatusView: View {
    private let model: ManualTransferCoordinator
    @State private var presented = false
    public init(model: ManualTransferCoordinator) { self.model = model }
    public var body: some View {
        Button { presented.toggle() } label: {
            Image(systemName: model.busy ? "arrow.triangle.2.circlepath" : "network")
                .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .accessibilityLabel(Text("Connection status", bundle: .module))
        .accessibilityIdentifier("review.session.connectionStatus")
        .popover(isPresented: $presented) {
            ScrollView {
            VStack(alignment: .leading, spacing: Spacing.small) {
                Text("Connection status", bundle: .module).font(.headline)
                Text(verbatim: model.stage.description)
                if let message = model.message { Text(verbatim: message) }
                if let record = model.records.first {
                    Text("Last recorded preparation", bundle: .module).font(.subheadline)
                    Text(Date(timeIntervalSince1970: Double(record.record.preparedMilliseconds) / 1_000), format: .dateTime)
                    Text(record.record.collectionComplete ? "Collection stage completed; inspect Settings for media and overall outcome." : "Recorded operation is incomplete.", bundle: .module)
                } else {
                    Text("No recorded server transfer.", bundle: .module)
                }
                Text("Remote freshness is unknown. Review Connection settings for details.", bundle: .module)
                    .foregroundStyle(.token(.textSecondary))
            }
            .padding(Spacing.medium)
            .frame(idealWidth: ContentWidth.primaryAction)
            }
        }
        .task { await model.loadOffline() }
    }
}
