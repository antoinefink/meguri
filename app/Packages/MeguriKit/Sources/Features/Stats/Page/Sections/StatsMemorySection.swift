import SwiftUI

/// The Memory section's boxes: Retrievability, Stability and Difficulty (S7.12 to S7.14). Its unit (U301) puts each box's own view here; until
/// then the page shows the generic box drawn from the model's content.
enum StatsMemorySection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsGenericBox(slot: slot)
    }
}
