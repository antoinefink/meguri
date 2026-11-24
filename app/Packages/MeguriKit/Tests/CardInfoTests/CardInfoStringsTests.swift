@testable import CardInfo
import CoreBridge
import Foundation
import Testing

/// Card Info's labels (`stats/card-info` C12): every C4.1 row key has its
/// catalog label; the unavailable view's texts by C11 kind; a history entry
/// reads as one element, each column's label then its value.
@MainActor @Suite struct CardInfoStringsTests {
    private let english = Locale(identifier: "en_US")

    @Test func everyRowKeyHasItsLabel() {
        let labels = ["added": "Added", "first_review": "First Review", "latest_review": "Latest Review", "due": "Due",
                      "position": "Position", "interval": "Interval", "stability": "Stability", "difficulty": "Difficulty",
                      "retrievability": "Retrievability", "reviews": "Reviews", "lapses": "Lapses", "average_time": "Average Time",
                      "total_time": "Total Time", "card_type": "Card Type", "note_type": "Note Type", "deck": "Deck",
                      "preset": "Preset", "card_id": "Card ID", "note_id": "Note ID", "custom_data": "Custom Data"]
        for (key, label) in labels { #expect(CardInfoStrings.label(key, locale: english) == label, "\(key)") }
        #expect(CardInfoStrings.label("ease", locale: english) == "ease", "no Ease row, R1250: an unknown key shows as it is")
    }

    @Test func theUnavailableViewSaysWhyAndPointsToTheRepair() {
        #expect(CardInfoStrings.unavailable(.notFound, locale: english) == ("(No card to display.)", nil))
        let note = CardInfoStrings.unavailable(.missingNote, locale: english)
        #expect(note.title == "Card Info Unavailable" && note.text == "This card's note is missing. Check Database can repair it.")
        let cutoff = CardInfoStrings.unavailable(.invalidCutoff, locale: english)
        #expect(cutoff.text?.hasSuffix("Change it in Deck Options.") == true && cutoff.text?.contains("Check Database") == false)
        #expect(CardInfoStrings.unavailable(nil, locale: english).text == "Some of this card's records cannot be read.")
    }

    @Test func aHistoryEntryReadsItsLabelsAndValues() {
        let entry = CardInfoReview(id: 1, kind: .review, rating: 3, date: "2030-01-10", dateTime: "2030-01-10 @ 09:00",
                                   kindText: "Review", ratingText: "3", interval: "15 days", ease: "D:74%", time: "3s",
                                   stability: "")
        #expect(CardInfoStrings.historyLabel(entry, locale: english)
            == "Date, 2030-01-10; Type, Review; Rating, 3; Interval, 15 days; Time, 3s")
        #expect(CardInfoStrings.range(.firstMonth, locale: english) == "First Month")
    }
}
