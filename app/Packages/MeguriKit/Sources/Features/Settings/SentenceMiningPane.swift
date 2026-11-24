#if os(macOS)
import AppModel
import SwiftUI

public struct SentenceMiningPane: View {
    @Bindable private var model: SentenceMining
    @State private var portEntry: String
    @State private var invalidPort = false
    public init(model: SentenceMining) {
        self.model = model
        _portEntry = State(initialValue: String(model.configuredPort))
    }
    public var body: some View {
        Form {
            Section {
                Toggle("Receive mined sentences", isOn: Binding(get: { model.enabled }, set: model.setEnabled))
                    .accessibilityIdentifier("settings.integrations.enabled")
                LabeledContent("Status") { Text(verbatim: model.status).textSelection(.enabled) }
                    .accessibilityIdentifier("settings.integrations.status")
                LabeledContent("Local port") {
                    HStack {
                        TextField("Port", text: $portEntry)
                            .labelsHidden()
                            .accessibilityLabel("Local port")
                            .accessibilityIdentifier("settings.integrations.port")
                            .onSubmit(commitPort)
                        Button("Apply", action: commitPort)
                            .disabled(portEntry == String(model.configuredPort))
                            .accessibilityIdentifier("settings.integrations.applyPort")
                    }
                }
                if invalidPort {
                    Text("Enter a port from 1024 through 65535. The configured port has not changed.")
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("settings.integrations.portError")
                }
            } header: { Text("Sentence Mining") }
            footer: { Text("Choose Anki in Migaku. Current Migaku expects port 44432. A client that supports another port must use the same value as Meguri. Keep Meguri running to save sentences to this collection.") }
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
        .onChange(of: portEntry) { invalidPort = false }
        .onChange(of: model.configuredPort) { portEntry = String(model.configuredPort) }
    }

    private func commitPort() {
        invalidPort = !model.applyPort(portEntry)
        if !invalidPort { portEntry = String(model.configuredPort) }
    }
}
#endif
