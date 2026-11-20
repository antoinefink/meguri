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

    @Test func theRowsAreCentredOnTheirHalfOnlyWhileTheListCannotScroll() {
        // Three rows 277 points tall in a half 600 tall, under a system inset of 35.
        #expect(WelcomeArrangement.centredMargin(rowsHeight: 277, inset: 35, height: 600, followed: false, clearance: 48) == 126.5)
        // The margin never goes below none.
        #expect(WelcomeArrangement.centredMargin(rowsHeight: 277, inset: 200, height: 600, followed: false, clearance: 48) == 0)
        // An error or the previous session follows the rows: the list may scroll, so it starts at the top.
        #expect(WelcomeArrangement.centredMargin(rowsHeight: 277, inset: 35, height: 600, followed: true, clearance: 48) == nil)
        // Too tall to centre without scrolling, and nothing measured yet.
        #expect(WelcomeArrangement.centredMargin(rowsHeight: 520, inset: 35, height: 600, followed: false, clearance: 48) == nil)
        #expect(WelcomeArrangement.centredMargin(rowsHeight: nil, inset: 35, height: 600, followed: false) == nil)
    }

    @Test func aShortHalfOrAnAccessibilityTextSizeKeepsTheColumn() {
        // A large iPhone in landscape is regular width but too short for the heading beside the rows.
        #expect(WelcomeArrangement.resolve(phone: true, regularWidth: true, size: CGSize(width: 890, height: 330)) == .column)
        #expect(WelcomeArrangement.resolve(phone: true, regularWidth: true, size: CGSize(width: 951, height: 669), accessibilitySize: true) == .column)
        #expect(WelcomeArrangement.resolve(phone: true, regularWidth: true, size: CGSize(width: 951, height: 669)) == .sides)
    }
}
