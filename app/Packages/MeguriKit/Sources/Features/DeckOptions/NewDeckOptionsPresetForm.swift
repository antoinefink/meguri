import AppModel
import SwiftUI

/// Native name entry; creation changes only the open core draft.
public struct NewDeckOptionsPresetForm: View {
    @Bindable var model: DeckOptionsPageModel
    @FocusState private var nameFocused: Bool

    public init(model: DeckOptionsPageModel) { self.model = model }

    public var body: some View {
        #if os(macOS)
        VStack(alignment: .leading) {
            Text("deckOptions.preset.newTitle", bundle: .module).font(.headline)
            form
        }
        .padding()
        .presentationSizing(.form.fitted(horizontal: false, vertical: true))
        .interactiveDismissDisabled(model.addingPreset)
        #else
        NavigationStack { form }
            .interactiveDismissDisabled(model.addingPreset)
        #endif
    }

    private var form: some View {
        Form {
            Section {
                TextField(text: $model.presetName) { Text("deckOptions.preset.name", bundle: .module) }
                    .focused($nameFocused)
                    .disabled(model.addingPreset)
                    .onSubmit { model.createPreset() }
                    .accessibilityIdentifier("deckOptions.newPreset.name")
            } footer: {
                Text("deckOptions.preset.newFooter", bundle: .module)
            }
            if model.presetCreationFailed {
                Text("deckOptions.preset.createFailed", bundle: .module)
                    .accessibilityIdentifier("deckOptions.newPreset.failure")
            }
            if model.addingPreset { ProgressView() }
        }
        #if os(macOS)
        .formStyle(.columns)
        #else
        .navigationTitle(Text("deckOptions.preset.newTitle", bundle: .module))
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button { model.cancelNewPreset() } label: { Text("deckOptions.cancel", bundle: .module) }
                    .keyboardShortcut(.cancelAction)
                    .disabled(model.addingPreset)
                    .accessibilityIdentifier("deckOptions.newPreset.cancel")
            }
            ToolbarItem(placement: .confirmationAction) {
                Button { model.createPreset() } label: { Text("deckOptions.preset.create", bundle: .module) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!model.canCreatePreset)
                    .accessibilityIdentifier("deckOptions.newPreset.create")
            }
        }
        .task { nameFocused = true }
    }
}
