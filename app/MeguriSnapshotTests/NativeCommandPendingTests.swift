#if os(macOS)
import AppKit
#endif
import CoreBridge
import Foundation
import Observation
import Testing
@testable import AppModel
@testable import Meguri

/// Model-only command admission regressions: no window, app activation or Core calls.
@MainActor @Suite struct NativeCommandPendingTests {
    #if os(macOS)
    @Test func sidebarFrameHandoffBlocksFormerDestinationCommandsButAllowsNewNavigation() async throws {
        var configuration = LaunchConfiguration()
        configuration.deviceDefaults = nil
        let navigation = AppNavigation()
        let collection = ReviewCollection(configuration: configuration, navigation: navigation)
        collection.loadDeckTree = {
            CountedDeckTree(generation: 1, nextChangeAt: Int64.max, nodes: [
                CountedDeckNode(id: 1, name: "A", filtered: false, studyCollapsed: false,
                    browserCollapsed: false, counts: ReviewCounts(new: 1, learning: 0, review: 0), children: []),
                CountedDeckNode(id: 2, name: "B", filtered: false, studyCollapsed: false,
                    browserCollapsed: false, counts: ReviewCounts(new: 1, learning: 0, review: 0), children: []),
            ])
        }
        collection.receiveLifecycle(.ready(generation: 1))
        await collection.loadCounts()
        collection.addNotePresentation.configure { _ in true }
        navigation.openDeckPage(DeckID(1))
        let context = NativeCommandContext(collection: collection, host: .main)
        try #require(context.shownDeck == 1)
        try #require(context.fileActionsAvailable && context.canNewNote && context.canNewDeck)
        try #require(context.canStudyShownDeck && context.canExportShownDeck && context.canDeleteShownDeck)
        try #require(context.canRenameShownDeck && context.canAddSubdeck && context.canOpenDeckOptions && context.canCreateFiltered)
        navigation.selectFromSidebar(.section(.browse))
        let intent = try #require(navigation.macSidebarSelectionID)
        #expect(navigation.homeDeckPage == DeckID(1))
        #expect(context.shownDeck == nil)
        #expect(!context.fileActionsAvailable && !context.canNewNote && !context.canNewDeck)
        #expect(!context.canStudyShownDeck && !context.canExportShownDeck && !context.canDeleteShownDeck)
        #expect(!context.canRenameShownDeck && !context.canAddSubdeck && !context.canOpenDeckOptions && !context.canCreateFiltered)
        #expect(!context.undoRoute(redo: false).enabled)
        #expect(context.browseCommands == nil && !context.canSaveNoteType)
        try #require(context.canShowBrowse && context.canShowNoteTypes)
        context.showNoteTypes()
        navigation.commitMacSidebarSelection(intent)
        #expect(navigation.section == .noteTypes)
        #expect(!navigation.sidebarInteractionPending)
        navigation.selectFromSidebar(.section(.home))
        #expect(!context.noteTypeSaveShown)
        navigation.selectFromSidebar(.deck(DeckID(2)))
        let deckIntent = try #require(navigation.macSidebarSelectionID)
        try #require(context.canShowBrowse && context.canShowNoteTypes)
        context.showBrowse()
        navigation.commitMacSidebarSelection(deckIntent)
        #expect(navigation.section == .browse)
        #expect(!navigation.sidebarInteractionPending)
        try #require(context.browseCommands != nil && context.browseInspector != nil)
        navigation.selectFromSidebar(.section(.noteTypes))
        #expect(context.browseCommands == nil && context.browseInspector == nil)
        #expect(!context.canToggleBrowseMode && !context.canEditBrowseNote && !context.canDeleteNotes)
    }
    #endif

    #if os(macOS)
    /// Commands evaluated while a press waits for its frame stop at the pending
    /// guard and read nothing of the route; the route's commit must still make
    /// them register again, or New Note, New Deck and Find stay disabled.
    @Test func commandsEvaluatedDuringAPendingPressRegisterAgainWhenItCommits() async throws {
        var configuration = LaunchConfiguration()
        configuration.deviceDefaults = nil
        let navigation = AppNavigation()
        let collection = ReviewCollection(configuration: configuration, navigation: navigation)
        collection.loadDeckTree = {
            CountedDeckTree(generation: 1, nextChangeAt: Int64.max, nodes: [
                CountedDeckNode(id: 1, name: "A", filtered: false, studyCollapsed: false,
                    browserCollapsed: false, counts: ReviewCounts(new: 1, learning: 0, review: 0), children: []),
            ])
        }
        collection.receiveLifecycle(.ready(generation: 1))
        await collection.loadCounts()
        collection.addNotePresentation.configure { _ in true }
        navigation.openDeckPage(DeckID(1))
        let context = NativeCommandContext(collection: collection, host: .main)
        navigation.selectFromSidebar(.section(.stats))
        let intent = try #require(navigation.macSidebarSelectionID)
        let changed = CommandObservation()
        withObservationTracking {
            let registered = context.observingReviewAvailability()
            _ = (registered.canNewNote, registered.canNewDeck, registered.canFind, registered.fileActionsAvailable)
        } onChange: { changed.fired = true }
        #expect(!context.canNewNote && !context.canNewDeck)
        navigation.commitMacSidebarSelection(intent)
        #expect(changed.fired, "the commit re-registers the commands")
        #expect(context.canNewNote && context.canNewDeck && context.fileActionsAvailable)
    }

