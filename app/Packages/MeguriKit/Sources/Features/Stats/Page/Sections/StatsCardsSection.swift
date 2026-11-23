import SwiftUI

/// The Cards section's boxes: Cards Over Time, Card Counts and Added and Started (S7.6 to S7.8). Its unit (U299) puts each box's own view here; until
/// then the page shows the generic box drawn from the model's content.
enum StatsCardsSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsGenericBox(slot: slot)
    }
}
