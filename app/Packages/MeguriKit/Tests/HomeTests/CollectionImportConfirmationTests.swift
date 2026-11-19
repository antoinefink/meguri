import Testing
@testable import CoreBridge
@testable import Home

/// N17.2: one confirmation, in plain words.
@MainActor @Suite struct CollectionImportConfirmationTests {
    @Test func titleNamesTheFile() {
        #expect(CollectionEntryPresentation(fileName: "Japanese.colpkg").confirmationTitle == "Import “Japanese.colpkg”?")
        #expect(CollectionEntryPresentation().confirmationTitle == "Import This Collection?")
    }

    @Test func summaryCountsNotesCardsAndMediaFilesWithPlurals() {
        let one = CollectionEntryPresentation.Facts(notes: 1, cards: 1, reviews: 7, replacingExisting: false, mediaCount: 1)
        #expect(CollectionEntryPresentation.summary(one) == "1 note · 1 card · 1 media file")
        let many = CollectionEntryPresentation.Facts(notes: 6, cards: 11, reviews: 7, replacingExisting: true, mediaCount: 0)
        #expect(CollectionEntryPresentation.summary(many) == "6 notes · 11 cards · 0 media files")
        #expect(CollectionEntryPresentation.count(7, "review", "reviews") == "7 reviews")
    }

    @Test func consequenceShowsOnlyWhenACollectionIsReplaced() throws {
        let replacing = CollectionEntryPresentation.Facts(notes: 6, cards: 11, reviews: 7, replacingExisting: true)
        #expect(CollectionEntryPresentation.consequence(replacing, device: "iPhone")
            == "This replaces the collection on this iPhone. Meguri keeps a backup of the current one.")
        let first = CollectionEntryPresentation.Facts(notes: 6, cards: 11, reviews: 7, replacingExisting: false)
        #expect(CollectionEntryPresentation.consequence(first, device: "iPhone") == nil)
        // package/container P16.4: the media size and what happens to media already here.
        #expect(CollectionEntryPresentation.mediaNote(first, device: "Mac") == nil)
        let media = CollectionEntryPresentation.Facts(notes: 6, cards: 11, reviews: 7, replacingExisting: false, mediaCount: 3, mediaDecodedBytes: 2_000_000)
        let note = try #require(CollectionEntryPresentation.mediaNote(media, device: "Mac"))
        #expect(note.contains("merged") && note.contains("kept") && note.contains("Mac"))
    }

    @Test func retainedDetailsStayOutOfTheOrdinaryFlow() {
        let plan = CollectionImportMedia(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1)
        let facts = CollectionEntryPresentation.Facts(notes: 6, cards: 11, reviews: 7, replacingExisting: true)
        #expect(!CollectionEntryPresentation(facts: facts).showsRetainedDetails)
        #expect(!CollectionEntryPresentation(message: "Import cancelled.", preparedMedia: plan).showsRetainedDetails)
        #expect(!CollectionEntryPresentation(progress: .init(title: "Importing…", completed: 1, total: nil, canCancel: true),
                                             preparedMedia: plan).showsRetainedDetails)
        #expect(CollectionEntryPresentation(message: "Recovery", requiresRecovery: true, mediaReference: "r6").showsRetainedDetails)
    }
}
