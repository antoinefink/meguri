import AppModel
import CoreBridge
import SwiftUI

/// Only delivered destinations, in R47 order. Both row affordances use this builder.
struct DeckMenu: View {
    let collection: ReviewCollection
    let deck: CountedDeckNode

    var body: some View {
        if !deck.filtered {
            Button { Task { await collection.startStudy(deckID: deck.id) } } label: {
                Text("Study", bundle: .module)
            }
            .disabled(!collection.canStudy || collection.navigation.sheet != nil || collection.addNotePresentation.opening)
            .accessibilityIdentifier("home.deckMenu.study.\(deck.id)")
            Button { collection.navigation.present(.add(deck: DeckID(deck.id))) } label: {
                Text("Add Note", bundle: .module)
            }
            .disabled(!collection.addNotePresentation.canPresent(presetDeck: deck.id))
            .accessibilityIdentifier("home.deckMenu.addNote.\(deck.id)")
            Button { collection.openDeckPage(deckID: deck.id) } label: { Text("Overview", bundle: .module) }
                .disabled(collection.review != nil || collection.navigation.sheet != nil)
                .accessibilityIdentifier("home.deckMenu.overview.\(deck.id)")
        }
        Button { collection.deckActions.present(.rename(deck.id)) } label: { Text("Rename…", bundle: .module) }
            .keyboardShortcut("r", modifiers: [.command, .shift])
            .disabled(!collection.deckActions.canPresent(.rename(deck.id)))
            .accessibilityIdentifier("home.deckMenu.rename.\(deck.id)")
        if !deck.filtered {
            Button { collection.deckActions.present(.subdeck(deck.id)) } label: { Text("New Subdeck…", bundle: .module) }
                .disabled(!collection.deckActions.canPresent(.subdeck(deck.id)))
                .accessibilityIdentifier("home.deckMenu.subdeck.\(deck.id)")
        }
    }
}

private struct DeckMenuAffordances: ViewModifier {
    let collection: ReviewCollection?
    let deck: CountedDeckNode
    #if os(macOS)
    @State private var hovering = false
    @FocusState private var menuFocused: Bool
    #endif

    func body(content: Content) -> some View {
        if let collection {
            #if os(macOS)
            HStack {
                content
                Menu { DeckMenu(collection: collection, deck: deck) } label: {
                    Label { Text("Deck Actions", bundle: .module) } icon: { Image(systemName: "ellipsis") }
                        .labelStyle(.iconOnly)
                }
                .focused($menuFocused)
                .accessibilityIdentifier("home.deckMenu.open.\(deck.id)")
                .menuStyle(.button)
                .menuIndicator(.hidden)
                .buttonStyle(.borderless)
                .fixedSize()
                .opacity(hovering || menuFocused ? 1 : 0)
                .help(Text("Deck Actions", bundle: .module))
            }
            .contextMenu { DeckMenu(collection: collection, deck: deck) }
            .onHover { hovering = $0 }
            #else
            content.contextMenu { DeckMenu(collection: collection, deck: deck) }
            #endif
        } else { content }
    }
}

extension View {
    func deckMenu(collection: ReviewCollection?, deck: CountedDeckNode) -> some View {
        modifier(DeckMenuAffordances(collection: collection, deck: deck))
    }
}
