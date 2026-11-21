import AppModel
import CoreBridge
import Editor
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
    var canOpenDeckOptions: Bool {
        guard host == .main, review == nil, collection.navigation.sheet == nil, let deck = shownDeck else { return false }
        return collection.deckOptions.canOpen(deckID: deck)
    }
    /// N20.4, N20.7: the shown deck, filtered included, outside review, with no
    /// sheet, panel or text input owning ⌘⌫ (H9.2): a focused text view keeps
    /// it for deleting to the line's start.
    var canDeleteShownDeck: Bool {
        guard let deck = shownDeck, collection.deckDeletion.canDelete(deckID: deck, host: host) else { return false }
        #if os(macOS)
        return !(MacUndoResponder.shared.firstResponder is NSText)
        #else
        // UIKit gives a text input's own ⌘⌫ priority over menu key commands.
        return true
        #endif
    }
    func deleteShownDeck() {
        guard canDeleteShownDeck, let deck = shownDeck else { return }
        collection.deckDeletion.delete(deckID: deck, host: host)
    }

    // MARK: View

    /// W14: View › Note Types (⇧⌘T) selects the sidebar row, outside review
    /// and with no sheet over the window.
    var canShowNoteTypes: Bool {
        host == .main && layout == .sidebar && collection.navigation.sheet == nil
            && collection.review == nil && collection.navigation.review == nil
    }
    func showNoteTypes() {
        guard canShowNoteTypes else { return }
        collection.navigation.select(.section(.noteTypes))
    }

    /// D3: File › Save (⌘S) saves the shown note type's draft, as the toolbar's Save does.
    var noteTypeSaveShown: Bool {
        host == .main && collection.navigation.section == .noteTypes && collection.navigation.sheet == nil
            && collection.review == nil && collection.navigation.review == nil
    }
    var canSaveNoteType: Bool {
        guard noteTypeSaveShown, let id = collection.noteTypes.selection else { return false }
        return collection.noteTypes.canSave(id)
    }
    var noteTypeSaveAsks: Bool {
        guard noteTypeSaveShown, let id = collection.noteTypes.selection else { return false }
        return collection.noteTypes.saveAsks(id)
    }
    func saveNoteType() {
        guard canSaveNoteType else { return }
        collection.noteTypes.save()
    }

    // MARK: Card

    var review: ReviewCoordinator? {
        guard host == .main, collection.navigation.sheet == nil else { return nil }
        return collection.review
    }
    var reviewInputAvailable: Bool {
        guard let review else { return false }
        return ReviewInputOwnership.admitsNativeCommand(review)
    }

    // MARK: Undo and Redo (R173, R174)

    /// The main window's own Undo and Redo, while no sheet or panel holds the
    /// keys: there the system's items keep the text inputs' undo.
    var replacesUndo: Bool {
        guard host == .main, collection.navigation.sheet == nil else { return false }
        #if os(iOS)
        // iPad: a text input that is editing keeps the system's Undo (R174);
        // during review the card's own key handling is unchanged.
        if review == nil, TextInputFocus.shared.editing { return false }
        #endif
        return true
    }

    func undoRoute(redo: Bool) -> EditUndoRouting.Route {
        var inputs = EditUndoRouting.Inputs()
        inputs.reviewShown = review != nil
        #if os(macOS)
        let responder = MacUndoResponder.shared.firstResponder
        // In review a focused text input in the card or a control owns it; else a text view.
        inputs.textOwnsUndo = inputs.reviewShown ? responder != nil && !reviewInputAvailable : responder is NSTextView
        let manager = responder?.undoManager
        inputs.textAvailable = (redo ? manager?.canRedo : manager?.canUndo) == true
        inputs.textTitle = redo ? manager?.redoMenuItemTitle : manager?.undoMenuItemTitle
        inputs.reviewOwnsKeys = reviewInputAvailable
        #else
        // Observable state only: the card host keeps the keys (N11.3).
        inputs.reviewOwnsKeys = review?.paused == false
        #endif
        if let review {
            let available = redo ? review.canRedo : review.canUndo
            inputs.reviewStep = available ? CollectionUndoModel.stepName(redo ? review.redoStatus : review.undoStatus) : nil
        }
        // W13: while Note Types shows, its draft's own steps come before the collection's.
        if !inputs.reviewShown, !inputs.textOwnsUndo, collection.navigation.section == .noteTypes {
            let draft = collection.noteTypes
            inputs.draftStep = (redo ? draft.redoStep : draft.undoStep)?.rawValue
        }
        let history = collection.collectionUndo
        inputs.collectionStep = (redo ? history.canRedo : history.canUndo) ? (redo ? history.redoName : history.undoName) : nil
        return EditUndoRouting.route(inputs)
    }

    func performUndo(redo: Bool) {
        let route = undoRoute(redo: redo)
        guard route.enabled else { return }
        switch route.target {
        case .text:
            #if os(macOS)
            NSApp.sendAction(redo ? Selector(("redo:")) : Selector(("undo:")), to: nil, from: nil)
            #endif
        case .review:
            guard reviewInputAvailable else { return }
            review?.restore(redo: redo)
        case .draft:
            if redo { collection.noteTypes.redo() } else { collection.noteTypes.undo() }
        case .collection:
            if redo { collection.collectionUndo.redo() } else { collection.collectionUndo.undo() }
        }
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
        CommandGroup(replacing: .saveItem) {
            Button(LocalizedStringKey(context?.noteTypeSaveAsks == true ? "Save…" : "Save")) { context?.saveNoteType() }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(context?.canSaveNoteType != true)
        }
        CommandGroup(replacing: .importExport) {
            Button("Import Collection…") { context?.present(.collectionEntry) }
                .keyboardShortcut("i", modifiers: [.command, .shift])
                .disabled(context?.fileActionsAvailable != true || context?.collection.entry.canPresent != true)
            Button("Export Collection…") { context?.present(.collectionExport) }
                .keyboardShortcut("e", modifiers: [.command, .shift])
                .disabled(context?.fileActionsAvailable != true || context?.collection.collectionExport.canPresent != true)
        }
        // One Undo and one Redo (N20.4): the text input's, the review's or the
        // collection's (EditUndoRouting); a sheet or panel keeps the system's.
        if let context, context.replacesUndo {
            CommandGroup(replacing: .undoRedo) {
                let undo = context.undoRoute(redo: false), redo = context.undoRoute(redo: true)
                Button(Self.title(undo, redo: false)) { context.performUndo(redo: false) }
                    .keyboardShortcut(undoKey("z", modifiers: .command, review: context.review != nil))
                    .disabled(!undo.enabled)
                Button(Self.title(redo, redo: true)) { context.performUndo(redo: true) }
                    .keyboardShortcut(undoKey("z", modifiers: [.command, .shift], review: context.review != nil))
                    .disabled(!redo.enabled)
            }
        }
        // The system's Find, Spelling, Substitutions and other text groups stay;
        // Find… comes first, so ⌘F reaches it while the deck workspace shows and
        // the text inputs' own Find otherwise (when it is disabled).
        // W14: View › Note Types; ⇧⌘N stays New Deck… (N22.1).
        CommandGroup(before: .sidebar) {
            Button("Note Types") { context?.showNoteTypes() }
                .keyboardShortcut("t", modifiers: [.command, .shift])
                .disabled(context?.canShowNoteTypes != true)
            Divider()
        }
        CommandGroup(before: .textEditing) {
            Button("Find…") { context?.find() }
                .keyboardShortcut("f", modifiers: .command)
                .disabled(context?.canFind != true)
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
            // N20.9: the shown normal deck's options; disabled during review.
            Button("Deck Options…") {
                guard let context, let deck = context.shownDeck, context.canOpenDeckOptions else { return }
                context.collection.openDeckOptions(deckID: deck)
            }
            .keyboardShortcut("o", modifiers: [.command, .option])
            .disabled(context?.canOpenDeckOptions != true)
            Divider()
            Button("Delete Deck") { context?.deleteShownDeck() }
                .keyboardShortcut(.delete, modifiers: .command)
                .disabled(context?.canDeleteShownDeck != true)
        }
        CommandMenu("Card") {
            Button("Show Answer") {
                guard context?.reviewInputAvailable == true else { return }
                context?.review?.reveal()
            }
            .keyboardShortcut(cardKey(.space))
            .disabled(!cardEnabled(context?.review?.canReveal))
            Divider()
            rating("Again", .again, key: "1")
            rating("Hard", .hard, key: "2")
            rating("Good", .good, key: "3")
            rating("Easy", .easy, key: "4")
            Divider()
            // N20.8: the review's card actions, with the More menu's keys.
            Button("Edit Note") {
                guard let context, context.reviewInputAvailable, context.collection.editNoteFlow.canOpen else { return }
                Task { await context.collection.openEditNote() }
            }
            .keyboardShortcut("e", modifiers: .command)
            .disabled(!cardEnabled(context?.collection.editNoteFlow.canOpen))
            Button("Bury Card") {
                guard context?.reviewInputAvailable == true else { return }
                context?.review?.act(.bury)
            }
            .keyboardShortcut("j", modifiers: .command)
            .disabled(!cardEnabled(context?.review?.canActOnCard))
            Button("Suspend Card") {
                guard context?.reviewInputAvailable == true else { return }
                context?.review?.act(.suspend)
            }
            .keyboardShortcut("j", modifiers: [.command, .shift])
            .disabled(!cardEnabled(context?.review?.canActOnCard))
            Divider()
            Button("Back to Decks") {
                guard let context, context.reviewInputAvailable, context.review?.canFinish == true else { return }
                Task { await context.collection.finishReview() }
            }
            .keyboardShortcut(cardKey(.escape))
            .disabled(!cardEnabled(context?.review?.canFinish))
        }
    }

    /// R173: "Undo <operation>" while one can be undone, plain "Undo" otherwise.
    private static func title(_ route: EditUndoRouting.Route, redo: Bool) -> String {
        switch route.title {
        case .text(let text): return text
        case .named(let step):
            // S5: the review's own steps by their display names ("Undo Bury Card");
            // the collection's steps keep their V4 names ("Undo Update Note").
            let name = route.target == .review ? ReviewStepNames.display(step) : step
            return redo ? String(localized: "Redo \(name)") : String(localized: "Undo \(name)")
        case .plain: return redo ? String(localized: "Redo") : String(localized: "Undo")
        }
    }

    /// On iPad the card host keeps its keys during review (N11.3): one key path.
    private func undoKey(_ key: KeyEquivalent, modifiers: EventModifiers, review: Bool) -> KeyboardShortcut? {
        #if os(macOS)
        KeyboardShortcut(key, modifiers: modifiers)
        #else
        review ? nil : KeyboardShortcut(key, modifiers: modifiers)
        #endif
    }
    /// The Card menu's unmodified keys: the Mac's menu shows and takes them; on
    /// iPad the card host owns them (N11.3), so the items carry none.
    private func cardKey(_ key: KeyEquivalent) -> KeyboardShortcut? {
        #if os(macOS)
        KeyboardShortcut(key, modifiers: [])
        #else
        nil
        #endif
    }
    /// The Mac reads the card's input ownership; iPad only state SwiftUI observes.
    private func cardEnabled(_ available: Bool?) -> Bool {
        #if os(macOS)
        context?.reviewInputAvailable == true && available == true
        #else
        context?.review?.paused == false && available == true
        #endif
    }

    private func rating(_ title: String, _ rating: ReviewRating, key: KeyEquivalent) -> some View {
        Button(LocalizedStringKey(title)) {
            guard context?.reviewInputAvailable == true else { return }
            context?.review?.rate(rating)
        }
        .keyboardShortcut(cardKey(key))
        .disabled(!cardEnabled(context?.review?.canRate))
    }
}

/// ⇧⌘O, Find…'s second key (N20.4): a key equivalent of the window with no
/// menu item of its own, so it also acts while a text input has the focus, as
/// menu keys do (H9.2). The same action and guard as Find….
struct FindSecondKey: View {
    let context: NativeCommandContext
    var body: some View {
        Button { context.find() } label: { EmptyView() }
            .keyboardShortcut("o", modifiers: [.command, .shift])
            .disabled(!context.canFind)
            .opacity(0)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}