    /// The page a pending press leaves stays enabled, so its key equivalents
    /// wait for the route; ⌘[ cancels and keys without ⌘ keep their paths.
    @Test func keyEquivalentsWaitForThePendingPressUnlessTheSidebarHasTheKeys() {
        #expect(MacSidebarPendingKeys.route(modifiers: .command, characters: "[", responderInSidebar: false) == .cancel)
        #expect(MacSidebarPendingKeys.route(modifiers: .command, characters: "[", responderInSidebar: true) == .cancel)
        #expect(MacSidebarPendingKeys.route(modifiers: .command, characters: "\u{8}", responderInSidebar: false) == .withhold)
        #expect(MacSidebarPendingKeys.route(modifiers: .command, characters: "s", responderInSidebar: false) == .withhold)
        #expect(MacSidebarPendingKeys.route(modifiers: [.command, .shift], characters: "[", responderInSidebar: false) == .withhold)
        #expect(MacSidebarPendingKeys.route(modifiers: .command, characters: "s", responderInSidebar: true) == .deliver)
        #expect(MacSidebarPendingKeys.route(modifiers: [], characters: "\r", responderInSidebar: false) == .deliver)
        #expect(MacSidebarPendingKeys.route(modifiers: .option, characters: "s", responderInSidebar: false) == .deliver)
    }
    #endif

    @Test func pendingDeckDoesNotExposeCommittedDeckOrBrowseCommands() async {
        var configuration = LaunchConfiguration()
        configuration.deviceDefaults = nil
        let navigation = AppNavigation()
        let collection = ReviewCollection(configuration: configuration, navigation: navigation)
        let counts = ReviewCounts(new: 1, learning: 0, review: 0)
        collection.loadDeckTree = {
            CountedDeckTree(generation: 1, nextChangeAt: Int64.max, nodes: [
                CountedDeckNode(id: 1, name: "A", filtered: false, studyCollapsed: false,
                    browserCollapsed: false, counts: counts, children: []),
                CountedDeckNode(id: 2, name: "B", filtered: false, studyCollapsed: false,
                    browserCollapsed: false, counts: counts, children: []),
            ])
        }
        collection.receiveLifecycle(.ready(generation: 1))
        await collection.loadCounts()
        navigation.openDeckPage(DeckID(1))
        let context = NativeCommandContext(collection: collection, host: .main)
        #expect(context.shownDeck == 1)
        let intent = navigation.beginDeckSelection(deck: DeckID(2))
        #expect(context.shownDeck == nil)
        #expect(!context.canStudyShownDeck && !context.canExportShownDeck && !context.canDeleteShownDeck)
        #expect(!context.canRenameShownDeck && !context.canAddSubdeck && !context.canOpenDeckOptions)
        #expect(context.fileActionsAvailable)
        #expect(context.canShowBrowse && context.canShowNoteTypes)
        context.showBrowse()
        #expect(navigation.pendingDeckPage == nil)
        #expect(navigation.section == .browse)
        // A completion for the abandoned request cannot affect the new section.
        navigation.finishDeckSelection(intent: intent)
        #expect(navigation.section == .browse)
        _ = navigation.beginDeckSelection(deck: DeckID(2))
        #expect(context.browseCommands == nil)
        #expect(context.browseInspector == nil)
        #expect(!context.canToggleBrowseMode && !context.canEditBrowseNote && !context.canDeleteNotes)
        #expect(context.canShowBrowse && context.canShowNoteTypes)
        context.showNoteTypes()
        #expect(navigation.pendingDeckPage == nil)
        #expect(navigation.section == .noteTypes)
    }
}

/// What a `withObservationTracking` change handler saw: it is a Sendable closure.
private nonisolated final class CommandObservation: @unchecked Sendable {
    var fired = false
}
