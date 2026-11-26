import AppModel
import CoreBridge
import SwiftUI

/// F1/F6: the name and searches lead; preview delays appear only when needed.
public struct FilteredStudyForm: View {
    @Bindable private var model: FilteredStudyModel
    public init(model: FilteredStudyModel) { self.model = model }
    public var body: some View {
        StudyFormFrame(title: model.editingDeck == nil ? "Create Filtered Deck" : "Edit Filtered Deck",
                       model: model, id: "home.filteredStudy") {
            Section {
                TextField(text: $model.definition.name) { Text("Name", bundle: .module) }
                    .accessibilityIdentifier("home.filteredStudy.name")
            }
            if !model.definition.terms.isEmpty {
                Section {
                    FilteredTermFields(term: $model.definition.terms[0], limit: $model.firstLimitText, id: "first")
                } header: { Text("First Search", bundle: .module) }
            }
            Section {
                Toggle(isOn: $model.usesSecondTerm) { Text("Use a second search", bundle: .module) }
                    .accessibilityIdentifier("home.filteredStudy.secondEnabled")
                if model.usesSecondTerm { FilteredTermFields(term: $model.secondTerm, limit: $model.secondLimitText, id: "second") }
            } header: { Text("Second Search", bundle: .module) }
            Section {
                Toggle(isOn: $model.definition.reschedule) { Text("Reschedule cards", bundle: .module) }
                    .accessibilityIdentifier("home.filteredStudy.reschedule")
                if !model.definition.reschedule {
                    TextField(text: $model.againText) { Text("Again (seconds)", bundle: .module) }
                        .accessibilityIdentifier("home.filteredStudy.again")
                    TextField(text: $model.hardText) { Text("Hard (seconds)", bundle: .module) }
                        .accessibilityIdentifier("home.filteredStudy.hard")
                    TextField(text: $model.goodText) { Text("Good (seconds)", bundle: .module) }
                        .accessibilityIdentifier("home.filteredStudy.good")
                }
            } footer: {
                if !model.definition.reschedule {
                    Text("Preview preserves each card’s home schedule. A zero delay finishes the card; Easy always finishes.", bundle: .module)
                }
            }
        }
    }
}

private struct FilteredTermFields: View {
    @Binding var term: FilteredStudyTerm
    @Binding var limit: String
    let id: String
    private let orders: [LocalizedStringKey] = [
        "Oldest review first", "Random", "Shortest interval first", "Longest interval first", "Most lapses first",
        "Oldest note first", "Due first", "Newest note first", "Lowest retrievability first",
        "Highest retrievability first", "Relative overdueness"
    ]
    var body: some View {
        TextField(text: $term.search, axis: .vertical) { Text("Search", bundle: .module) }
            .accessibilityIdentifier("home.filteredStudy.\(id).search")
        TextField(text: $limit) { Text("Card limit", bundle: .module) }
            .accessibilityIdentifier("home.filteredStudy.\(id).limit")
        Picker(selection: $term.order) {
            ForEach(orders.indices, id: \.self) { index in Text(orders[index], bundle: .module).tag(UInt32(index)) }
            if term.order > 10 { Text("Unknown order", bundle: .module).tag(term.order) }
        } label: { Text("Order", bundle: .module) }
        .accessibilityIdentifier("home.filteredStudy.\(id).order")
    }
}

/// F5: only the chosen preset's inputs are shown; Core supplies availability and suggestions.
public struct CustomStudyForm: View {
    @Bindable private var model: FilteredStudyModel
    private let modes: [LocalizedStringKey] = ["Adjust today’s new limit", "Adjust today’s review limit", "Forgotten cards",
        "Review ahead", "Preview recent new cards", "Study by state or tags"]
    private let states: [LocalizedStringKey] = ["New cards", "Due cards", "Previously introduced cards", "All cards (preview)"]
    public init(model: FilteredStudyModel) { self.model = model }
    public var body: some View {
        StudyFormFrame(title: "Custom Study", model: model, id: "home.customStudy") {
            Section {
                LabeledContent { Text(verbatim: model.sourceName) } label: { Text("Deck", bundle: .module) }
                    .accessibilityIdentifier("home.customStudy.deck")
                Picker(selection: $model.modeIndex) {
                    ForEach(modes.indices, id: \.self) { index in Text(modes[index], bundle: .module).tag(index) }
                } label: { Text("Study", bundle: .module) }
                .accessibilityIdentifier("home.customStudy.mode")
                if let defaults = model.defaults, model.modeIndex < 2 {
                    LabeledContent {
                        Text(model.modeIndex == 0 ? defaults.availableNew : defaults.availableReview, format: .number)
                    } label: { Text("Available cards before daily limits", bundle: .module) }
                    .accessibilityIdentifier("home.customStudy.available")
                }
                TextField(text: $model.amountText) {
                    Text(model.modeIndex < 2 ? "Change in today’s limit" : model.modeIndex == 5 ? "Card limit" : "Days", bundle: .module)
                }
                .accessibilityIdentifier("home.customStudy.amount")
                if model.modeIndex == 2 { Text("Choose 1–30 days.", bundle: .module) }
            } footer: {
                if model.modeIndex < 2 { Text("Positive changes allow more cards today. Negative changes reduce today’s allowance.", bundle: .module) }
                if model.modeIndex == 2 || model.modeIndex == 4 || (model.modeIndex == 5 && model.stateIndex == 3) {
                    Text("Preview preserves each card’s home schedule.", bundle: .module)
                }
            }
            if model.modeIndex == 5 {
                Section {
                    Picker(selection: $model.stateIndex) {
                        ForEach(states.indices, id: \.self) { index in Text(states[index], bundle: .module).tag(index) }
                    } label: { Text("Card state", bundle: .module) }
                    .accessibilityIdentifier("home.customStudy.state")
                    TextField(text: $model.includeTags, axis: .vertical) { Text("Include tags", bundle: .module) }
                        .accessibilityIdentifier("home.customStudy.includeTags")
                    TextField(text: $model.excludeTags, axis: .vertical) { Text("Exclude tags", bundle: .module) }
                        .accessibilityIdentifier("home.customStudy.excludeTags")
                } footer: {
                    Text("Enter one complete tag per line. Match any included tag and exclude every excluded tag. Leave inclusions blank to include all tags.", bundle: .module)
                }
            }
        }
    }
}

