import SwiftUI

/// The Decks section's boxes: By Deck (S7.15). Its unit (U301) puts each box's own view here; until
/// then the page shows the generic box drawn from the model's content.
enum StatsDecksSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsGenericBox(slot: slot)
    }
}
