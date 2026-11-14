import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Fixed samples of the production timer label.
/// These are component fixtures, not placement or live-progression evidence.
public struct ReviewTimerPreview: View {
    public nonisolated enum State: String, CaseIterable, Sendable { case running, frozen, capped }
    public let state: State
    public init(state: State) { self.state = state }
    public var body: some View {
        ReviewTimerLabel(presentation: ReviewTimerPresentation(milliseconds: state == .capped ? 60_000 : 12_000,
                // Every fixture is a static sample, including the running
                // question-side appearance; none advances an answer clock.
                capSeconds: 60, isVisible: true, isCounting: false))
    }
}

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
        ReviewLayout(progress: waiting || state == .finishedWithNewWork ? 0.9 : 1,
            remaining: state == .finishedWithNewWork ? "0 new, 0 learning, 1 to review" :
                (waiting ? "0 new, 1 learning, 0 to review" : "0 new, 0 learning, 0 to review"),
            notice: nil, undoDescription: "Undo Answer Card", canUndo: true, canRedo: false,
            phase: waiting ? .waiting : .finished, states: nil, enabled: true,
            close: {}, undo: {}, redo: {}, reveal: {}, rate: { _ in }) {
                ReviewStatusLayout {
                    ReviewTerminalContent(waiting: waiting, continueAvailable: ready, canContinue: ready,
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
        ReviewLayout(progress: 0.4, remaining: "4 new, 2 learning, 12 to review",
            notice: state == .error ? "Couldn’t save the answer. Please answer this card again." : nil,
            undoDescription: "Undo Answer Card", canUndo: true, canRedo: false,
            phase: state == .loading ? .loading : (answer ? .answer : .question), states: states,
            enabled: true, close: {}, undo: {}, redo: {}, reveal: {}, rate: { _ in }) {
                if state == .loading {
                    ReviewStatusLayout { LoadingStateView(Text(verbatim: "Preparing review…")) }
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
