import AppModel
import CoreBridge
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The production editor reads the collection's retained host. WebKit remains
/// the field input, scrolling and focus owner; there is no outer ScrollView.
public struct AddNoteView: View {
    public let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    private var flow: AddNoteFlow { collection.addNoteFlow }

    public var body: some View {
        presentation
            .onChange(of: flow.isPresented) { _, presented in
                if !presented { collection.navigation.dismissSheet() }
            }
            .onChange(of: flow.refusal) { _, state in
                if let state, let key = statusKey(state) {
                    AccessibilityNotification.Announcement(String(localized: String.LocalizationValue(key), bundle: .module)).post()
                }
            }
            .onChange(of: flow.lastAdded?.id) { _, id in
                if id != nil, let added = flow.lastAdded {
                    AccessibilityNotification.Announcement(String(localized: "\(added.cards.count) cards added", bundle: .module)).post()
                }
            }
            .onChange(of: flow.error) { _, message in
                if let message { AccessibilityNotification.Announcement(message).post() }
            }
    }

    private var presentation: some View {
        #if os(macOS)
        VStack {
            HStack {
                Text("Add Note", bundle: .module).font(.headline)
                Spacer()
                closeButton
            }
            .padding()
            Divider()
            editor
            Divider()
            HStack { Spacer(); addButton }.padding()
        }
        .modifier(AddNoteDecisions(flow: flow))
        .accessibilityIdentifier("editor.add")
        #else
        NavigationStack {
            editor
                .navigationTitle(Text("Add Note", bundle: .module))
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { closeButton }
                    ToolbarItem(placement: .confirmationAction) { addButton }
                }
        }
        .presentationDetents([.large])
        .presentationSizing(.page)
        // Always hold the native gesture; an attempted swipe asks the same
        // asynchronous held-flush guard as Close. It never authorizes discard.
        .interactiveDismissDisabled()
        .background(AddNoteSwipeGuard(attempt: close))
        .modifier(AddNoteDecisions(flow: flow))
        .accessibilityIdentifier("editor.add")
        #endif
    }

    private var editor: some View {
        VStack(alignment: .leading) {
            if let draft = flow.draft {
                Grid(alignment: .leading) {
                    GridRow {
                        Text("Type", bundle: .module)
                        Text(verbatim: draft.notetype.name)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("editor.add.type")
                    GridRow {
                        Text("Deck", bundle: .module)
                        Text(verbatim: draft.deck.fullName)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("editor.add.deck")
                }
                .padding(.horizontal)
                if let host = flow.runtime as? FieldEditorHost {
                    FieldEditorView(host: host)
                        .id(host.draft.identity)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    if let error = host.error, flow.error == nil {
                        failure(error)
                    }
                } else {
                    ContentUnavailableView {
                        Label { Text("Editor Unavailable", bundle: .module) } icon: { Image(systemName: "exclamationmark.triangle") }
                    }
                }
                if let error = draft.fieldCheckError, flow.error == nil { failure(error) }
            }
            if flow.opening || flow.saving || flow.guardingClose {
                ProgressView().accessibilityIdentifier("editor.add.busy")
            }
            if let state = flow.refusal ?? flow.draft?.fieldCheck?.state {
                fieldStatus(state)
            }
            if let operation = flow.exclusiveOperation {
                Text("Waiting for \(operation)", bundle: .module)
                    .accessibilityIdentifier("editor.add.waiting")
            }
            if let error = flow.error { failure(error) }
            if let issue = collection.addNotePresentation.issue, !repeatsFlowError(issue) {
                AddNoteRequestMessage(issue: issue)
                    .accessibilityIdentifier("editor.add.requestError")
            }
            if let added = flow.lastAdded {
                Text("\(added.cards.count) cards added", bundle: .module)
                    .accessibilityIdentifier("editor.add.result")
                    .accessibilityAddTraits(.updatesFrequently)
            }
        }
    }

    @ViewBuilder private func fieldStatus(_ state: FieldState) -> some View {
        if let key = statusKey(state) {
            Text(LocalizedStringKey(key), bundle: .module)
                .accessibilityIdentifier("editor.add.fieldStatus")
                .accessibilityAddTraits(.updatesFrequently)
        }
    }
    private func statusKey(_ state: FieldState) -> String? {
        switch state {
        case .normal: nil
        case .empty:
            flow.refusal == .empty || flow.draft?.isBlank == false ? "Enter content in the first field." : nil
        case .duplicate: "A note with this first field already exists."
        case .notetypeNotCloze: "This note type does not support cloze deletions."
        case .fieldNotCloze: "This field does not support cloze deletions."
        case .missingCloze: "This note has no cloze deletion."
        }
    }
    private func failure(_ message: String) -> some View {
        Label { Text(verbatim: message) } icon: { Image(systemName: "exclamationmark.triangle") }
            .accessibilityIdentifier("editor.add.error")
            .accessibilityAddTraits(.updatesFrequently)
    }
    private func repeatsFlowError(_ issue: AddNotePresentation.Issue) -> Bool {
        if case .failed(let message) = issue { return message == flow.error }
        return false
    }
    private var addButton: some View {
        Button(action: add) { Text("Add", bundle: .module) }
            .buttonStyle(.borderedProminent)
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(!flow.canSave || (flow.runtime as? FieldEditorHost)?.editable != true)
            .accessibilityIdentifier("editor.add.save")
    }
    private var closeButton: some View {
        Button(role: .close, action: close) {
            Label { Text("Close", bundle: .module) } icon: { Image(systemName: "xmark") }
        }
        .labelStyle(.iconOnly)
        .keyboardShortcut(.cancelAction)
        .disabled(flow.opening || collection.addNotePresentation.opening)
        .accessibilityIdentifier("editor.add.close")
    }
    private func add() {
        #if os(macOS)
        if let event = NSApp.currentEvent, event.type == .keyDown, event.isARepeat { return }
        #endif
        Task { await flow.save() }
    }
    private func close() { collection.navigation.dismissSheet() }
}

private struct AddNoteDecisions: ViewModifier {
    let flow: AddNoteFlow
    func body(content: Content) -> some View {
        let cloze = flow.missingClozeConfirmation
        let discard = flow.discardConfirmation
        return content
            .confirmationDialog(Text("Add without a cloze deletion?", bundle: .module), isPresented: Binding(
                get: { cloze != nil },
                set: { if !$0, let cloze { flow.answerMissingCloze(cloze, add: false) } }
            ), titleVisibility: .visible, presenting: cloze) { token in
                Button { flow.answerMissingCloze(token, add: true) } label: { Text("Add", bundle: .module) }
                Button(role: .cancel) { flow.answerMissingCloze(token, add: false) } label: { Text("Cancel", bundle: .module) }
            }
            .confirmationDialog(Text("Discard current input?", bundle: .module), isPresented: Binding(
                get: { discard != nil },
                set: { if !$0, let discard { flow.answerDiscard(discard, discard: false) } }
            ), titleVisibility: .visible, presenting: discard) { token in
                Button(role: .destructive) { flow.answerDiscard(token, discard: true) } label: { Text("Discard", bundle: .module) }
                Button(role: .cancel) { flow.answerDiscard(token, discard: false) } label: { Text("Keep Editing", bundle: .module) }
            }
    }
}

struct AddNoteRequestMessage: View {
    let issue: AddNotePresentation.Issue
    var body: some View {
        switch issue {
        case .unavailable: Text("Add Note is unavailable. Check the collection and try again.", bundle: .module)
        case .otherPresentation: Text("Close the current presentation before opening another.", bundle: .module)
        case .reviewDeferred: Text("Finish the review before adding a note.", bundle: .module)
        case .failed(let message): Text(verbatim: message)
        }
    }
}

/// Reports a refused request after an existing modal releases presentation.
/// It never dismisses that modal in order to display its own error.
public struct AddNoteRequestFeedback: ViewModifier {
    let collection: ReviewCollection?
    public init(collection: ReviewCollection?) { self.collection = collection }
    public func body(content: Content) -> some View {
        content.alert(Text("Couldn’t Open Add Note", bundle: .module), isPresented: Binding(
            get: { collection?.addNotePresentation.issue != nil && collection?.navigation.sheet == nil
                && collection?.reviewIsActive == true },
            set: { if !$0 { collection?.addNotePresentation.clearIssue() } }
        )) {
            Button { collection?.addNotePresentation.clearIssue() } label: { Text("OK", bundle: .module) }
        } message: {
            if let issue = collection?.addNotePresentation.issue { AddNoteRequestMessage(issue: issue) }
        }
    }
}

extension ReviewCollection {
    public func configureAddNotePresentation() {
        addNotePresentation.configure { [weak self] deck in
            guard let self else { return false }
            return await prepareAddNoteEditor(presetDeck: deck)?.editable == true
        }
    }
}
