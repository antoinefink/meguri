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
    var customStudy: (() -> Void)? = nil
    @Environment(\.locale) private var locale

    @State private var finished = false
    @FocusState private var continueFocused: Bool
    @FocusState private var customStudyFocused: Bool
    #if !os(macOS)
    @FocusState private var keyboardFocused: Bool
    #endif
    private var showsDone: Bool { !waiting || continueAvailable }
    private func finish() {
        guard canFinish, showsDone, !finished else { return }
        finished = true
        close()
    }

    var body: some View {
        content
            #if os(macOS)
            .background(ReviewTerminalKeyboard(enabled: canFinish && showsDone,
                continueFocused: continueFocused, customStudyFocused: customStudyFocused, done: finish))
            #else
            .focusable()
            .focused($keyboardFocused)
            .onAppear { keyboardFocused = true }
            .onKeyPress(keys: [.space, .return, KeyEquivalent("\u{3}")], phases: [.down, .repeat]) { press in
                guard press.modifiers.isEmpty, canFinish, showsDone else { return .ignored }
                if press.phase == .down, customStudyFocused || (press.key == .space && continueFocused) { return .ignored }
                if press.phase == .down { finish() }
                return .handled
            }
            #endif
    }
    @ViewBuilder private var content: some View {
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
                // One element with its own label: a combined element's text is
                // a label on touch but a value on the Mac.
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("\(Text(summary.cards, format: .number)), \(Self.caption(summary, locale: locale))"))
                .reviewStateHeading()
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
                .focused($continueFocused)
                .disabled(!canContinue)
                .accessibilityIdentifier("review.session.continue")
                .reviewStateTarget()
        }
        if !waiting, let customStudy {
            Button(action: customStudy) { Text("Custom Study…", bundle: .module).reviewStateLabel() }
                .focused($customStudyFocused)
                .disabled(!canFinish)
                .accessibilityIdentifier("review.session.customStudy")
                .reviewStateTarget()
        }
        Button(action: finish) { Text("Done", bundle: .module).reviewStateLabel() }
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
