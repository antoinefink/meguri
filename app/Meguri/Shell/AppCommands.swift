import AppModel
import CoreBridge
import Home
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// What the menu bar acts on in the focused window (N4, N20.4). Each command
/// uses the action and the guard of its visible control.
struct NativeCommandContext {
    let collection: ReviewCollection
    let host: AppNavigation.SheetHost
    var layout: ShellLayout = .sidebar

    var fileActionsAvailable: Bool {
        collection.navigation.sheet == nil && !collection.deckActions.busy && !collection.manualTransfers.busy
            && !collection.entry.pickerPresented && collection.manualTransfers.preview == nil && collection.manualTransfers.mediaPreview == nil
    }
    func present(_ route: SheetRoute) {
        guard collection.navigation.sheet == nil else { return }
        switch route {
        case .collectionEntry:
            guard fileActionsAvailable, collection.entry.canPresent else { return }
            collection.entry.present()
        case .collectionExport:
            guard fileActionsAvailable, collection.collectionExport.canPresent else { return }
            collection.collectionExport.start()
        case .manualTransfer:
            // Reopening must also admit an active operation or authoritative
            // preview. Only the workflow's explicit actions start work.
            guard !collection.entry.busy, !collection.collectionExport.busy else { return }
            collection.navigation.present(.manualTransfer)
        default: return
        }
        collection.navigation.sheetHost = host
    }

    // MARK: File

    /// The Add menu's New Note: on a deck page it presets that deck.
    private var newNoteDeck: Int64? {
        guard let deck = shownDeck, collection.deck(deck)?.filtered != true else { return nil }
        return deck
    }
    var canNewNote: Bool { host == .main && collection.addNotePresentation.canPresent(presetDeck: newNoteDeck) }
    func newNote() {
        guard canNewNote else { return }
        collection.navigation.present(.add(deck: newNoteDeck.map { DeckID($0) }))
    }
    var canNewDeck: Bool { collection.deckActions.canPresent(.create, host: host) }
    func newDeck() {
        guard canNewDeck else { return }
        collection.deckActions.present(.create, host: host)
    }

    // MARK: Edit

    var canFind: Bool { host == .main && DeckSearchFocus.available(collection, layout: layout) }
    func find() {
        guard canFind else { return }
        DeckSearchFocus.request(collection, layout: layout)
    }

    // MARK: Deck

    /// The deck the workspace shows: the sidebar's selection, or the deck page.
    var shownDeck: Int64? {
        guard host == .main, fileActionsAvailable, collection.navigation.section == .home,
              collection.review == nil, collection.navigation.review == nil,
              let deck = collection.navigation.homeDeckPage?.rawValue,
              collection.deck(deck) != nil || collection.deckPage(deck) != nil else { return nil }
        return deck
    }
    /// N14.3: as the deck page, no Study while a regular deck has nothing waiting; never a filtered deck.
    var canStudyShownDeck: Bool {
        guard let deck = shownDeck, collection.deck(deck)?.filtered != true else { return false }
        return collection.canStudy && collection.offersStudy(deckID: deck)
    }
    var canRenameShownDeck: Bool { shownDeck.map { collection.deckActions.canPresent(.rename($0), host: host) } ?? false }
    var canAddSubdeck: Bool { shownDeck.map { collection.deckActions.canPresent(.subdeck($0), host: host) } ?? false }

    // MARK: Card

    var review: ReviewCoordinator? {
        guard host == .main, collection.navigation.sheet == nil else { return nil }
        return collection.review
    }
    var reviewInputAvailable: Bool {
        guard let review else { return false }
        return ReviewInputOwnership.admitsNativeCommand(review)
    }
}

private struct NativeCommandContextKey: FocusedValueKey { typealias Value = NativeCommandContext }
extension FocusedValues {
    var nativeCommandContext: NativeCommandContext? {
        get { self[NativeCommandContextKey.self] }
        set { self[NativeCommandContextKey.self] = newValue }
    }
}

