import AppModel
import DesignSystem
import SwiftUI

/// Only the working local collection actions are mounted in this slice.
public struct SettingsView: View {
    private let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        Form {
            Section {
                Button { collection.entry.present() } label: {
                    Text("Import Collection", bundle: .module).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                }
                    .disabled(!collection.entry.canPresent)
                    .accessibilityIdentifier("settings.root.importCollection")
                Button { collection.collectionExport.present() } label: {
                    Text("Export Collection — Without Media", bundle: .module).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                }
                    .accessibilityIdentifier("settings.root.exportCollection")
                Text("Save a latest-format .colpkg containing every deck, note, card and saved review. Images, audio and other media are excluded. No account or connection is needed.", bundle: .module)
                    .foregroundStyle(.token(.textSecondary))
            } header: { Text("Collection Files", bundle: .module) }
        }
        .navigationTitle(Text("Settings", bundle: .module))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("settings.root.content")
    }
}
