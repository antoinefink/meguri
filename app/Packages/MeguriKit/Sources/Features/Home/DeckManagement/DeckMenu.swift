import AppModel
import CoreBridge
import SwiftUI

/// The deck's long-press and right-click menu (N20.2): Study and Add Note,
/// then Rename…, New Subdeck… and Deck Options…, then Export Deck… and Delete Deck.
/// A filtered deck offers Rename…, Export Deck… and Delete Deck.
struct DeckMenu: View {
    let collection: ReviewCollection
    let deck: CountedDeckNode
    /// Row menus ask beside their row; toolbar menus use the window presenter (N20.7).
    var deletionAnchor: DeckDeletionModel.Anchor = .row(place: "list")

    var body: some View {
        // N20.2: each item with its system symbol, on the Mac too.
        items.labelStyle(.titleAndIcon)
    }

    @ViewBuilder private var items: some View {
        if !deck.filtered {
            Group {
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
            // A separator, not a Section: a section's header item took the
            // items' identifiers away in the Mac's context menu.
            Divider()
        }
        Group {
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
                // N20.9: the deck's options, normal decks only.
                Button { collection.openDeckOptions(deckID: deck.id) } label: {
                    Label { Text("Deck Options…", bundle: .module) } icon: { Image(systemName: "gearshape") }
                }
                #if os(macOS)
                .keyboardShortcut("o", modifiers: [.command, .option])
                #endif
                .disabled(!collection.deckOptions.canOpen(deckID: deck.id))
                .accessibilityIdentifier("home.deckMenu.options.\(deck.id)")
            }
        }
        Divider()
        Button { collection.collectionExport.presentDeck(deck.id) } label: {
            Label { Text("Export Deck…", bundle: .module) } icon: { Image(systemName: "square.and.arrow.up") }
        }
        .disabled(!collection.collectionExport.canPresentDeck(deck.id))
        .accessibilityIdentifier("home.deckMenu.export.\(deck.id)")
        Divider()
        Button(role: .destructive) {
            collection.deckDeletion.delete(deckID: deck.id, anchor: deletionAnchor)
        } label: {
            Label { Text("Delete Deck", bundle: .module) } icon: { Image(systemName: "trash") }
        }
        #if os(macOS)
        .keyboardShortcut(.delete, modifiers: .command)
        #endif
        .disabled(!collection.deckDeletion.canDelete(deckID: deck.id))
        .accessibilityIdentifier("home.deckMenu.delete.\(deck.id)")
    }
}

extension View {
    /// The row's deck menu: a long press on touch, a right-click on the Mac.
    @ViewBuilder func deckMenu(collection: ReviewCollection?, deck: CountedDeckNode, place: String = "list") -> some View {
        if let collection {
            contextMenu { DeckMenu(collection: collection, deck: deck, deletionAnchor: .row(place: place)) }
                .deckDeletionQuestion(collection: collection, deckID: deck.id, place: place)
        } else {
            self
        }
    }
}
