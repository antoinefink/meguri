import AppModel
import Browse
import CoreBridge
import Editor
import Home
import Stats
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
    /// The Statistics page, behind its flag (U296); nil while it cannot be reached.
    var statsPage: StatsPage?

    var fileActionsAvailable: Bool {
        collection.navigation.sheet == nil && !collection.deckActions.busy && !collection.manualTransfers.busy
            && !collection.entry.pickerPresented && !collection.intake.pickerPresented && collection.manualTransfers.preview == nil && collection.manualTransfers.mediaPreview == nil
    }
    func present(_ route: SheetRoute) {
        guard collection.navigation.sheet == nil else { return }
        switch route {
        case .collectionEntry:
            guard fileActionsAvailable, collection.entry.canPresent else { return }
            collection.entry.present()
        case .deckImport:
            // File › Import… (U162, `package/file-intake` K1): the picker of the intake.
            guard fileActionsAvailable, collection.intake.canChoose else { return }
            collection.intake.choose(host: host)
            return
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

    /// Add's separate native window does not make the main workspace modal.
    /// Its own focused field still owns all editing commands.
    private var workspaceOwnsCommands: Bool {
        #if os(macOS)
        if case .add = collection.navigation.sheet {
            return NSApp.keyWindow?.identifier?.rawValue != "editor.add.window"
        }
        #endif
        return collection.navigation.sheet == nil
    }

    // MARK: Edit

    var canFind: Bool { host == .main && (canFindInBrowse || DeckSearchFocus.available(collection, layout: layout)) }
    func find() {
        guard canFind else { return }
        if canFindInBrowse { return collection.browse.focusField() }
        DeckSearchFocus.request(collection, layout: layout)
    }
    /// N21.8, T10: in Browse, Find… focuses Browse's search field.
    private var canFindInBrowse: Bool {
        layout == .sidebar && collection.navigation.section == .browse && workspaceOwnsCommands
            && collection.review == nil && collection.navigation.review == nil && collection.hasCollection
    }

    // MARK: Browse (U275, N21.8)

    /// Browse shows in the main window, with no sheet or review: the Note
    /// and Card menus' Browse items act on its selection (N21.8); outside it
    /// they are disabled.
    var browseCommands: BrowseCommands? {
        host == .main && canFindInBrowse ? collection.browse.commands : nil
    }
    func browseState(_ command: BrowseCommand) -> BrowseCommandState? { browseCommands?.state(command) }
    func browseEnabled(_ command: BrowseCommand) -> Bool { browseState(command)?.enabled == true }
    func performBrowse(_ command: BrowseCommand) {
        guard browseEnabled(command) else { return }
        browseCommands?.perform(command)
    }
    /// The menu bar's title of a Browse command for the selection.
    func browseTitle(_ command: BrowseCommand) -> String {
        let state = browseState(command) ?? BrowseCommandState.of(command, facts: nil, available: false)
        return BrowseCommandText.title(command, state)
    }
    /// N21.8: ⌘⌫ deletes notes only while the table has the focus, and never
    /// takes the key from a text input (H9.2).
    var canDeleteNotes: Bool {
        guard browseEnabled(.delete) else { return false }
        #if os(macOS)
        // The results table is the first responder (its outline view carries the
        // table's identifier): never a text input, the sidebar or the inspector.
        return BrowseResponder.tableIsFirst(MacUndoResponder.shared.firstResponder)
            || (collection.browse.results.tableFocused && !(MacUndoResponder.shared.firstResponder is NSText))
        #else
        return collection.browse.results.tableFocused
        #endif
    }
    var canSelectSiblings: Bool { browseCommands?.canSelectSiblings == true }

    // MARK: The inspector (U273, N21.3, N21.4, N21.8)

    /// The inspector while Browse shows in the main window.
    var browseInspector: BrowseInspector? { browseCommands == nil ? nil : collection.browse.inspector }
    /// N21.8: Note › Edit Note in Browse: the inspector's first field.
    var canEditBrowseNote: Bool { browseEnabled(.editNote) && browseInspector?.canEditNote == true }
    /// The focused field page of the inspector's note, for the Format menu.
    var formattingHost: FieldEditorHost? { browseInspector?.note?.runtime as? FieldEditorHost }
    /// The flags' shown names (`tags/flags-and-marked` F4).
    var flagNames: [String] { collection.browse.flagNames }

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

    /// N21.1, N21.8: View › Browse (⇧⌘B) selects the Browse row, which
    /// returns to Browse's kept state (`browser/workspace` V1); disabled
    /// while review presents a card, when Card › Show in Browse takes the
    /// key, and while a sheet holds the window.
    var canShowBrowse: Bool {
        host == .main && layout == .sidebar && workspaceOwnsCommands
            && collection.review == nil && collection.navigation.review == nil && collection.hasCollection
    }
    func showBrowse() {
        guard canShowBrowse else { return }
        collection.navigation.select(.section(.browse))
    }

    /// N25.11: View › Statistics (⇧⌘S) selects the Statistics row, with the page's kept deck
    /// and period (S13), under the guards of View › Browse.
    var canShowStatistics: Bool {
        statsPage != nil && host == .main && layout == .sidebar && workspaceOwnsCommands
            && collection.review == nil && collection.navigation.review == nil && collection.hasCollection
    }
    func showStatistics() {
        guard canShowStatistics else { return }
        collection.navigation.select(.section(.stats))
    }

    /// N25.11: View › Go to Section, while the page shows its sections.
    var canGoToSection: Bool {
        canShowStatistics && collection.navigation.section == .stats && statsPage?.shown == true
            && statsPage?.model.state == .content
    }
    func goToSection(_ section: StatsSectionID) {
        guard canGoToSection else { return }
        statsPage?.go(to: section)
    }

    /// N21.8, V2.1: View › Cards and Notes (⌥⌘T) toggles Browse's mode
    /// while Browse is shown.
    var canToggleBrowseMode: Bool {
        canShowBrowse && collection.navigation.section == .browse
    }
    func toggleBrowseMode() {
        guard canToggleBrowseMode else { return }
        let browse = collection.browse
        browse.setMode(browse.mode == .cards ? .notes : .cards)
    }

    /// U272: View › Columns lists Browse's columns for the mode shown, while
    /// Browse shows its table.
    var browseColumns: BrowseModel? {
        canToggleBrowseMode && collection.browse.results.columnSet != nil ? collection.browse : nil
    }

    /// V13: View › Card Info (⌥⌘I) shows the inspector with Info selected.
    var canShowCardInfo: Bool { browseInspector != nil }
    func toggleCardInfo() {
        guard canShowCardInfo else { return }
        browseInspector?.toggleCardInfo()
    }

    /// W14: View › Note Types (⇧⌘T) selects the sidebar row, outside review,
    /// with no sheet over the window, and once a collection exists.
    var canShowNoteTypes: Bool {
        host == .main && layout == .sidebar && workspaceOwnsCommands
            && collection.review == nil && collection.navigation.review == nil && collection.hasCollection
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
            // U162: one Import… for deck and collection packages; the file decides the flow.
            Button("Import…") { context?.present(.deckImport) }
                .keyboardShortcut("i", modifiers: [.command, .shift])
                .disabled(context?.fileActionsAvailable != true || context?.collection.intake.canChoose != true)
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
        // N21.8: View › Browse (the Mac only: the iPad's Browse row waits for its own layout);
        // W14: View › Note Types; ⇧⌘N stays New Deck… (N22.1).
        CommandGroup(before: .sidebar) {
            // N25.1, N25.11: Statistics before Browse, behind its flag (U296).
            if StatsRollout.enabled {
                Button("Statistics") { context?.showStatistics() }
                    .keyboardShortcut("s", modifiers: [.command, .shift])
                    .disabled(context?.canShowStatistics != true)
                Menu("Go to Section") {
                    ForEach(StatsSectionID.allCases, id: \.self) { section in
                        Button { context?.goToSection(section) } label: { Text(verbatim: section.title()) }
                    }
                }
                .disabled(context?.canGoToSection != true)
                .accessibilityIdentifier("stats.goToSection")
            }
            // N21.8's order: Browse, Note Types, Cards and Notes, Card Info.
            #if os(macOS)
            Button("Browse") { context?.showBrowse() }
                .keyboardShortcut("b", modifiers: [.command, .shift])
                .disabled(context?.canShowBrowse != true)
            #endif
            Button("Note Types") { context?.showNoteTypes() }
                .keyboardShortcut("t", modifiers: [.command, .shift])
                .disabled(context?.canShowNoteTypes != true)
            #if os(macOS)
            Button("Cards and Notes") { context?.toggleBrowseMode() }
                .keyboardShortcut("t", modifiers: [.command, .option])
                .disabled(context?.canToggleBrowseMode != true)
            // N21.3: the inspector's toggle, ⌃⌘I as in the system's InspectorCommands.
            Button(LocalizedStringKey(context?.browseInspector?.shown == true ? "Hide Inspector" : "Show Inspector")) {
                context?.browseInspector?.toggle()
            }
                .keyboardShortcut("i", modifiers: [.command, .control])
                .disabled(context?.browseInspector == nil)
            // U272: the columns of the mode shown, with checks (V2.3).
            Menu("Columns") {
                if let browse = context?.browseColumns { BrowseColumnsMenuContent(model: browse) }
            }
                .disabled(context?.browseColumns == nil)
            // V13: the inspector with its Info tab (U274).
            Button("Card Info") { context?.toggleCardInfo() }
                .keyboardShortcut("i", modifiers: [.command, .option])
                .disabled(context?.canShowCardInfo != true)
            #endif
            Divider()
        }
        // N21.8: Select All (the system's, ⌘A in the table) is followed by
        // Select Sibling Cards.
        CommandGroup(after: .pasteboard) {
            Button(BrowseCommandText.selectSiblings()) { context?.browseCommands?.selectSiblings() }
                .disabled(context?.canSelectSiblings != true)
        }
        CommandGroup(before: .textEditing) {
            Button("Find…") { context?.find() }
                .keyboardShortcut("f", modifiers: .command)
                .disabled(context?.canFind != true)
            // N21.8: their sheets (U277), also with no selection.
            browseItem(.findReplace, key: "f", [.command, .option])
            browseItem(.findDuplicates)
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
        // N21.8: the Note menu, between Deck and Card. Edit Note is the review's
        // (N20.8) or, in Browse, the inspector's first field (U273: disabled until then).
        CommandMenu("Note") {
            // N21.8: the review's Edit Note (N20.8) or, in Browse, the inspector's first field.
            Button("Edit Note") {
                guard let context else { return }
                if context.canEditBrowseNote { return context.performBrowse(.editNote) }
                guard context.reviewInputAvailable, context.collection.editNoteFlow.canOpen else { return }
                Task { await context.collection.openEditNote() }
            }
            .keyboardShortcut("e", modifiers: .command)
            .disabled(!cardEnabled(context?.collection.editNoteFlow.canOpen) && context?.canEditBrowseNote != true)
            Divider()
            browseItem(.addTags, key: "a", [.command, .shift])
            browseItem(.removeTags, key: "a", [.command, .option, .shift])
            Divider()
            browseItem(.changeNoteType, key: "m", [.command, .shift])
            browseItem(.editNoteType)
            Divider()
            Button(context?.browseTitle(.delete) ?? BrowseCommandText.title(.delete, .of(.delete, facts: nil, available: false))) {
                guard context?.canDeleteNotes == true else { return }
                context?.performBrowse(.delete)
            }
            .keyboardShortcut(.delete, modifiers: .command)
            .disabled(context?.canDeleteNotes != true)
        }
        #if os(macOS)
        // N21.8: the Format menu, acting on the inspector's focused rich field
        // (`editor/formatting` F2). Highlight, Text Colour, Lists and Remove
        // Formatting come with the editor's formatting unit (U192).
        CommandMenu("Format") {
            formatItem("Bold", .bold, key: "b")
            formatItem("Italic", .italic, key: "i")
            formatItem("Underline", .underline, key: "u")
            Divider()
            // F14: ⌘= Superscript, ⇧⌘= Subscript (decided 2026-10-27).
            formatItem("Superscript", .superscript, key: "=")
            formatItem("Subscript", .`subscript`, key: "=", [.command, .shift])
            Divider()
            Button("Cloze Deletion") { context?.formattingHost?.cloze(sameNumber: false) }
                .keyboardShortcut("c", modifiers: [.command, .shift])
                .disabled(context?.formattingHost?.canCloze != true)
            Button("Cloze Deletion (Same Number)") { context?.formattingHost?.cloze(sameNumber: true) }
                .keyboardShortcut("c", modifiers: [.command, .shift, .option])
                .disabled(context?.formattingHost?.canCloze != true)
        }
        #endif
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
            // N21.8: Browse's card commands (Change Deck… comes with U276's sheet).
            browseItem(.changeDeck, key: "d", [.command])
            flagMenu
            Divider()
            // N20.8 and N21.8: one item per command and key, the review's card in
            // review, Browse's selection in Browse (the two never show together).
            Button(reviewOrBrowse(.bury, review: "Bury Card")) {
                if context?.review != nil {
                    guard context?.reviewInputAvailable == true else { return }
                    context?.review?.act(.bury)
                } else {
                    context?.performBrowse(.bury)
                }
            }
            .keyboardShortcut("j", modifiers: .command)
            .disabled(!cardEnabled(context?.review?.canActOnCard) && context?.browseEnabled(.bury) != true)
            browseItem(.unbury)
            Button(reviewOrBrowse(.suspend, review: "Suspend Card")) {
                if context?.review != nil {
                    guard context?.reviewInputAvailable == true else { return }
                    context?.review?.act(.suspend)
                } else {
                    context?.performBrowse(.suspend)
                }
            }
            .keyboardShortcut("j", modifiers: [.command, .shift])
            .disabled(!cardEnabled(context?.review?.canActOnCard) && context?.browseEnabled(.suspend) != true)
            browseItem(.unsuspend)
            Divider()
            browseItem(.setDueDate, key: "d", [.command, .shift])
            browseItem(.forget, key: "n", [.command, .option])
            Divider()
            Button("Back to Decks") {
                guard let context, context.reviewInputAvailable, context.review?.canFinish == true else { return }
                Task { await context.collection.finishReview() }
            }
            .keyboardShortcut(cardKey(.escape))
            .disabled(!cardEnabled(context?.review?.canFinish))
        }
    }

    /// A Browse command in the menu bar (N21.8): its title for the selection,
    /// its key, enabled as V8.1 says while Browse shows, disabled elsewhere.
    private func browseItem(_ command: BrowseCommand, key: KeyEquivalent? = nil, _ modifiers: EventModifiers = []) -> some View {
        Button(context?.browseTitle(command) ?? BrowseCommandText.title(command, .of(command, facts: nil, available: false))) {
            context?.performBrowse(command)
        }
        .keyboardShortcut(key.map { KeyboardShortcut($0, modifiers: modifiers) })
        .disabled(context?.browseEnabled(command) != true)
    }

    #if os(macOS)
    private func formatItem(_ title: LocalizedStringKey, _ format: FieldEditorHost.InlineFormat, key: KeyEquivalent,
                            _ modifiers: EventModifiers = .command) -> some View {
        Button(title) { context?.formattingHost?.format(format) }
            .keyboardShortcut(key, modifiers: modifiers)
            .disabled(context?.formattingHost?.canFormat != true)
    }
    #endif

    /// Bury Card and Suspend Card: the review's title in review, Browse's
    /// ("Bury Cards" for several) otherwise.
    private func reviewOrBrowse(_ command: BrowseCommand, review title: String) -> String {
        guard let context, context.review == nil, context.browseCommands != nil else { return title }
        return context.browseTitle(command)
    }

    /// Flag ▸ (N21.8): No Flag ⌘0, then the seven flags by shown name, ⌘1 to ⌘7.
    private var flagMenu: some View {
        Menu(BrowseCommandText.title(.flag(1), .of(.flag(1), facts: nil, available: false))) {
            ForEach(Array(UInt8(0)...7), id: \.self) { flag in
                Button { context?.performBrowse(.flag(flag)) } label: {
                    if flag == 0 {
                        Text(verbatim: BrowseCommandText.flag(0, names: context?.flagNames ?? []))
                    } else {
                        Label { Text(verbatim: BrowseCommandText.flag(flag, names: context?.flagNames ?? [])) }
                            icon: { BrowseFlagSymbol(flag: flag) }
                    }
                }
                    .keyboardShortcut(KeyEquivalent(Character(String(flag))), modifiers: .command)
                    .disabled(context?.browseEnabled(.flag(flag)) != true)
                if flag == 0 { Divider() }
            }
        }
        .disabled(context?.browseCommands == nil)
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
            // The Mac's accessibility tree still lists this invisible, empty 24 x 8 button behind
            // every page (accessibilityHidden does not take a button out of it), so it is named.
            .accessibilityIdentifier("shell.root.findSecondKey")
    }
}
