import SwiftUI

/// The Knowledge section's boxes: Knowledge Over Time, Retention and Forgotten (S7.1 to S7.3). Its unit (U297) puts each box's own view here; until
/// then the page shows the generic box drawn from the model's content.
enum StatsKnowledgeSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsGenericBox(slot: slot)
    }
}
