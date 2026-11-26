import CoreBridge
import Foundation
import Testing
@testable import AppModel
@testable import Meguri

/// Model-only command admission regressions: no window, app activation or Core calls.
@MainActor @Suite struct NativeCommandPendingTests {
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