private struct StudyFormFrame<Content: View>: View {
    let title: LocalizedStringKey
    @Bindable var model: FilteredStudyModel
    let id: String
    @ViewBuilder let content: () -> Content
    var body: some View {
        #if os(macOS)
        VStack(alignment: .leading) {
            Text(title, bundle: .module).font(.headline)
            form
            HStack {
                cancel
                Spacer()
                confirm
            }
        }
        .padding()
        .presentationSizing(.form)
        .interactiveDismissDisabled(model.busy)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(id + ".sheet")
        #else
        NavigationStack {
            form.navigationTitle(Text(title, bundle: .module))
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { cancel }
                    ToolbarItem(placement: .confirmationAction) { confirm }
                }
        }
        .interactiveDismissDisabled(model.busy)
        .accessibilityIdentifier(id + ".sheet")
        #endif
    }
    private var form: some View {
        Form {
            Group { content() }.disabled(!model.loaded || model.busy || model.needsRefresh)
            if model.busy { ProgressView().accessibilityIdentifier(id + ".progress") }
            if let failure = model.failure {
                Section {
                    Text(message(failure), bundle: .module)
                        .accessibilityIdentifier(id + ".message")
                    if model.canSaveEmpty {
                        Button { model.confirmEmpty() } label: { Text("Save Empty Deck", bundle: .module) }
                            .accessibilityIdentifier(id + ".saveEmpty")
                    }
                    if !model.loaded {
                        Button { model.load() } label: { Text("Try Again", bundle: .module) }
                            .disabled(model.busy)
                            .accessibilityIdentifier(id + ".retry")
                    }
                }
            }
        }
        .formStyle(.grouped)
    }
    private var cancel: some View {
        Button { model.cancel() } label: { Text("Cancel", bundle: .module) }
            .keyboardShortcut(.cancelAction)
            .disabled(model.busy)
            .accessibilityIdentifier(id + ".cancel")
    }
    private var confirm: some View {
        Button {
            if model.needsRefresh { model.refresh() } else { model.submit() }
        } label: {
            Text(model.needsRefresh ? "Refresh Decks" : model.sourceDeck != nil ? "Apply" : "Save and Build", bundle: .module)
        }
        .keyboardShortcut(.defaultAction)
        .disabled(!model.canSubmit && !model.needsRefresh)
        .accessibilityIdentifier(id + ".save")
    }
    private func message(_ failure: FilteredStudyModel.Failure) -> LocalizedStringKey {
        switch failure {
        case .load: "Couldn’t read study settings. Try again."
        case .write: "Couldn’t apply study settings. Check the name, search and amounts. No eligible cards may match. A normal deck with the session’s name must be renamed first."
        case .noMatches:
            model.sourceDeck == nil
                ? "No eligible cards matched. Edit the search or choose Save Empty Deck. The previous deck is unchanged."
                : "No eligible cards matched. Edit the study choices and try again. The previous session is unchanged."
        case .refresh: "The change was saved. Refresh decks to continue."
        }
    }
}

/// Shared by Home and Browse; an operation only publishes feedback to its current route.
public struct FilteredStudyFeedback: ViewModifier {
    @Bindable var model: FilteredStudyModel
    public init(model: FilteredStudyModel) { self.model = model }
    public func body(content: Content) -> some View {
        content.alert(Text("Couldn’t Update Study Deck", bundle: .module), isPresented: $model.operationFailed) {
            Button(role: .cancel) {} label: { Text("OK", bundle: .module) }
        } message: { Text("Try again, or edit the search and card limits.", bundle: .module) }
    }
}

struct FilteredDeckActions: View {
    let collection: ReviewCollection
    let deck: Int64
    var body: some View {
        Button { collection.filteredStudy.edit(deck) } label: {
            Label { Text("Edit Filtered Deck…", bundle: .module) } icon: { Image(systemName: "pencil") }
        }
        .disabled(!collection.filteredStudy.canEdit(deck))
        .accessibilityIdentifier("home.filteredDeck.edit.\(deck)")
        Button { collection.filteredStudy.rebuild(deck) } label: {
            Label { Text("Rebuild", bundle: .module) } icon: { Image(systemName: "arrow.clockwise") }
        }
        .disabled(!collection.filteredStudy.canEdit(deck))
        .accessibilityIdentifier("home.filteredDeck.rebuild.\(deck)")
        Button { collection.filteredStudy.empty(deck) } label: {
            Label { Text("Empty", bundle: .module) } icon: { Image(systemName: "tray") }
        }
        .disabled(!collection.filteredStudy.canEdit(deck))
        .accessibilityIdentifier("home.filteredDeck.empty.\(deck)")
    }
}

/// The app menu shares this feature's translated catalog with both package forms.
public enum StudyCommandText {
    public static var create: String { String(localized: "Create Filtered Deck…", bundle: .module) }
    public static var edit: String { String(localized: "Edit Filtered Deck…", bundle: .module) }
    public static var custom: String { String(localized: "Custom Study…", bundle: .module) }
    public static var rebuild: String { String(localized: "Rebuild", bundle: .module) }
    public static var empty: String { String(localized: "Empty", bundle: .module) }
}
