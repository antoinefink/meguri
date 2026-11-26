import CoreBridge
import SwiftUI

/// The deck's task stays visible; only choices this export can honor are offered.
struct DeckExportOptionsForm: View {
    let name: String
    @Binding var options: DeckExportOptions
    var canExport = true
    var message: String?
    let export: () -> Void
    let cancel: () -> Void

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    LabeledContent { Text(verbatim: name).textSelection(.enabled) } label: { Text("Deck", bundle: .module) }
                        .accessibilityIdentifier("home.deckExport.deck")
                    LabeledContent { Text("Deck Package (.apkg)", bundle: .module) } label: { Text("Format", bundle: .module) }
                } footer: {
                    Text("Includes this deck and its subdecks. All cards of each selected note are included.", bundle: .module)
                }
                Section {
                    Toggle(isOn: $options.includeScheduling) { Text("Include scheduling information", bundle: .module) }
                        .accessibilityIdentifier("home.deckExport.scheduling")
                    Toggle(isOn: $options.includePresets) { Text("Include deck presets", bundle: .module) }
                        .accessibilityIdentifier("home.deckExport.presets")
                    Toggle(isOn: $options.includeMedia) { Text("Include media", bundle: .module) }
                        .accessibilityIdentifier("home.deckExport.media")
                } footer: {
                    Text("Without scheduling information, cards start fresh when imported. Media includes files used by the selected notes and their note types.", bundle: .module)
                }
                if let message {
                    Section { Text(LocalizedStringKey(message), bundle: .module).accessibilityIdentifier("home.deckExport.message") }
                }
            }
            .formStyle(.grouped)
            .accessibilityIdentifier("home.deckExport.content")
            .navigationTitle(Text("Export Deck", bundle: .module))
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(action: cancel) { Text("Cancel", bundle: .module) }
                        .keyboardShortcut(.cancelAction)
                        .accessibilityIdentifier("home.deckExport.close")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(action: export) { Text("Export", bundle: .module) }
                        .keyboardShortcut(.defaultAction)
                        .disabled(!canExport)
                        .accessibilityIdentifier("home.deckExport.export")
                }
            }
        }
        .presentationSizing(.form)
        .interactiveDismissDisabled()
    }
}

/// Production form over explicit sample choices; functional evidence uses a real collection.
public struct DeckExportOptionsPreview: View {
    @State private var options: DeckExportOptions
    public init(changed: Bool = false) {
        _options = State(initialValue: changed
            ? DeckExportOptions(includeScheduling: true, includePresets: true, includeMedia: false)
            : DeckExportOptions())
    }
    public var body: some View {
        DeckExportOptionsForm(name: "Languages::Spanish", options: $options, export: {}, cancel: {})
    }
}
