import AppModel
import SwiftUI

/// New Deck, New Subdeck and Rename (N20): a Name and a Parent Deck, never a
/// typed "::" path. The draft and the admitted job belong to ReviewCollection,
/// never this view. A sheet, except beside the deck list on a wide iPhone,
/// where it takes the deck page's place (`embedded`).
public struct DeckNameForm: View {
    @Bindable private var model: DeckActionsModel
    private let embedded: Bool
    @FocusState private var nameFocused: Bool
    @State private var selection: TextSelection?

    public init(model: DeckActionsModel, embedded: Bool = false) {
        self.model = model; self.embedded = embedded
    }

    public var body: some View {
        #if os(macOS)
        // A Mac sheet has no navigation bar: the title leads, the buttons sit along the bottom.
        VStack(alignment: .leading) {
            title.font(.headline)
            form
        }
        .padding()
        .presentationSizing(.form.fitted(horizontal: false, vertical: true))
        .modifier(DeckNameFormChrome(model: model))
        #else
        if embedded {
            form.modifier(DeckNameFormChrome(model: model))
        } else {
            NavigationStack { form }.modifier(DeckNameFormChrome(model: model))
        }
        #endif
    }

    private var form: some View {
        Form {
            Section {
                nameRow
                parentRow
            } footer: {
                if let footer = model.footer, model.finalName == nil {
                    footerText(footer)
                        .accessibilityIdentifier("home.deckName.footer")
                }
            }
            statusRows
        }
        #if os(macOS)
        // Labels lead their controls, as Mac forms have them.
        .formStyle(.columns)
        #endif
        #if !os(macOS)
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
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
        .task {
            // D19.3: focus in Name, the cursor at the end.
            nameFocused = true
            await Task.yield()
            selection = TextSelection(insertionPoint: model.name.endIndex)
        }
    }

    @ViewBuilder private var nameRow: some View {
        if let finalName = model.finalName {
            // After a save whose page could not open yet: what was saved, as people read it.
            LabeledContent {
                Text(verbatim: finalName.components(separatedBy: "::").joined(separator: " › "))
            } label: {
                Text("Saved As", bundle: .module)
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("home.deckName.savedName")
        } else {
            #if os(macOS)
            nameField
            #else
            LabeledContent { nameField } label: { Text("Name", bundle: .module) }
            #endif
        }
    }

    private var nameField: some View {
        TextField(text: $model.name, selection: $selection) { Text("Name", bundle: .module) }
            .focused($nameFocused)
            .disabled(model.busy || model.needsRefresh)
            .accessibilityIdentifier("home.deckName.name")
            .onSubmit { model.submit() }
    }

    @ViewBuilder private var parentRow: some View {
        if model.finalName == nil {
            Picker(selection: $model.parentID) {
                Text("None", bundle: .module).tag(Int64?.none)
                if !model.parentChoices.isEmpty { Divider() }
                ForEach(model.parentChoices) { choice in
                    // Indented by depth; read as the whole path.
                    Text(verbatim: String(repeating: "\u{2003}", count: choice.depth) + choice.name)
                        .accessibilityLabel(Text(verbatim: choice.spokenPath))
                        .tag(Optional(choice.id))
                }
            } label: {
                Text("Parent Deck", bundle: .module)
            } currentValueLabel: {
                if let parent = model.parent { Text(verbatim: parent.name) } else { Text("None", bundle: .module) }
            }
            .pickerStyle(.menu)
            .disabled(model.busy || model.needsRefresh)
            .accessibilityValue(model.parent.map { Text(verbatim: $0.spokenPath) } ?? Text("None", bundle: .module))
            .accessibilityIdentifier("home.deckName.parent")
        }
    }

    @ViewBuilder private var statusRows: some View {
        if model.message != nil || model.busy || model.needsRefresh {
            Section {
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
        }
    }

    private func footerText(_ footer: DeckNaming.Footer) -> Text {
        switch footer {
        case .subdecksMove(let count): Text("Its \(count) subdecks move with it.", bundle: .module)
        case .inside(let path): Text("Inside \(path).", bundle: .module)
        }
    }

    private var title: Text {
        switch model.action {
        case .rename: Text("Rename Deck", bundle: .module)
        case .subdeck: Text("New Subdeck", bundle: .module)
        default: Text("New Deck", bundle: .module)
        }
    }
    private var confirmation: LocalizedStringKey {
        if case .rename = model.action { "Rename" } else { "Create" }
    }
}

/// What the form keeps wherever it shows: no dismissal while saving, and one
/// identifier for the whole form.
private struct DeckNameFormChrome: ViewModifier {
    let model: DeckActionsModel
    func body(content: Content) -> some View {
        content
            .interactiveDismissDisabled(model.busy)
            #if os(macOS)
            // Without a navigation stack, the sheet's identifier needs its own container.
            .accessibilityElement(children: .contain)
            #endif
            .accessibilityIdentifier("home.deckName.sheet")
    }
}
