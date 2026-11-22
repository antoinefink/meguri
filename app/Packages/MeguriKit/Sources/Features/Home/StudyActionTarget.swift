import AppModel
import SwiftUI

/// Tells the collection which deck this screen's primary Study starts, while
/// it shows: that deck's first card is prepared beneath it, so Study opens
/// on it (R1813, N16.8).
struct StudyActionTarget: ViewModifier {
    let collection: ReviewCollection
    let deckID: Int64?
    @State private var owner = UUID()

    func body(content: Content) -> some View {
        content
            .onAppear { collection.showStudyAction(deckID: deckID, owner: owner) }
            .onChange(of: deckID) { _, deck in collection.showStudyAction(deckID: deck, owner: owner) }
            .onDisappear { collection.hideStudyAction(owner: owner) }
    }
}
