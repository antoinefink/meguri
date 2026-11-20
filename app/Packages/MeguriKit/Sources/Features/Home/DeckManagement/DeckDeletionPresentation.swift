import AppModel
import SwiftUI

/// N20.7's question and failure alert, in the system's forms: an alert on the
/// Mac's window; on touch a confirmation dialog, which the system shows from
/// the bottom at compact width and as a popover beside its view at regular
/// width (where it drops Cancel: a tap elsewhere cancels).
public struct DeckDeletionFeedback: ViewModifier {
    let collection: ReviewCollection?
    public init(collection: ReviewCollection?) { self.collection = collection }

    public func body(content: Content) -> some View {
        let model = collection?.deckDeletion
        content
            #if os(macOS)
            .alert(Text(verbatim: model?.confirmation.map(DeckDeletionText.title) ?? ""),
                   isPresented: DeckDeletionBinding.question(model) { _ in true }) {
                // Delete is never the default: Return cancels, as Esc does. The
                // system gives the cancel role Esc, and Return when the other
                // button is destructive.
                Button(role: .cancel) { model?.cancel() } label: { Text("Cancel", bundle: .module) }
                Button(role: .destructive) { model?.confirm() } label: { Text("Delete", bundle: .module) }
            } message: {
                Text(verbatim: model?.confirmation.map(DeckDeletionText.message) ?? "")
            }
            #else
            // The Deck menu's command, with no row to stand beside.
            .deckDeletionDialog(model) { $0 == .window }
            #endif
            .alert(Text(verbatim: model?.failure.map(DeckDeletionText.failureTitle) ?? ""), isPresented: Binding(
                get: { model?.failure != nil },
                set: { if !$0 { model?.dismissFailure() } }
            )) {
                Button { model?.dismissFailure() } label: { Text("OK", bundle: .module) }
                    .keyboardShortcut(.defaultAction)
            } message: {
                Text(verbatim: model?.failure.map(DeckDeletionText.failureMessage) ?? "")
            }
    }
}

extension View {
    /// The question beside the row whose deck menu asked it (touch only).
    @ViewBuilder func deckDeletionQuestion(collection: ReviewCollection, deckID: Int64, place: String) -> some View {
        #if os(macOS)
        self
        #else
        deckDeletionDialog(collection.deckDeletion) { anchor in
            anchor == .row(place: place) && collection.deckDeletion.confirmation?.deckID == deckID
        }
        #endif
    }

    #if !os(macOS)
    fileprivate func deckDeletionDialog(_ model: DeckDeletionModel?,
                                        shows: @escaping (DeckDeletionModel.Anchor) -> Bool) -> some View {
        confirmationDialog(Text(verbatim: model?.confirmation.map(DeckDeletionText.title) ?? ""),
                           isPresented: DeckDeletionBinding.question(model, shows: shows),
                           titleVisibility: .visible) {
            Button(role: .destructive) { model?.confirm() } label: { Text("Delete Deck", bundle: .module) }
            Button(role: .cancel) { model?.cancel() } label: { Text("Cancel", bundle: .module) }
        } message: {
            Text(verbatim: model?.confirmation.map(DeckDeletionText.message) ?? "")
        }
    }
    #endif
}

private enum DeckDeletionBinding {
    /// Shown while a question waits for this place; dismissing it cancels.
    static func question(_ model: DeckDeletionModel?, shows: @escaping (DeckDeletionModel.Anchor) -> Bool) -> Binding<Bool> {
        Binding(
            get: { model?.confirmation.map { shows($0.anchor) } ?? false },
            set: { if !$0, let model, let question = model.confirmation, shows(question.anchor) { model.cancel() } }
        )
    }
}
