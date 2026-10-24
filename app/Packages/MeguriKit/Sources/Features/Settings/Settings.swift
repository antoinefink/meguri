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
    var body: some View {
        Section {
            Button { collection?.entry.present() } label: {
                Text("Import Collection", bundle: .module).fixedSize(horizontal: false, vertical: true).frame(minWidth: minimumActionSize, minHeight: minimumActionSize).contentShape(Rectangle())
            }
                .disabled(collection?.entry.canPresent != true)
                .accessibilityIdentifier("settings.root.importCollection")
            Button { collection?.collectionExport.present() } label: {
                Text("Export Collection — Without Media", bundle: .module).fixedSize(horizontal: false, vertical: true).frame(minWidth: minimumActionSize, minHeight: minimumActionSize).contentShape(Rectangle())
            }
                .disabled(collection == nil)
                .accessibilityIdentifier("settings.root.exportCollection")
            Text("Save a latest-format .colpkg containing every deck, note, card and saved review. Images, audio and other media are excluded. No account or connection is needed.", bundle: .module)
                .foregroundStyle(.token(.textSecondary))
                .fixedSize(horizontal: false, vertical: true)
        } header: { Text("Collection Files", bundle: .module) }
    }
    private var minimumActionSize: CGFloat {
        #if os(macOS)
        28
        #else
        44
        #endif
    }
}
