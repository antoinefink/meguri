import AppModel
import CoreBridge
import DesignSystem
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
            .onChange(of: flow.confirmation) { _, added in
                if let added {
                    AccessibilityNotification.Announcement(String(localized: "Added to \(Self.deckName(added.deck))", bundle: .module)).post()
                }
            }
            .onChange(of: flow.error) { _, message in
                if let message { AccessibilityNotification.Announcement(message).post() }
            }
    }

    private var presentation: some View {
        NoteFormChrome(title: Text("Add Note", bundle: .module), subtitle: confirmationText,
                       subtitleIdentifier: "editor.add.result", identifier: "editor.add",
                       host: flow.runtime as? FieldEditorHost, decisions: AddNoteDecisions(flow: flow),
                       attemptClose: close) {
            closeButton
        } confirm: {
            addButton
        } content: {
            editor
        }
    }

    private var editor: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let draft = flow.draft {
                AddNoteSelectionForm(flow: flow, draft: draft)
                if let host = flow.runtime as? FieldEditorHost {
                    FieldEditorView(host: host)
                        // The page, not the draft: after an add the same web view
                        // stays mounted and first responder (N17.1).
                        .id(ObjectIdentifier(host.webView))
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ContentUnavailableView {
                        Label { Text("Editor Unavailable", bundle: .module) } icon: { Image(systemName: "exclamationmark.triangle") }
                    }
                }
            }
            messages
        }
    }

    /// Lines that explain a refusal or a failure, under the fields. A blank or
    /// ordinary draft shows none.
    @ViewBuilder private var messages: some View {
        let host = flow.runtime as? FieldEditorHost
        let draft = flow.draft
        VStack(alignment: .leading) {
            if let error = host?.error, flow.error == nil { failure(error) }
            if let error = draft?.fieldCheckError, flow.error == nil { failure(error) }
            if flow.opening || flow.saving || flow.guardingClose {
                ProgressView().accessibilityIdentifier("editor.add.busy")
            }
            if let state = flow.refusal ?? draft?.fieldCheck?.state {
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
        }
        .padding(.horizontal)
    }

    /// "Added to <deck>", until the next note is started.
    private var confirmationText: String? {
        guard let added = flow.confirmation, flow.draft?.unchangedSinceLastAdd == true else { return nil }
        return String(localized: "Added to \(Self.deckName(added.deck))", bundle: .module)
    }
    /// A deck is named by its last component, as on its own page.
    static func deckName(_ fullName: String) -> String {
        fullName.components(separatedBy: "::").last.flatMap { $0.isEmpty ? nil : $0 } ?? fullName
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
        // N17.1: Add waits for the first field, so the live empty state says
        // nothing. Only a refused add names it (editor/add-flow A5 step 4).
        case .empty: flow.refusal == .empty ? "Enter content in the first field." : nil
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
            .disabled(!flow.canAdd || (flow.runtime as? FieldEditorHost)?.editable != true)
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

/// The frame Add Note and Edit Note share (N17.1, N20.8): the title and its
/// subtitle, the cancellation and confirmation actions and, on the Mac, the
/// bottom bar with the one mode control. Touch: a navigation bar in a sheet
/// that holds the swipe down for the form's own close guard.
struct NoteFormChrome<Cancel: View, Confirm: View, Content: View, Decisions: ViewModifier>: View {
    let title: Text
    let subtitle: String?
    let subtitleIdentifier: String
    let identifier: String
    let host: FieldEditorHost?
    let decisions: Decisions
    let attemptClose: () -> Void
    /// Mac: Edit Note is a window-modal sheet (N20.8, option A): the title
    /// over its subtitle, and Cancel beside Done at the bottom trailing
    /// corner, as every Mac sheet places them. Add keeps its panel's header.
    var sheet = false
    @ViewBuilder let cancel: () -> Cancel
    @ViewBuilder let confirm: () -> Confirm
    @ViewBuilder let content: () -> Content

    var body: some View {
        #if os(macOS)
        Group {
            if sheet { sheetBody } else { panelBody }
        }
        .modifier(decisions)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(identifier)
        #else
        NavigationStack {
            content()
                // N17.1: over the whole screen the form keeps a readable column.
                .frame(maxWidth: coversScreen ? ContentWidth.readable : .infinity)
                .frame(maxWidth: .infinity)
                .background(Color(uiColor: .systemGroupedBackground).ignoresSafeArea())
                .navigationTitle(title)
                .navigationBarTitleDisplayMode(.inline)
                // The subtitle covers no field and moves nothing while the keyboard is up.
                .navigationSubtitle(Text(verbatim: subtitle ?? ""))
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { cancel() }
                    ToolbarItem(placement: .confirmationAction) { confirm() }
                }
        }
        .presentationDetents([.large])
        .presentationSizing(.page)
        // Always hold the native gesture; an attempted swipe asks the same
        // asynchronous held-flush guard as Close. It never authorizes discard.
        .interactiveDismissDisabled()
        .background(AddNoteSwipeGuard(attempt: attemptClose))
        .modifier(decisions)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(identifier)
        #endif
    }

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    private var coversScreen: Bool {
        AddNotePresentation.coversScreen(phone: UIDevice.current.userInterfaceIdiom == .phone, regularWidth: sizeClass == .regular)
    }
    #endif
    #if os(macOS)
    private var panelBody: some View {
        VStack(spacing: 0) {
            HStack {
                title.font(.headline)
                if let subtitle { subtitleText(subtitle) }
                Spacer()
                cancel()
            }
            .padding()
            Divider()
            content()
            Divider()
            HStack { modeToggle; Spacer(); confirm() }.padding()
        }
        // The page's own background, so the fields and their form read as one surface.
        .background(Color(nsColor: .textBackgroundColor))
    }
    /// The sheet keeps the window's own background; the page draws the
    /// fields' grouped box on it (`FieldEditor/page.html`).
    private var sheetBody: some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading) {
                title.font(.headline)
                if let subtitle { subtitleText(subtitle).font(.subheadline) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding()
            content()
            Divider()
            HStack { modeToggle; Spacer(); cancel(); confirm() }.padding()
        }
        .frame(minWidth: 520, idealWidth: 640, maxWidth: 900, minHeight: 420, idealHeight: 640, maxHeight: .infinity)
    }
    private func subtitleText(_ subtitle: String) -> some View {
        Text(verbatim: subtitle).foregroundStyle(.secondary)
            .accessibilityIdentifier(subtitleIdentifier)
    }
    /// The one mode control, for the field that has focus (⇧⌘X does the same).
    @ViewBuilder private var modeToggle: some View {
        if let host {
            Toggle(isOn: Binding(get: { host.focusedMode == .html }, set: { _ in host.toggleFocusedMode() })) {
                Label { Text("HTML Editor", bundle: .module) } icon: { Image(systemName: "chevron.left.forwardslash.chevron.right") }
            }
            .toggleStyle(.button)
            .labelStyle(.iconOnly)
            .disabled(host.focusedMode == nil)
            .help(Text("HTML Editor", bundle: .module))
            .accessibilityIdentifier(host.modeControlIdentifier)
        }
    }
    #endif
}

