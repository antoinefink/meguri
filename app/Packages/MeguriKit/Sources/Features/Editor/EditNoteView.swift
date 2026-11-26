import AppModel
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// Edit Note during a review (N20.8, `review/edit-current` E7): Add Note's
/// form on the current card's note, titled Edit Note, with Cancel and Done
/// and "<deck> · <note type>" as its subtitle. Neither can be changed here.
public struct EditNoteView: View {
    public let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    private var flow: EditNoteFlow { collection.editNoteFlow }
    private var host: FieldEditorHost? { flow.runtime as? FieldEditorHost }

    public var body: some View {
        NoteFormChrome(title: Text("Edit Note", bundle: .module), subtitle: subtitle,
                       subtitleIdentifier: "review.editCurrent.subtitle", identifier: "review.editCurrent",
                       host: host, decisions: EditNoteDecisions(flow: flow), attemptClose: cancel, window: true) {
            cancelButton
        } confirm: {
            doneButton
        } content: {
            editor
        }
        .onChange(of: flow.saveFailure) { _, failure in
            if let failure { AccessibilityNotification.Announcement(Self.message(failure)).post() }
        }
    }

    private var subtitle: String? {
        flow.subtitle.map { String(localized: "\($0.deck) · \($0.notetype)", bundle: .module) }
    }

    private var editor: some View {
        VStack(alignment: .leading, spacing: 0) {
            #if os(macOS)
            facts
            #endif
            if let host {
                FieldEditorView(host: host)
                    .id(ObjectIdentifier(host.webView))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ContentUnavailableView {
                    Label { Text("Editor Unavailable", bundle: .module) } icon: { Image(systemName: "exclamationmark.triangle") }
                }
            }
            VStack(alignment: .leading) {
                if let error = host?.error { failure(Text(verbatim: error)) }
                if flow.saving || flow.guardingClose {
                    ProgressView().accessibilityIdentifier("review.editCurrent.busy")
                }
                if let failure = flow.saveFailure { self.failure(Text(Self.message(failure))) }
                if let error = flow.error { failure(Text(verbatim: error)) }
            }
            .padding(.horizontal)
        }
    }

    #if os(macOS)
    /// F1 (2026-11-25): the note type and deck as plain text in the form's
    /// label column, above the fields; neither can be changed here.
    @ViewBuilder private var facts: some View {
        if let subtitle = flow.subtitle {
            NoteFormSelectionRows {
                Text(verbatim: subtitle.notetype).foregroundStyle(.secondary)
            } deck: {
                Text(verbatim: subtitle.deck).foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("review.editCurrent.subtitle")
        }
    }
    #endif

    private func failure(_ text: Text) -> some View {
        Label { text } icon: { Image(systemName: "exclamationmark.triangle") }
            .accessibilityIdentifier("review.editCurrent.error")
            .accessibilityAddTraits(.updatesFrequently)
    }
    /// `editor/edit-flows` W9's lines for a failed save.
    static func message(_ failure: EditNoteFlow.SaveFailure) -> String {
        switch failure {
        case .checkDatabase: String(localized: "This note can’t be saved until you run Check Database.", bundle: .module)
        case .notSaved: String(localized: "Your changes couldn’t be saved.", bundle: .module)
        }
    }

    private var cancelButton: some View {
        Button(role: .cancel, action: cancel) { Text("Cancel", bundle: .module) }
            .keyboardShortcut(.cancelAction)
            .disabled(flow.opening)
            .accessibilityIdentifier("review.editCurrent.cancel")
    }
    private var doneButton: some View {
        Button(action: done) {
            #if os(macOS)
            Text("Save", bundle: .module)
            #else
            Text("Done", bundle: .module)
            #endif
        }
            .buttonStyle(.borderedProminent)
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(!flow.canSave || host?.editable != true)
            .accessibilityIdentifier("review.editCurrent.done")
    }
    private func done() {
        #if os(macOS)
        if let event = NSApp.currentEvent, event.type == .keyDown, event.isARepeat { return }
        #endif
        Task { await flow.save() }
    }
    private func cancel() { Task { await flow.requestCancel() } }
}

private struct EditNoteDecisions: ViewModifier {
    let flow: EditNoteFlow
    func body(content: Content) -> some View {
        let discard = flow.discardConfirmation
        return content
            .confirmationDialog(Text("Discard changes?", bundle: .module), isPresented: Binding(
                get: { discard != nil },
                set: { if !$0, let discard { flow.answerDiscard(discard, discard: false) } }
            ), titleVisibility: .visible, presenting: discard) { token in
                Button(role: .destructive) { flow.answerDiscard(token, discard: true) } label: { Text("Discard", bundle: .module) }
                Button(role: .cancel) { flow.answerDiscard(token, discard: false) } label: { Text("Keep Editing", bundle: .module) }
            }
    }
}

// The collection retains the actual host above layout changes, as for Add.
extension ReviewCollection {
    /// Every Edit Note entry calls this: the pencil now, Card › Edit Note,
    /// the context menu and E with them (N4).
    @discardableResult public func openEditNote() async -> Bool {
        await editNoteFlow.open { [media] draft, current in
            let host = try FieldEditorHost(draft: draft, isCurrent: current)
            // Option A (2026-11-24): the fields' thumbnails and sound chips, as in Browse.
            host.show(media: media)
            return host
        }
    }
}
