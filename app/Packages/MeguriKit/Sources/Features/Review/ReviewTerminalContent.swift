import AppModel
import DesignSystem
import SwiftUI

/// Shared by the live session and fixed terminal previews. Readiness is supplied
/// exclusively by the applied head; optional committed summary facts do not own it.
///
/// N16.5: a session that ends says how many cards and how long, then Done;
/// Keep Going joins Done only while learning cards are ready again. A session
/// still waiting on a learning card says so and offers Check Now.
struct ReviewTerminalContent: View {
    let waiting: Bool
    let summary: ReviewSessionSummary
    let continueAvailable: Bool
    let canContinue: Bool
    let canRefresh: Bool
    let canFinish: Bool
    let refresh: () -> Void
    let continueReview: () -> Void
    let close: () -> Void
    @Environment(\.locale) private var locale

    var body: some View {
        if waiting && !continueAvailable {
            ReviewStateMessage(title: Text("Waiting for Learning", bundle: .module),
                message: Text("Your next learning card is not ready yet.", bundle: .module)) {
                Button(action: refresh) { Text("Check Now", bundle: .module).reviewStateLabel() }
                    .disabled(!canRefresh)
                    .accessibilityIdentifier("review.session.refresh")
                    .reviewStateTarget()
            }
        } else if summary.cards == 0 {
            // Nothing was answered, so there is no number to state.
            ReviewStateMessage(title: Text("Finished for Now", bundle: .module)) { actions }
        } else {
            VStack(spacing: Spacing.large) {
                VStack(spacing: Spacing.xxSmall) {
                    Text(summary.cards, format: .number)
                        .font(.largeTitle.bold())
                        .foregroundStyle(.token(.textPrimary))
                    Self.caption(summary, locale: locale)
                        .foregroundStyle(.token(.textSecondary))
                }
                .multilineTextAlignment(.center)
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("review.session.summary")
                ReviewStateActions { actions }
            }
            .padding()
            .frame(maxWidth: .infinity)
        }
    }
    @ViewBuilder private var actions: some View {
        if continueAvailable {
            Button(action: continueReview) { Text("review.congrats.continue", bundle: .module).reviewStateLabel() }
                .disabled(!canContinue)
                .accessibilityIdentifier("review.session.continue")
                .reviewStateTarget()
        }
        Button(action: close) { Text("Done", bundle: .module).reviewStateLabel() }
            .disabled(!canFinish)
            .accessibilityIdentifier("review.session.done")
            .reviewStateTarget()
    }
    /// "cards in 12 minutes" under the number: minutes from one minute up, seconds before.
    static func caption(_ summary: ReviewSessionSummary, locale: Locale) -> Text {
        let units: Set<Duration.UnitsFormatStyle.Unit> = summary.seconds < 60 ? [.seconds] : [.hours, .minutes]
        let time = summary.duration.formatted(.units(allowed: units, width: .wide).locale(locale))
        return summary.cards == 1 ? Text("card in \(time)", bundle: .module) : Text("cards in \(time)", bundle: .module)
    }
}
