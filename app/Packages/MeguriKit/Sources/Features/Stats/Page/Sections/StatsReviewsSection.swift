import SwiftUI

/// The Reviews section's boxes: Reviews Over Time and Activity (S7.4, S7.5). Its unit (U298) puts each box's own view here; until
/// then the page shows the generic box drawn from the model's content.
enum StatsReviewsSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsGenericBox(slot: slot)
    }
}
