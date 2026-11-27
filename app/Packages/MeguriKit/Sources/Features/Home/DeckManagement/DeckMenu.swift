import AppModel
import CoreBridge
import SwiftUI

/// The deck's long-press and right-click menu (N20.2): study and inspection,
/// Custom Study, deck management, then Export Deck… and Delete Deck.
/// A filtered deck keeps its study actions and Export Deck…, takes neither
/// notes nor subdecks; its settings edit the saved search definition.
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
            if !deck.filtered {
                Button { collection.navigation.present(.add(deck: DeckID(deck.id))) } label: {
                    Label { Text("Add Note", bundle: .module) } icon: { Image(systemName: "square.and.pencil") }
                }
                .disabled(!collection.addNotePresentation.canPresent(presetDeck: deck.id))
                .accessibilityIdentifier("home.deckMenu.addNote.\(deck.id)")
            }
            if collection.deckImportBrowseAvailable {
                Button { collection.filteredStudy.browseCards(deck.id) } label: {
                    Label { Text("Browse Cards", bundle: .module) } icon: { Image(systemName: "magnifyingglass") }
                }
                .disabled(!collection.filteredStudy.canPresent)
                .accessibilityIdentifier("home.deckMenu.browse.\(deck.id)")
            }
            if let show = collection.showDeckStatistics {
                Button { show(deck.id) } label: {
                    Label { Text("Deck Statistics", bundle: .module) } icon: { Image(systemName: "chart.bar.xaxis") }
                }
                .disabled(!collection.filteredStudy.canPresent)
                .accessibilityIdentifier("home.deckMenu.statistics.\(deck.id)")
            }
        }
        // A separator, not a Section: a section's header item took the
        // items' identifiers away in the Mac's context menu.
        Divider()
        if deck.filtered {
            FilteredDeckActions(collection: collection, deck: deck.id)
            Divider()
        } else {
            // N20.2: actions on this deck only. Create Filtered Deck makes another
            // deck, so it lives in the Add menu and the Deck menu (owner, 2026-11-27).
            Button { collection.filteredStudy.custom(deck.id) } label: {
                Label { Text("Custom Study", bundle: .module) } icon: { Image(systemName: "slider.horizontal.3") }
            }
            .disabled(!collection.filteredStudy.canCustom(deck.id))
            .accessibilityIdentifier("home.deckMenu.customStudy.\(deck.id)")
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
            contextMenu {
                DeckMenu(collection: collection, deck: deck, deletionAnchor: .row(place: place))
                    .disabled(collection.navigation.sidebarInteractionPending)
            }
                .deckDeletionQuestion(collection: collection, deckID: deck.id, place: place)
        } else {
            self
        }
    }
}