/// N20.4: File, Edit, Deck and Card, the same on the Mac and in the iPad menu
/// bar. There is no Study menu: Undo and Redo are Edit's alone.
struct AppCommands: Commands {
    @FocusedValue(\.nativeCommandContext) private var context

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Note") { context?.newNote() }
                .keyboardShortcut("n", modifiers: .command)
                .disabled(context?.canNewNote != true)
            Button("New Deck…") { context?.newDeck() }
                .keyboardShortcut("n", modifiers: [.command, .shift])
                .disabled(context?.canNewDeck != true)
        }
        CommandGroup(replacing: .importExport) {
            Button("Import Collection…") { context?.present(.collectionEntry) }
                .keyboardShortcut("i", modifiers: [.command, .shift])
                .disabled(context?.fileActionsAvailable != true || context?.collection.entry.canPresent != true)
            Button("Export Collection…") { context?.present(.collectionExport) }
                .keyboardShortcut("e", modifiers: [.command, .shift])
                .disabled(context?.fileActionsAvailable != true || context?.collection.collectionExport.canPresent != true)
        }
        // A review's answers are the undo target while it shows; otherwise the
        // system's group keeps text fields' own undo (R173, R174).
        if context?.review != nil {
            CommandGroup(replacing: .undoRedo) {
                Button(undoTitle) { restore(redo: false) }
                    .keyboardShortcut("z", modifiers: .command)
                    .disabled(context?.review?.canUndo != true)
                    .accessibilityIdentifier("shell.command.undo")
                Button(redoTitle) { restore(redo: true) }
                    .keyboardShortcut("z", modifiers: [.command, .shift])
                    .disabled(context?.review?.canRedo != true)
                    .accessibilityIdentifier("shell.command.redo")
            }
        }
        CommandGroup(replacing: .textEditing) {
            // ⇧⌘O is its second key, with no item of its own (Home, HomeSidebarDeckSearch).
            Button("Find…") { context?.find() }
                .keyboardShortcut("f", modifiers: .command)
                .disabled(context?.canFind != true)
                .accessibilityIdentifier("shell.command.find")
        }
        CommandMenu("Deck") {
            Button("Study") {
                guard let context, let deck = context.shownDeck, context.canStudyShownDeck else { return }
                Task { await context.collection.startStudy(deckID: deck) }
            }
            .keyboardShortcut("r", modifiers: .command)
            .disabled(context?.canStudyShownDeck != true)
            Divider()
            Button("Rename…") {
                guard let context, let deck = context.shownDeck, context.canRenameShownDeck else { return }
                context.collection.deckActions.present(.rename(deck), host: context.host)
            }
            .keyboardShortcut("r", modifiers: [.command, .shift])
            .disabled(context?.canRenameShownDeck != true)
            Button("New Subdeck…") {
                guard let context, let deck = context.shownDeck, context.canAddSubdeck else { return }
                context.collection.deckActions.present(.subdeck(deck), host: context.host)
            }
            .disabled(context?.canAddSubdeck != true)
        }
        CommandMenu("Card") {
            Button("Show Answer") {
                guard context?.reviewInputAvailable == true else { return }
                context?.review?.reveal()
            }
            .keyboardShortcut(.space, modifiers: [])
            .disabled(context?.reviewInputAvailable != true || context?.review?.canReveal != true)
            Divider()
            rating("Again", .again, key: "1")
            rating("Hard", .hard, key: "2")
            rating("Good", .good, key: "3")
            rating("Easy", .easy, key: "4")
            Divider()
            Button("Back to Decks") {
                guard let context, context.reviewInputAvailable, context.review?.canFinish == true else { return }
                Task { await context.collection.finishReview() }
            }
            .keyboardShortcut(.escape, modifiers: [])
            .disabled(context?.reviewInputAvailable != true || context?.review?.canFinish != true)
        }
    }

    /// R173: "Undo <operation>" while one can be undone, plain "Undo" otherwise.
    private var undoTitle: String {
        guard context?.review?.canUndo == true, case .available(let step)? = context?.review?.undoStatus else {
            return String(localized: "Undo")
        }
        return String(localized: "Undo \(step.name)")
    }
    private var redoTitle: String {
        guard context?.review?.canRedo == true, case .available(let step)? = context?.review?.redoStatus else {
            return String(localized: "Redo")
        }
        return String(localized: "Redo \(step.name)")
    }

    /// The review's own undo while the card owns the keys; a text field in the
    /// card keeps its native undo (N4).
    private func restore(redo: Bool) {
        guard let context, let review = context.review else { return }
        guard context.reviewInputAvailable else {
            #if os(macOS)
            NSApp.sendAction(redo ? Selector(("redo:")) : Selector(("undo:")), to: nil, from: nil)
            #endif
            return
        }
        review.restore(redo: redo)
    }

    private func rating(_ title: String, _ rating: ReviewRating, key: KeyEquivalent) -> some View {
        Button(LocalizedStringKey(title)) {
            guard context?.reviewInputAvailable == true else { return }
            context?.review?.rate(rating)
        }
        .keyboardShortcut(key, modifiers: [])
        .disabled(context?.reviewInputAvailable != true || context?.review?.canRate != true)
    }
}
