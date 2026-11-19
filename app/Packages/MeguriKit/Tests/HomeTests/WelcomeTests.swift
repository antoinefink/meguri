import CoreGraphics
import Testing
@testable import Home

@Suite struct WelcomeTests {
    @Test func listsSyncThenImportThenStartEmptyUnderTheSurvivingIdentifiers() {
        #expect(WelcomeChoice.allCases == [.sync, .importFile, .startEmpty])
        #expect(WelcomeChoice.allCases.map(\.identifier)
            == ["home.collection.accountSync", "home.collection.import", "home.collection.startEmpty"])
        #expect(WelcomeChoice.allCases.map(\.title) == ["Set Up Sync", "Import a Collection File", "Start Empty"])
        #expect(WelcomeChoice.allCases.map(\.explanation) == [
            "Get your collection from Meguri Sync or iCloud.",
            "Open a .colpkg file exported from Meguri or Anki.",
            "Begin with no decks and add your own notes.",
        ])
    }

    @Test func onlyAnIPhoneAtRegularWidthHeldWideSetsTheTwoSidesApart() {
        let wide = CGSize(width: 951, height: 669), tall = CGSize(width: 669, height: 951)
        #expect(WelcomeArrangement.resolve(phone: true, regularWidth: true, size: wide) == .sides)
        // Held tall, folded, an ordinary iPhone in landscape, and an iPad or a Mac at any size.
        #expect(WelcomeArrangement.resolve(phone: true, regularWidth: true, size: tall) == .column)
        #expect(WelcomeArrangement.resolve(phone: true, regularWidth: false, size: CGSize(width: 466, height: 678)) == .column)
        #expect(WelcomeArrangement.resolve(phone: true, regularWidth: false, size: CGSize(width: 874, height: 402)) == .column)
        #expect(WelcomeArrangement.resolve(phone: false, regularWidth: true, size: wide) == .column)
    }

    @Test func theColumnStaysReadableAndCentredInAWiderPane() {
        #expect(WelcomeArrangement.columnMargin(width: 402, readable: 640) == nil)
        #expect(WelcomeArrangement.columnMargin(width: 640, readable: 640) == nil)
        #expect(WelcomeArrangement.columnMargin(width: 1000, readable: 640) == 180)
    }

    @Test func theRowsAreCentredOnTheirHalfFromWhereTheyWereMeasured() {
        // No margin yet, rows measured at 113...205 in a half visible from 34 to 381.
        let first = WelcomeArrangement.centredOffset(current: nil, rows: 113...205, visible: 34...381, clearance: 48)
        #expect(first == 48.5)
        // Measured again in the middle, the margin stays.
        #expect(WelcomeArrangement.centredOffset(current: 48.5, rows: 161.5...253.5, visible: 34...381, clearance: 48) == 48.5)
        // A system inset the first step did not know about is corrected by the next.
        #expect(WelcomeArrangement.centredOffset(current: 48.5, rows: 141.5...233.5, visible: 34...381, clearance: 48) == 68.5)
        // Too tall to centre without scrolling: the system's margin, from the top.
        #expect(WelcomeArrangement.centredOffset(current: 20, rows: 113...390, visible: 34...381, clearance: 48) == nil)
        // Nothing measured yet.
        #expect(WelcomeArrangement.centredOffset(current: nil, rows: nil, visible: 34...381) == nil)
        #expect(WelcomeArrangement.rowsSpan([CGRect(x: 0, y: 10, width: 5, height: 20), CGRect(x: 0, y: 30, width: 5, height: 20)]) == 10...50)
        #expect(WelcomeArrangement.rowsSpan([CGRect]()) == nil)
    }
}