/// The form's first group: Deck, then Type, each a menu on its current value.
/// The fields under it scroll in their own view, so this group keeps the
/// height of its two rows and never scrolls.
private struct AddNoteSelectionForm: View {
    let flow: AddNoteFlow
    let draft: NoteEditorDraft
    @State private var top: CGFloat = 0
    @State private var deckBottom: CGFloat = 0
    @State private var typeTop: CGFloat = 0
    @State private var typeBottom: CGFloat?
    /// The last row's lower edge: its content plus half the gap between the
    /// two rows' contents, which is one row's own inset.
    private var bottom: CGFloat? { typeBottom.map { $0 + max(0, typeTop - deckBottom) / 2 } }

    private var decks: [DeckNameEntry] {
        flow.deckChoices.contains { $0.id == draft.deck.id } ? flow.deckChoices : [draft.deck] + flow.deckChoices
    }
    private var notetypes: [NotetypeName] {
        flow.notetypeChoices.contains { $0.id == draft.notetype.id } ? flow.notetypeChoices
            : [NotetypeName(id: draft.notetype.id, name: draft.notetype.name)] + flow.notetypeChoices
    }
    private var busy: Bool { flow.opening || flow.saving || flow.guardingClose || flow.exclusiveOperation != nil }

    var body: some View {
        Form {
            Section {
                Picker(selection: Binding(get: { draft.deck.id }, set: { id in Task { await flow.selectDeck(id) } })) {
                    ForEach(decks, id: \.id) { Text(verbatim: $0.fullName).tag($0.id) }
                } label: { Text("Deck", bundle: .module) }
                .accessibilityIdentifier("editor.add.deck")
                .onGeometryChange(for: CGFloat.self) { $0.frame(in: .global).maxY } action: { deckBottom = $0 }
                Picker(selection: Binding(get: { draft.notetype.id }, set: { id in Task { await flow.selectNotetype(id) } })) {
                    ForEach(notetypes, id: \.id) { Text(verbatim: $0.name).tag($0.id) }
                } label: { Text("Type", bundle: .module) }
                .accessibilityIdentifier("editor.add.type")
                .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { typeTop = $0.minY; typeBottom = $0.maxY }
            }
            .disabled(busy)
        }
        .formStyle(.grouped)
        .scrollDisabled(true)
        .scrollContentBackground(.hidden)
        .onGeometryChange(for: CGFloat.self) { $0.frame(in: .global).minY } action: { top = $0 }
        // Until the rows report their place the form takes its share of the sheet.
        .frame(height: bottom.map { max(0, $0 - top) })
    }
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
