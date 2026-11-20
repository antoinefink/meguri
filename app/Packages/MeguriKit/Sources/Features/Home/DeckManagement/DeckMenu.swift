import AppModel
import CoreBridge
import SwiftUI

/// The deck's long-press and right-click menu (N20): Study and Add Note, then
/// Rename… and New Subdeck…. A filtered deck takes neither notes nor
/// subdecks and is not studied yet, so it offers Rename… only.
struct DeckMenu: View {
    let collection: ReviewCollection
    let deck: CountedDeckNode

    var body: some View {
        if !deck.filtered {
            Section {
                Button { Task { await collection.startStudy(deckID: deck.id) } } label: {
                    Label { Text("Study", bundle: .module) } icon: { Image(systemName: "play") }
                }
                #if os(macOS)
                // The Deck menu's key, shown beside the item (N4).
                .keyboardShortcut("r", modifiers: .command)
                #endif
                // N14.3: as the deck page, no Study while nothing waits.
                .disabled(!collection.canStudy || !collection.offersStudy(deckID: deck.id) || collection.navigation.sheet != nil || collection.addNotePresentation.opening)
                .accessibilityIdentifier("home.deckMenu.study.\(deck.id)")
                Button { collection.navigation.present(.add(deck: DeckID(deck.id))) } label: {
                    Label { Text("Add Note", bundle: .module) } icon: { Image(systemName: "square.and.pencil") }
                }
                .disabled(!collection.addNotePresentation.canPresent(presetDeck: deck.id))
                .accessibilityIdentifier("home.deckMenu.addNote.\(deck.id)")
            }
        }
        Section {
            Button { collection.deckActions.present(.rename(deck.id)) } label: {
                Label { Text("Rename…", bundle: .module) } icon: { Image(systemName: "pencil") }
            }
            #if os(macOS)
            .keyboardShortcut("r", modifiers: [.command, .shift])
            #endif
            .disabled(!collection.deckActions.canPresent(.rename(deck.id)))
            .accessibilityIdentifier("home.deckMenu.rename.\(deck.id)")
            if !deck.filtered {
                Button { collection.deckActions.present(.subdeck(deck.id)) } label: {
                    Label { Text("New Subdeck…", bundle: .module) } icon: { Image(systemName: "folder.badge.plus") }
                }
                .disabled(!collection.deckActions.canPresent(.subdeck(deck.id)))
                .accessibilityIdentifier("home.deckMenu.subdeck.\(deck.id)")
            }
        }
    }
}

extension View {
    /// The row's deck menu: a long press on touch, a right-click on the Mac.
    @ViewBuilder func deckMenu(collection: ReviewCollection?, deck: CountedDeckNode) -> some View {
        if let collection {
            contextMenu { DeckMenu(collection: collection, deck: deck) }
        } else {
            self
        }
    }
}
