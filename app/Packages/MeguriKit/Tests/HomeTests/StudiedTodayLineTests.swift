#if os(macOS)
import AppKit
import SwiftUI
import Testing
@testable import Home

/// N24.2's Studied Today row where it does not stack: the label and the value wrap before
/// the row would run past its width (the design brief's wrap-before-truncate rule).
@MainActor @Suite struct StudiedTodayLineTests {
    private func size(_ value: String, width: CGFloat) -> CGSize {
        NSHostingController(rootView: StudiedTodayLine(value: value, fraction: 0.4))
            .sizeThatFits(in: CGSize(width: width, height: .greatestFiniteMagnitude))
    }

    @Test(arguments: [90.0, 140.0, 200.0])
    func neverRunsPastTheRow(width: Double) {
        let long = "1,234 cards · 1 hr, 23 min"
        #expect(size(long, width: width).width <= width)
    }

    @Test func oneLineWhileEverythingFits() {
        let wide = size("12 cards · 3 min", width: 600), narrow = size("12 cards · 3 min", width: 140)
        #expect(wide.width <= 600 && wide.height < narrow.height)
    }
}
#endif
