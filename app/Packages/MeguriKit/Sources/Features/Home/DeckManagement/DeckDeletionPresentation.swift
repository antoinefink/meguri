import AppModel
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// N20.7's question and failure alert, in the system's forms: an alert on the
/// Mac's window; on touch the system's confirmation dialog, which iOS 27 shows
/// as a popover from its view (a tap elsewhere cancels).
public struct DeckDeletionFeedback: ViewModifier {
    let collection: ReviewCollection?
    public init(collection: ReviewCollection?) { self.collection = collection }
    @Environment(\.locale) private var locale
    #if os(macOS)
    /// app/AGENTS.md: no alert while the app is hidden (it would quit). A
    /// question or failure that arrives then waits in the model until it shows.
    @State private var appVisible = !NSApplication.shared.isHidden
    #else
    private let appVisible = true
    #endif

    public func body(content: Content) -> some View {
        let model = collection?.deckDeletion, locale = locale, visible = appVisible
        content
            #if os(macOS)
            .alert(Text(verbatim: model?.confirmation.map { DeckDeletionText.title(name: $0.name, locale: locale) } ?? ""),
                   isPresented: DeckDeletionBinding.question(model, visible: visible) { _ in true }) {
                // Delete is never the default: no button takes Return, so
                // Return deletes nothing; the cancel role takes Esc.
                Button(role: .cancel) { model?.cancel() } label: { Text("Cancel", bundle: .module) }
                Button(role: .destructive) { model?.confirm() } label: { Text("Delete", bundle: .module) }
            } message: {
                Text(verbatim: model?.confirmation.map { DeckDeletionText.message($0, locale: locale) } ?? "")
            }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didHideNotification)) { _ in appVisible = false }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didUnhideNotification)) { _ in appVisible = true }
            #else
            // The Deck menu's command, with no row to stand beside.
            .modifier(DeckDeletionDialog(model: model) { $0 == .window })
            #endif
            .alert(Text(verbatim: model?.failure.map { DeckDeletionText.failureTitle(name: $0.name, locale: locale) } ?? ""),
                   isPresented: Binding(
                get: { visible && model?.failure != nil },
                // Only a dismissal of a shown alert acknowledges the failure.
                set: { if !$0, visible { model?.dismissFailure() } }
            )) {
                Button { model?.dismissFailure() } label: { Text("OK", bundle: .module) }
                    .keyboardShortcut(.defaultAction)
            } message: {
                Text(verbatim: model?.failure.map { DeckDeletionText.failureMessage($0.reason, locale: locale) } ?? "")
            }
    }
}

extension View {
    /// The question beside the row whose deck menu asked it (touch only).
    @ViewBuilder func deckDeletionQuestion(collection: ReviewCollection, deckID: Int64, place: String) -> some View {
        #if os(macOS)
        self
        #else
        modifier(DeckDeletionDialog(model: collection.deckDeletion) { anchor in
            anchor == .row(place: place) && collection.deckDeletion.confirmation?.deckID == deckID
        })
        #endif
    }
}

#if !os(macOS)
/// The touch confirmation, in the window's locale.
private struct DeckDeletionDialog: ViewModifier {
    let model: DeckDeletionModel?
    let shows: (DeckDeletionModel.Anchor) -> Bool
    @Environment(\.locale) private var locale
    func body(content: Content) -> some View {
        let locale = locale
        content.confirmationDialog(
            Text(verbatim: model?.confirmation.map { DeckDeletionText.title(name: $0.name, locale: locale) } ?? ""),
            isPresented: DeckDeletionBinding.question(model, visible: true, shows: shows),
            titleVisibility: .visible) {
            Button(role: .destructive) { model?.confirm() } label: { Text("Delete Deck", bundle: .module) }
            Button(role: .cancel) { model?.cancel() } label: { Text("Cancel", bundle: .module) }
        } message: {
            Text(verbatim: model?.confirmation.map { DeckDeletionText.message($0, locale: locale) } ?? "")
        }
    }
}
#endif

private enum DeckDeletionBinding {
    /// Shown while a question waits for this place and the app is visible;
    /// dismissing a shown question cancels it.
    static func question(_ model: DeckDeletionModel?, visible: Bool,
                         shows: @escaping (DeckDeletionModel.Anchor) -> Bool) -> Binding<Bool> {
        Binding(
            get: { visible && (model?.confirmation.map { shows($0.anchor) } ?? false) },
            set: { if !$0, visible, let model, let question = model.confirmation, shows(question.anchor) { model.cancel() } }
        )
    }
}
