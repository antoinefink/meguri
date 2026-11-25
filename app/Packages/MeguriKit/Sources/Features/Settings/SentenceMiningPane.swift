#if os(macOS)
import AppModel
import SwiftUI

public struct SentenceMiningPane: View {
    @Bindable private var model: SentenceMining
    public init(model: SentenceMining) { self.model = model }
    public var body: some View {
        Form {
            Section {
                receiverRow(.migaku, name: "Migaku", label: "Migaku (44432)", key: "migaku")
                receiverRow(.ankiConnect, name: "AnkiConnect", label: "AnkiConnect (8765)", key: "ankiConnect")
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

    private func receiverRow(_ id: SentenceMining.EndpointID, name: String, label: String, key: String) -> some View {
        let endpoint = model.endpoint(id)
        let status = receiverStatus(endpoint.statusState)
        return HStack {
            Toggle(label, isOn: Binding(get: { endpoint.enabled }, set: { model.setEnabled($0, for: id) }))
                .toggleStyle(.checkbox)
                .accessibilityIdentifier("settings.integrations.\(key).enabled")
            Spacer()
            HStack {
                Image(systemName: "circle.fill")
                    .font(.caption2)
                    .foregroundStyle(status.color)
                    .accessibilityHidden(true)
                Text(verbatim: status.text)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .help(endpoint.status)
                    .accessibilityHint(Text(verbatim: endpoint.status))
                    .accessibilityIdentifier("settings.integrations.\(key).status")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text(verbatim: name))
        .accessibilityIdentifier("settings.integrations.\(key).row")
    }

    private func receiverStatus(_ state: SentenceMining.Endpoint.StatusState) -> (text: String, color: Color) {
        switch state {
        case .off: ("Off", .secondary)
        case .waiting: ("Waiting", .secondary)
        case .starting: ("Starting…", .secondary)
        case .listening: ("Listening", .green)
        case .portInUse: ("Port in use", .orange)
        }
    }
}
#endif
