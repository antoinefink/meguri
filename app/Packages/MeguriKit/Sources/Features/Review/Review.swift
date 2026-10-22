import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

public struct ReviewView: View {
    public let model: ReviewCoordinator
    public let close: () -> Void
    public let visibilityChanged: (Bool) -> Void
    @Environment(\.colorScheme) private var colorScheme
    public init(model: ReviewCoordinator, visibilityChanged: @escaping (Bool) -> Void = { _ in }, close: @escaping () -> Void) {
        self.model = model; self.visibilityChanged = visibilityChanged; self.close = close
    }
    public var body: some View {
        ReviewLayout(progress: model.progress, remaining: model.remainingDescription, notice: model.notice,
            undoDescription: undoDescription, canUndo: model.canUndo, canRedo: model.canRedo,
            phase: model.phase, states: model.item?.nextStates, enabled: !model.paused,
            close: close, undo: { model.restore() }, redo: { model.restore(redo: true) },
            reveal: model.reveal, rate: model.rate,
            timerText: model.item?.homePreset.showTimer == true ? { model.timerText } : nil) {
            ZStack {
                if let pair = model.pair {
                    ReviewCardHost(model: model, pair: pair, close: close)
                        .accessibilityIdentifier("review.session.card")
                        .accessibilityAction(named: Text("Show Answer", bundle: .module)) { model.reveal() }
                        #if os(iOS)
                        .accessibilityAction(.magicTap) { model.reveal() }
                        #endif
                        .opacity(model.phase == .question || model.phase == .answer || model.phase == .transition ? 1 : 0)
                }
                stateContent
            }
        }
        .onAppear { visibilityChanged(true) }
        .onDisappear { visibilityChanged(false) }
        .task { await appearance() }
        .onChange(of: colorScheme) { _, _ in Task { await appearance() } }
    }
    @ViewBuilder private var stateContent: some View {
        switch model.phase {
        case .loading: LoadingStateView(Text("Preparing review…", bundle: .module))
        case .waiting:
            EmptyStateView(Text("Waiting for Learning", bundle: .module), systemImage: "clock",
                description: Text("Your next learning card is not ready yet.", bundle: .module)) {
                    Button { model.retry() } label: { Text("Check Now", bundle: .module) }.accessibilityIdentifier("review.session.refresh")
                }
        case .finished:
            EmptyStateView(Text("Finished for Now", bundle: .module), systemImage: "checkmark.circle",
                description: Text("Your answers have been recorded. You can go back to your last answer or close the session.", bundle: .module)) {
                    Button(action: close) { Text("Done", bundle: .module) }.accessibilityIdentifier("review.session.done")
                }
        case .error:
            ErrorStateView(Text("Review Paused", bundle: .module), message: Text(verbatim: model.notice ?? ""),
                retryIdentifier: "review.session.retry", retry: model.retry)
        case .ended: Text("Session Ended", bundle: .module)
        case .transition:
            if model.item == nil { ProgressView() }
        case .question, .answer: EmptyView()
        }
    }
    private var undoDescription: String {
        switch model.undoStatus {
        case .available(let step): "Undo \(step.name)"
        case .unavailable(let reason):
            switch reason {
            case .none: String(localized: "No answer to undo", bundle: .module)
            case .clearedByRollover: String(localized: "Undo cleared by a new study day", bundle: .module)
            case .clearedBySync, .clearedByOneWay: String(localized: "Undo cleared by sync", bundle: .module)
            case .alreadyUploaded, .uploadPending: String(localized: "This answer has already been shared", bundle: .module)
            case .changedExternally, .clearedByNonUndoableOp: String(localized: "Undo cleared by a collection change", bundle: .module)
            }
        }
    }
    private func appearance() async { try? await model.pair?.setAppearance(colorScheme == .dark ? .dark : .light) }
}


