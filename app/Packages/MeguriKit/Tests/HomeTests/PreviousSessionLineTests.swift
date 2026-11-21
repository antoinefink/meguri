#if os(macOS)
import AppKit
import SwiftUI
import Testing
@testable import Home

/// N24.2's Previous Session row: the value goes under the label when both do not fit on one
/// line, and every text wraps before the row would run past its width.
@MainActor @Suite struct PreviousSessionLineTests {
    private func size(decks: String?, _ value: String, width: CGFloat) -> CGSize {
        NSHostingController(rootView: PreviousSessionLine(decks: decks, value: value, stacks: false))
            .sizeThatFits(in: CGSize(width: width, height: .greatestFiniteMagnitude))
    }

    @Test(arguments: [90.0, 140.0, 200.0])
    func neverRunsPastTheRow(width: Double) {
        let value = "1,234 cards · Yesterday at 11:58 PM"
        #expect(size(decks: "Languages, Science, and Culture", value, width: width).width <= width)
        #expect(size(decks: nil, value, width: width).width <= width)
    }

    @Test func besideTheLabelWhileBothFit() {
        let wide = size(decks: "Vocabulary in Español", "46 cards · 8:12 AM", width: 600)
        let narrow = size(decks: "Vocabulary in Español", "46 cards · 8:12 AM", width: 160)
        #expect(wide.width <= 600 && wide.height < narrow.height)
    }
}
#endif
