import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Compact/regular mobile Settings uses system navigation into grouped forms.
public struct SettingsView: View {
    private let collection: ReviewCollection
    #if !os(macOS)
    @Environment(\.shellLayout) private var shellLayout
    #endif
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        List {
            ForEach(SettingsPage.visible, id: \.self) { page in
                NavigationLink(value: Destination.settingsPage(page)) {
                    SettingsPageLabel(page: page)
                }
                .accessibilityIdentifier("settings.root.\(page.rawValue)")
            }
        }
        .navigationTitle(Text("Settings", bundle: .module))
        #if !os(macOS)
        // As the Decks tab's root (N14.8).
        .toolbarTitleDisplayMode(shellLayout == .compact ? .inlineLarge : .automatic)
        #endif
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("settings.root.content")
    }
}

struct SettingsPageLabel: View {
    let page: SettingsPage
    var body: some View {
        switch page {
        case .accountSync: Label(SyncText.value("sync.settings.section"), systemImage: "icloud")
        case .collection: Label { Text("Collection", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
        case .ankiServer: Label { Text("Anki Server", bundle: .module) } icon: { Image(systemName: "network") }
        }
    }
}

public struct SettingsPageView: View {
    let collection: ReviewCollection
    let page: SettingsPage
    public init(collection: ReviewCollection, page: SettingsPage) { self.collection = collection; self.page = page }
    public var body: some View {
        if page == .accountSync { AccountSyncView(collection: collection) }
        else {
            Form {
                if page == .collection {
                    SettingsCollectionFilesSection(collection: collection)
                    if SettingsPage.showsAnkiServer {
                        ManualTransferSections(model: collection.manualTransfers, area: .transfers)
                    }
                } else {
                    ManualTransferSections(model: collection.manualTransfers, area: .connection)
                    Section {
                        DisclosureGroup {
                            ConnectionMetadata(model: collection.manualTransfers)
                            ManualTransferSections(model: collection.manualTransfers, area: .history, inlineHistory: true)
                        } label: { Text("Connection details", bundle: .module) }
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle(Text(page == .collection ? "Collection" : "Anki Server", bundle: .module))
            .manualTransferPresentation(model: collection.manualTransfers, isActive: collection.navigation.sheet != .manualTransfer)
        }
    }
}

/// The same collection file actions appear in Settings render fixtures.
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
            CollectionActionRow {
                Button { if let present { present(.collectionEntry) } else { collection?.entry.present() } } label: {
                    Text("Import Collection", bundle: .module)
                        #if !os(macOS)
                        // Borderless draws no bezel: the label carries the touch target.
                        .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                        .contentShape(Rectangle())
                        #endif
                }
                #if os(macOS)
                .buttonStyle(.bordered)
                #else
                .buttonStyle(.borderless)
                #endif
                .controlSize(.regular)
                .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                .contentShape(Rectangle())
                .disabled(!fileActionsAvailable || collection?.entry.canPresent != true)
                .accessibilityIdentifier("settings.root.importCollection")
            } label: {
                Label { Text("From a file", bundle: .module) } icon: { Image(systemName: "square.and.arrow.down") }
            }
            CollectionActionRow {
                Button { if let present { present(.collectionExport) } else { collection?.collectionExport.start() } } label: {
                    Text("Export Collection", bundle: .module)
                        #if !os(macOS)
                        // Borderless draws no bezel: the label carries the touch target.
                        .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                        .contentShape(Rectangle())
                        #endif
                }
                #if os(macOS)
                .buttonStyle(.bordered)
                #else
                .buttonStyle(.borderless)
                #endif
                .controlSize(.regular)
                .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                .contentShape(Rectangle())
                .disabled(!fileActionsAvailable || collection?.collectionExport.canPresent != true)
                .accessibilityIdentifier("settings.root.exportCollection")
            } label: {
                Label { Text("To a file", bundle: .module) } icon: { Image(systemName: "square.and.arrow.up") }
            }
        } header: { Text("Collection Files", bundle: .module) }
        footer: {
            // N17.3: one line under the Export row says what the file holds.
            Text("Export saves all decks, cards, review history and media in one file.", bundle: .module)
                .accessibilityIdentifier("settings.root.exportCollection.detail")
        }
    }

}

/// Compact native Mac rows; other platforms retain their labeled Form layout.
private struct CollectionActionRow<Content: View, RowLabel: View>: View {
    private let content: Content
    private let label: RowLabel
    init(@ViewBuilder content: () -> Content, @ViewBuilder label: () -> RowLabel) {
        self.content = content(); self.label = label()
    }
    var body: some View {
        #if os(macOS)
        HStack(alignment: .center) {
            label
            Spacer()
            content
        }
        #else
        LabeledContent { content } label: { label }
        #endif
    }
}

#if os(macOS)
/// Native Settings shares the collection. Only this device-local section is restored.
public struct MacSettingsView: View {
    private let collection: ReviewCollection
    private let present: (SheetRoute) -> Void
    @AppStorage("settings.selectedSection") private var selectedSection = "accountSync"
    public init(collection: ReviewCollection, present: @escaping (SheetRoute) -> Void) {
        self.collection = collection; self.present = present
    }
    public var body: some View {
        NavigationSplitView(columnVisibility: .constant(.all)) {
            List(selection: Binding<String?>(get: { selectedSection }, set: { if let value = $0, value != selectedSection { selectedSection = value } })) {
                ForEach(SettingsPage.visible.filter(matchesSearch), id: \.self) { page in
                    SettingsPageLabel(page: page)
                        .tag(page.rawValue)
                        .accessibilityIdentifier("settings.root.\(page.rawValue)")
                }
            }
            .searchable(text: Binding(get: { collection.syncPresentation.settingsSearch },
                set: { collection.syncPresentation.settingsSearch = $0 }), placement: .sidebar)
            .background(SettingsSearchIdentifiers())
            .sidebarColumnChrome()
            .toolbar(removing: .sidebarToggle)
            .navigationSplitViewColumnWidth(min: 200, ideal: 215, max: 260)
        } detail: {
            if section == "accountSync" {
                NavigationStack {
                    AccountSyncView(collection: collection, host: .settings)
                }
            } else {
              Form {
                if section == "collection" {
                    SettingsCollectionFilesSection(collection: collection, present: present)
                    if SettingsPage.showsAnkiServer { serverActions }
                } else {
                    ManualTransferSections(model: collection.manualTransfers, area: .connection)
                    if hasActiveTransfer { continueTransfer }
                    Section {
                        DisclosureGroup {
                            VStack(alignment: .leading, spacing: Spacing.small) {
                                ConnectionMetadata(model: collection.manualTransfers)
                                ManualTransferSections(model: collection.manualTransfers, area: .history, inlineHistory: true)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        } label: { Text("Connection details", bundle: .module) }
                        .accessibilityElement(children: .contain)
                        .accessibilityIdentifier("settings.anki.details")
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle(Text(section == "collection" ? "Collection" : "Anki Server", bundle: .module))
            .detailColumnChrome()
            }
        }
        .onAppear {
            if selectedSection == "connection" { selectedSection = "ankiServer" }
            if !SettingsPage.visible.contains(where: { $0.rawValue == selectedSection }) { selectedSection = "accountSync" }
            consumeAccountRequest()
        }
        .onChange(of: collection.syncPresentation.settingsRequest) { _, _ in consumeAccountRequest() }
        .manualTransferPresentation(model: collection.manualTransfers, isActive: collection.navigation.sheet != .manualTransfer)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("settings.root.content")
    }
    /// A saved section that is now hidden shows Account & Sync, from the first render.
    private var section: String {
        SettingsPage.visible.contains { $0.rawValue == selectedSection } ? selectedSection : "accountSync"
    }
    private func consumeAccountRequest() {
        guard collection.syncPresentation.settingsRequest != nil else { return }
        selectedSection = "accountSync"
        collection.syncPresentation.settingsSearch = ""
        collection.syncPresentation.settingsRequest = nil
    }
    private func matchesSearch(_ page: SettingsPage) -> Bool {
        let query = collection.syncPresentation.settingsSearch
        guard !query.isEmpty else { return true }
        let terms: String = switch page {
        case .accountSync: SyncText.value("sync.settings.section") + " iCloud " + SyncText.value("sync.service.meguri")
        case .collection: String(localized: "Collection", bundle: .module) + " Import Export"
        case .ankiServer: String(localized: "Anki Server", bundle: .module) + " Connection"
        }
        return terms.localizedStandardContains(query)
    }
    private var hasActiveTransfer: Bool {
        let model = collection.manualTransfers
        return model.busy || model.preview != nil || model.mediaPreview != nil
    }
    private var hasTransferDetails: Bool {
        let model = collection.manualTransfers
        return hasActiveTransfer || model.message != nil || model.result != nil
    }
    private var canOpenTransfer: Bool {
        collection.navigation.sheet == nil && !collection.entry.busy && !collection.collectionExport.busy
    }
    private var serverActions: some View {
        Section {
            serverAction(.import)
            serverAction(.export)
            if let message = collection.manualTransfers.message {
                Text(verbatim: message)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("settings.anki.result")
                if let category = collection.manualTransfers.failureCategory {
                    ManualTransferFailureCategoryDetails(category: category)
                }
                if let method = collection.manualTransfers.failureRequestMethod {
                    ManualTransferFailureRequestDetails(method: method)
                }
                if let diagnostic = collection.manualTransfers.redirectDiagnostic {
                    ManualTransferRedirectDetails(diagnostic: diagnostic)
                }
            }
            if hasTransferDetails {
                transferButton("Details…", id: "settings.anki.transferDetails")
            }
        } header: { Text("Server Transfers", bundle: .module) }
        footer: { Text("A transfer replaces the destination collection. Review preparation and media decisions before continuing.", bundle: .module) }
    }
    private func serverAction(_ direction: ManualTransferDirection) -> some View {
        CollectionActionRow {
            Button {
                guard canOpenTransfer, !hasActiveTransfer else { return }
                collection.manualTransfers.select(direction)
                present(.manualTransfer)
            } label: {
                Text(direction == .import ? "Import…" : "Export…", bundle: .module)
            }
            .buttonStyle(.bordered)
            .controlSize(.regular)
            .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
            .contentShape(Rectangle())
            .disabled(!canOpenTransfer || hasActiveTransfer)
            .accessibilityIdentifier(direction == .import ? "settings.anki.openImport" : "settings.anki.openExport")
        } label: {
            Label {
                Text(direction == .import ? "From the server" : "To the server", bundle: .module)
            } icon: { Image(systemName: direction == .import ? "arrow.down.circle" : "arrow.up.circle") }
        }
    }
    private var continueTransfer: some View {
        Section { transferButton("Continue…", id: "settings.anki.continueTransfer") }
    }
    private func transferButton(_ title: LocalizedStringKey, id: String) -> some View {
        CollectionActionRow {
            Button { present(.manualTransfer) } label: { Text(title, bundle: .module) }
                .buttonStyle(.bordered)
                .controlSize(.regular)
                .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                .contentShape(Rectangle())
                .disabled(!canOpenTransfer)
                .accessibilityIdentifier(id)
        } label: { Text("Current transfer", bundle: .module) }
    }
}
#endif

/// Opening or closing this workflow has no transfer effect. The shared model
/// retains work and decisions when the sheet is closed.
public struct ManualTransferWorkflowView: View {
    private let model: ManualTransferCoordinator
    private let close: () -> Void
    public init(model: ManualTransferCoordinator, close: @escaping () -> Void) {
        self.model = model; self.close = close
    }
    public var body: some View {
        NavigationStack {
            ScrollViewReader { proxy in
                Form {
                    ManualTransferSections(model: model)
                    Section {} footer: {
                        Text("Closing this sheet keeps active work and pending decisions. Continue from Collection settings.", bundle: .module)
                    }
                }
                #if os(macOS)
                .formStyle(.grouped)
                #endif
                .task(id: model.mediaPreview != nil) {
                    guard model.mediaPreview != nil else { return }
                    await Task.yield()
                    guard !Task.isCancelled, model.mediaPreview != nil else { return }
                    proxy.scrollTo(ManualTransferSections.mediaDecisionID, anchor: .top)
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("settings.anki.transferWorkflow")
            .manualTransferPresentation(model: model)
            .navigationTitle(Text("Server Transfer", bundle: .module))
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button(action: close) { Text("Done", bundle: .module) }
                        .disabled(model.mediaPreview != nil)
                        .accessibilityIdentifier("settings.anki.closeTransfer")
                }
            }
        }
        .interactiveDismissDisabled(model.confirmationPresented || model.copyDestinationPresented || model.mediaPreview != nil)
    }
}

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
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        if let category = model.failureCategory {
            ManualTransferFailureCategoryDetails(category: category)
        }
        if let method = model.failureRequestMethod {
            ManualTransferFailureRequestDetails(method: method)
        }
        if let diagnostic = model.redirectDiagnostic {
            ManualTransferRedirectDetails(diagnostic: diagnostic)
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