struct ReviewLayout<Card: View>: View {
    let progress: Double
    let remaining: String
    let notice: String?
    let undoDescription: String
    let canUndo: Bool
    let canRedo: Bool
    let phase: ReviewCoordinator.Phase
    let states: ReviewSchedulingStates?
    let enabled: Bool
    let close: () -> Void
    let undo: () -> Void
    let redo: () -> Void
    let reveal: () -> Void
    let rate: (ReviewRating) -> Void
    var timerText: (() -> String)? = nil
    @Environment(\.appClock) var clock
    @ViewBuilder let card: () -> Card
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: Spacing.small) {
                Button(action: close) { Image(systemName: "xmark").frame(minWidth: 44, minHeight: 44) }
                    .accessibilityLabel(Text("Close Review", bundle: .module))
                    .accessibilityIdentifier("review.session.close")
                ProgressView(value: progress)
                    .tint(.token(.progressFill))
                    .accessibilityLabel(Text("Session Progress", bundle: .module))
                    .accessibilityValue(remaining)
                    .help(remaining)
                if let timerText {
                    TimelineView(.periodic(from: clock.now(), by: 1)) { _ in
                        Text(verbatim: timerText()).monospacedDigit().font(.caption)
                            .accessibilityIdentifier("review.session.timer")
                    }
                }
                Button { undo() } label: { Image(systemName: "arrow.uturn.backward").frame(minWidth: 44, minHeight: 44) }
                    .disabled(!canUndo)
                    .accessibilityLabel(undoDescription)
                    .help(undoDescription)
                    .accessibilityIdentifier("review.session.back")
                if canRedo {
                    Button { redo() } label: { Image(systemName: "arrow.uturn.forward").frame(minWidth: 44, minHeight: 44) }
                        .accessibilityLabel(Text("Redo Answer", bundle: .module))
                        .accessibilityIdentifier("review.session.redo")
                }
            }
            .buttonStyle(.plain)
            .padding(.horizontal, Spacing.small)
            if let notice = notice {
                Text(verbatim: notice).font(.callout).foregroundStyle(.token(.statusWarning))
                    .padding(Spacing.small)
                    .accessibilityIdentifier("review.session.notice")
            }
            card().frame(maxWidth: .infinity, maxHeight: .infinity)
            ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate)
        }
        .background(.token(.backgroundPrimary))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("review.session.screen")
    }
}

/// One reserved bottom region on both sides; controls never resize the card.
public struct ReviewAnswerBar: View {
    public let phase: ReviewCoordinator.Phase
    public let states: ReviewSchedulingStates?
    public let enabled: Bool
    public let reveal: () -> Void
    public let rate: (ReviewRating) -> Void
    public init(phase: ReviewCoordinator.Phase, states: ReviewSchedulingStates?, enabled: Bool,
                reveal: @escaping () -> Void, rate: @escaping (ReviewRating) -> Void) {
        self.phase = phase; self.states = states; self.enabled = enabled; self.reveal = reveal; self.rate = rate
    }
    public var body: some View {
        ZStack {
            // Kept in layout on both sides, including at large text sizes.
            HStack(spacing: Spacing.xSmall) {
                rating(.again, title: "Again", label: states?.again.label, color: .ratingAgain)
                rating(.hard, title: "Hard", label: states?.hard.label, color: .textPrimary)
                rating(.good, title: "Good", label: states?.good.label, color: .ratingGood)
                rating(.easy, title: "Easy", label: states?.easy.label, color: .textPrimary)
            }
            .opacity(phase == .answer ? 1 : 0)
            .allowsHitTesting(phase == .answer && enabled)
            .accessibilityHidden(phase != .answer)
            if phase == .question {
                Button(action: reveal) { Text("Show Answer", bundle: .module).frame(maxWidth: .infinity, minHeight: 44) }
                    .buttonStyle(.borderedProminent)
                    .disabled(!enabled)
                    .accessibilityIdentifier("review.session.reveal")
            }
        }
        .padding(Spacing.small)
        .frame(maxWidth: ContentWidth.readable)
        .frame(maxWidth: .infinity)
    }
    private func rating(_ rating: ReviewRating, title: String, label: ReviewAnswerLabel?, color: ColorToken) -> some View {
        Button { rate(rating) } label: {
            VStack(spacing: Spacing.xxSmall) {
                Text(verbatim: label.map(Self.interval) ?? "—").font(.headline)
                Text(LocalizedStringKey(title), bundle: .module).font(.caption)
            }
            .foregroundStyle(.token(color))
            .frame(maxWidth: .infinity, minHeight: 44)
        }
        .buttonStyle(.bordered)
        .disabled(!enabled || phase != .answer)
        .accessibilityLabel("\(title), \(label.map(Self.interval) ?? "")")
        .accessibilityIdentifier("review.session.rate.\(ReviewCoordinator.name(rating))")
    }
    public static func interval(_ label: ReviewAnswerLabel) -> String {
        let suffix: String
        switch label.unit {
        case .seconds: suffix = "s"
        case .minutes: suffix = "m"
        case .hours: suffix = "h"
        case .days: suffix = "d"
        case .months: suffix = "mo"
        case .years: suffix = "y"
        case .end: return "(end)"
        }
        return (label.lessThan ? "<" : "") + label.amount + suffix
    }
}
