import SwiftUI

/// The Answering section's boxes: Answers and Time of Day (S7.10, S7.11). Its unit (U300) puts each box's own view here; until
/// then the page shows the generic box drawn from the model's content.
enum StatsAnsweringSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsGenericBox(slot: slot)
    }
}
