#if os(macOS)
import AppModel
import SwiftUI

public struct SentenceMiningPane: View {
    @Bindable private var model: SentenceMining
    public init(model: SentenceMining) { self.model = model }
    public var body: some View {
        Form {
            Section {
                Toggle("Migaku (44432)", isOn: Binding(get: { model.migaku.enabled },
                    set: { model.setEnabled($0, for: .migaku) }))
                    .toggleStyle(.checkbox)
                    .accessibilityIdentifier("settings.integrations.migaku.enabled")
                LabeledContent("Migaku status") {
                    Text(verbatim: model.migaku.status).textSelection(.enabled)
                        .accessibilityIdentifier("settings.integrations.migaku.status")
                }
                Toggle("AnkiConnect (8765)", isOn: Binding(get: { model.ankiConnect.enabled },
                    set: { model.setEnabled($0, for: .ankiConnect) }))
                    .toggleStyle(.checkbox)
                    .accessibilityIdentifier("settings.integrations.ankiConnect.enabled")
                LabeledContent("AnkiConnect status") {
                    Text(verbatim: model.ankiConnect.status).textSelection(.enabled)
                        .accessibilityIdentifier("settings.integrations.ankiConnect.status")
                }
            } header: { Text("Sentence Mining") }
            footer: { Text("Choose Anki in Migaku. Keep Meguri running to save sentences.") }
            Section {
                Picker("Deck", selection: Binding(get: { model.selectedDeck }, set: model.selectDeck)) {
                    Text("Choose a deck").tag(Int64(0))
                    ForEach(model.decks, id: \.id) { Text(verbatim: $0.fullName).tag($0.id) }
                }
                .accessibilityIdentifier("settings.integrations.deck")
                Picker("Note type", selection: Binding(get: { model.selectedType }, set: model.selectType)) {
                    Text("Choose a note type").tag(Int64(0))
                    ForEach(model.types, id: \.id) { Text(verbatim: $0.name).tag($0.id) }
                }
                .accessibilityIdentifier("settings.integrations.noteType")
                if let error = model.error {
                    Text(verbatim: error).foregroundStyle(.secondary)
                        .accessibilityIdentifier("settings.integrations.error")
                }
            } header: { Text("Destination") }
            footer: { Text("Front receives the sentence and Back receives the translation. Audio and images use matching fields, or are appended to the sentence when a matching field is absent. Embedded media only; remote media URLs return an error.") }
            if !model.fieldMapping.isEmpty {
                Section {
                    DisclosureGroup("Field mapping") {
                        ForEach(model.fieldMapping, id: \.self) { Text(verbatim: $0) }
                    }
                    .accessibilityIdentifier("settings.integrations.mapping")
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle("Integrations")
        .accessibilityIdentifier("settings.integrations.content")
    }
}
#endif
