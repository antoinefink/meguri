import AppModel
import SwiftUI

/// The draft and admitted job belong to ReviewCollection, never this view.
public struct DeckNameSheet: View {
    @Bindable private var model: DeckActionsModel
    @FocusState private var nameFocused: Bool
    @State private var selection: TextSelection?

    public init(model: DeckActionsModel) { self.model = model }

    public var body: some View {
        #if os(macOS)
        // A Mac sheet has no navigation bar: the form leads, buttons along the bottom.
        content.frame(width: 420)
        #else
        NavigationStack { content }
        #endif
    }

    private var content: some View {
        Group {
            Form {
                #if os(macOS)
                Section { nameRows } header: { title.font(.headline).foregroundStyle(.primary) }
                #else
                nameRows
                #endif
            }
            #if os(macOS)
            .formStyle(.grouped)
            .scrollDisabled(true)
            .fixedSize(horizontal: false, vertical: true)
            #endif
            #if !os(macOS)
            .navigationTitle(title)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button { model.cancel() } label: {
                        if model.finalName != nil { Text("Close", bundle: .module) }
                        else { Text("Cancel", bundle: .module) }
                    }
                    .keyboardShortcut(.cancelAction)
                    .disabled(model.cancelling && model.busy)
                    .accessibilityIdentifier("home.deckName.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    if model.finalName != nil {
                        Button { model.cancel() } label: { Text("Done", bundle: .module) }
                            .keyboardShortcut(.defaultAction)
                            .disabled(model.busy)
                            .accessibilityIdentifier("home.deckName.done")
                    } else {
                        Button { model.submit() } label: { Text(confirmation, bundle: .module) }
                            .keyboardShortcut(.defaultAction)
                            .disabled(!model.canSubmit)
                            .accessibilityIdentifier("home.deckName.confirm")
                    }
                }
            }
        }
        .interactiveDismissDisabled(model.busy)
        #if os(macOS)
        // Without a navigation stack, the sheet's identifier needs its own container.
        .accessibilityElement(children: .contain)
        #endif
        .accessibilityIdentifier("home.deckName.sheet")
        .task {
            nameFocused = true
            await Task.yield()
            // Supported native selection keeps the subdeck prefix intact while typing.
            selection = TextSelection(insertionPoint: model.name.endIndex)
        }
    }

    @ViewBuilder private var nameRows: some View {
                if let finalName = model.finalName {
                    LabeledContent { Text(verbatim: finalName) } label: {
                        Text("Saved name", bundle: .module)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel(Text("Saved name", bundle: .module))
                    .accessibilityValue(Text(verbatim: finalName))
                    .accessibilityIdentifier("home.deckName.savedName")
                } else {
                    TextField(text: $model.name, selection: $selection) {
                        Text("Deck name", bundle: .module)
                    }
                    .focused($nameFocused)
                    .disabled(model.busy || model.needsRefresh)
                    .accessibilityIdentifier("home.deckName.name")
                    .onSubmit { model.submit() }
                }
                if let message = model.message {
                    Text(verbatim: message)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("home.deckName.message")
                }
                if model.busy {
                    ProgressView {
                        if model.needsOpen {
                            Text("Opening saved deck…", bundle: .module)
                        } else {
                            Text(model.cancelling ? "Cancelling…" : "Saving deck name…", bundle: .module)
                        }
                    }
                    .accessibilityIdentifier("home.deckName.progress")
                }
                if model.needsRefresh {
                    Button { model.resumeResult() } label: {
                        Text(model.needsOpen ? "Open Deck" : "Refresh Decks", bundle: .module)
                    }
                    .accessibilityIdentifier("home.deckName.refresh")
                }
    }

    private var title: Text {
        switch model.action {
        case .rename: Text("Rename Deck", bundle: .module)
        case .subdeck: Text("New Subdeck", bundle: .module)
        default: Text("Create Deck", bundle: .module)
        }
    }
    private var confirmation: LocalizedStringKey {
        if case .rename = model.action { "Rename" } else { "Create" }
    }
}
