import AppModel
import CoreBridge
import SwiftUI

/// Values observed by the Mac Settings detail path. Keep provider-specific
/// destinations distinct so a push retains the provider chosen at the click.
enum SyncSettingsDestination: Hashable {
    case troubleshooting, account, issues, meguriAffectedFiles, syncLog

    @MainActor @ViewBuilder func view(collection: ReviewCollection, host: AppNavigation.SheetHost) -> some View {
        switch self {
        case .troubleshooting:
            SyncTroubleshootingPage(collection: collection, host: host)
        case .account:
            Form {
                LabeledContent(SyncText.value("sync.meguri.address"), value: collection.sync.meguriEndpoint)
                    .accessibilityIdentifier("settings.sync.meguri.account.address")
                LabeledContent(SyncText.value("sync.meguri.username"), value: collection.sync.meguriUsername ?? "")
                    .accessibilityIdentifier("settings.sync.meguri.account.username")
                SyncServiceRows(collection: collection, host: .main, accountSignOut: true)
            }
            .formStyle(.grouped)
            .navigationTitle(SyncText.value("sync.settings.account"))
            .accessibilityIdentifier("settings.sync.meguri.account.content")
        case .issues:
            SyncIssuesPage(collection: collection)
        case .meguriAffectedFiles:
            MeguriAffectedFilesPage(collection: collection)
        case .syncLog:
            if let callbacks = collection.configuration.fieldDiagnostics {
                Form {
                    SyncDiagnosticsContent(snapshot: { try await callbacks.snapshot() },
                        safeExport: { try await callbacks.readableExport(sync: collection.sync.debugFacts) }, writeClipboard: callbacks.writeClipboard)
                }
                .formStyle(.grouped)
                .navigationTitle(SyncText.value("sync.trouble.log"))
            }
        }
    }
}

/// Only the Mac Settings host uses the explicit path. Other Sync screens keep
/// their native view destination links and the same destination content.
struct SyncSettingsLink<Label: View>: View {
    let destination: SyncSettingsDestination
    let collection: ReviewCollection
    let host: AppNavigation.SheetHost
    private let label: Label

    init(_ destination: SyncSettingsDestination, collection: ReviewCollection,
         host: AppNavigation.SheetHost, @ViewBuilder label: () -> Label) {
        self.destination = destination; self.collection = collection; self.host = host
        self.label = label()
    }

    var body: some View {
        #if os(macOS)
        if host == .settings {
            NavigationLink(value: destination) { label }
        } else {
            directLink
        }
        #else
        directLink
        #endif
    }

    private var directLink: some View {
        NavigationLink {
            destination.view(collection: collection, host: host)
        } label: { label }
    }
}
