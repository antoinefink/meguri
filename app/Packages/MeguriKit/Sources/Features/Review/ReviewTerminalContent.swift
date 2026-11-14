import DesignSystem
import SwiftUI

/// Shared by the live session and fixed terminal previews. Readiness is supplied
/// exclusively by the applied head; optional committed summary facts do not own it.
struct ReviewTerminalContent: View {
    let waiting: Bool
    let continueAvailable: Bool
    let canContinue: Bool
    let canRefresh: Bool
    let canFinish: Bool
    let refresh: () -> Void
    let continueReview: () -> Void
    let close: () -> Void

    var body: some View {
        EmptyStateView(title, systemImage: waiting && !continueAvailable ? "clock" : "checkmark.circle",
            description: description) {
                if continueAvailable {
                    Button(action: continueReview) { Text("review.congrats.continue", bundle: .module) }
                        .disabled(!canContinue)
                        .accessibilityIdentifier("review.session.continue")
                } else if waiting {
                    Button(action: refresh) { Text("Check Now", bundle: .module) }
                        .disabled(!canRefresh)
                        .accessibilityIdentifier("review.session.refresh")
                }
                if !waiting {
                    Button(action: close) { Text("Done", bundle: .module) }
                        .disabled(!canFinish)
                        .accessibilityIdentifier("review.session.done")
                }
            }
    }
    private var title: Text {
        waiting && !continueAvailable
            ? Text("Waiting for Learning", bundle: .module)
            : Text("Finished for Now", bundle: .module)
    }
    private var description: Text {
        waiting && !continueAvailable
            ? Text("Your next learning card is not ready yet.", bundle: .module)
            : Text("Your answers have been recorded. You can go back to your last answer or close the session.", bundle: .module)
    }
}
