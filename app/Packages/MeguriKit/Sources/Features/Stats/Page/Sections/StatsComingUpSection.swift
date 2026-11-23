import SwiftUI

/// The Coming Up section's boxes: Reviews Due (S7.9). Its unit (U300) puts each box's own view here; until
/// then the page shows the generic box drawn from the model's content.
enum StatsComingUpSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsGenericBox(slot: slot)
    }
}
