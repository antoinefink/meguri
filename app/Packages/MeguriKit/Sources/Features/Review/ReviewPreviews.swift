import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Fixed presentation data. Real WebKit/core evidence belongs to the shell UI
/// Preview cases exercise the toolbar, answer buttons and available width.
public enum ReviewQuestionPreview: PreviewStates {
    public static let previewID = "review.question"
    public static var previewFixtures: [PreviewFixture] {
        [PreviewState.typical, .overflow, .error, .loading].map { state in PreviewFixture(state) { ReviewFixture(state: state, answer: false) } }
    }
}
public enum ReviewAnswerPreview: PreviewStates {
    public static let previewID = "review.answer"
    public static var previewFixtures: [PreviewFixture] {
        [PreviewState.typical, .overflow, .error].map { state in PreviewFixture(state) { ReviewFixture(state: state, answer: true) } }
    }
}
public enum ReviewFinishedPreview: PreviewStates {
    public static let previewID = "review.congratulations"
    public nonisolated enum State: String, CaseIterable, Sendable {
        case waitingUnavailable, readyToContinue, finished, finishedWithNewWork
    }
    public static var previewFixtures: [PreviewFixture] {
        [PreviewFixture(.empty) { preview(.waitingUnavailable) },
         PreviewFixture(.typical) { preview(State.finished) }]
    }
    /// Static production-content fixtures; no Core session or media is exercised.
    public static func preview(_ state: State) -> some View {
        ReviewFinishedFixture(state: state)
    }
}
private struct ReviewFinishedFixture: View {
    let state: ReviewFinishedPreview.State
    private var waiting: Bool { state == .waitingUnavailable || state == .readyToContinue }
    private var ready: Bool { state == .readyToContinue || state == .finishedWithNewWork }
    var body: some View {
        ReviewLayout(progress: ReviewSessionProgress(answered: 9, remaining: waiting || state == .finishedWithNewWork ? 1 : 0),
            notice: nil, undoDescription: "Undo Answer", canUndo: true, canRedo: false,
            phase: waiting ? .waiting : .finished, states: nil, enabled: true,
            close: {}, undo: {}, redo: {}, reveal: {}, rate: { _ in }) {
                ReviewStatusLayout {
                    ReviewTerminalContent(waiting: waiting, summary: ReviewSessionSummary(cards: 42, seconds: 12 * 60),
                        continueAvailable: ready, canContinue: ready,
                        canRefresh: true, canFinish: true, refresh: {}, continueReview: {}, close: {})
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(.token(.backgroundPrimary))
    }
}
private struct ReviewFixture: View {
    let state: PreviewState
    let answer: Bool
    var body: some View {
        ReviewLayout(progress: ReviewSessionProgress(answered: 12, remaining: 18),
            notice: state == .error ? "Couldn’t save the answer. Please answer this card again." : nil,
            undoDescription: "Undo Answer", canUndo: true, canRedo: false,
            phase: state == .loading ? .loading : (answer ? .answer : .question), states: states,
            enabled: true, close: {}, undo: {}, redo: {}, reveal: {}, rate: { _ in },
            actions: ReviewMoreActions(editNoteEnabled: { state != .loading }, cardActionsEnabled: { state != .loading })) {
                if state == .loading {
                    ReviewStatusLayout { ProgressView() }
                }
                else {
                    ScrollView {
                        Text(verbatim: state == .overflow ? String(repeating: "A longer question with an answer that wraps across the card. ", count: 12) : (answer ? "Answer" : "Question"))
                            .font(.title2).padding(Spacing.large)
                    }
                }
            }
    }
    private var states: ReviewSchedulingStates {
        func answer(_ amount: String, unit: ReviewTimeUnit, less: Bool = false) -> ReviewAnswerState {
            ReviewAnswerState(state: .new(position: 0), delay: .days(amount: 1), left: 0, factor: 0,
                revlog: ReviewRevlogPreview(interval: 1, lastInterval: 0, kind: 0, factor: 0),
                label: ReviewAnswerLabel(lessThan: less, amount: amount, unit: unit))
        }
        return ReviewSchedulingStates(current: .new(position: 0), again: answer("10", unit: .minutes, less: true),
            hard: answer("1", unit: .days), good: answer("4", unit: .days), easy: answer(state == .overflow ? "136.2" : "8", unit: .days))
    }
}
