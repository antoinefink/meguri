import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

public struct ReviewView: View {
    public let model: ReviewCoordinator
    public let close: () -> Void
    public let status: AnyView?
    public let visibilityChanged: (Bool) -> Void
    @Environment(\.colorScheme) private var colorScheme
    public init(model: ReviewCoordinator, status: AnyView? = nil, visibilityChanged: @escaping (Bool) -> Void = { _ in }, close: @escaping () -> Void) {
        self.status = status
        self.model = model; self.visibilityChanged = visibilityChanged; self.close = close
    }
    public var body: some View {
        ReviewLayout(progress: model.progress, remaining: model.remainingDescription, notice: model.notice,
            undoDescription: undoDescription, canUndo: model.canUndo, canRedo: model.canRedo,
            phase: model.phase, states: model.item?.nextStates, enabled: !model.paused,
            close: close, undo: { model.restore() }, redo: { model.restore(redo: true) },
            reveal: model.reveal, rate: model.rate,
            status: status, timerPresentation: { model.timerPresentation }) {
            ZStack {
                if let pair = model.pair {
                    #if os(macOS)
                    ReviewCardHost(model: model, pair: pair, close: close)
                        .opacity(model.phase == .question || model.phase == .answer || model.phase == .transition ? 1 : 0)
                    #else
                    ReviewCardHost(model: model, pair: pair, close: close)
                        .accessibilityIdentifier("review.session.card")
                        .accessibilityAction(named: Text("Show Answer", bundle: .module)) { model.reveal() }
                        .accessibilityAction(.magicTap) { model.reveal() }
                        .opacity(model.phase == .question || model.phase == .answer || model.phase == .transition ? 1 : 0)
                    #endif
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
        case .loading:
            ReviewStatusLayout { LoadingStateView(Text("Preparing review…", bundle: .module)) }
                #if DEBUG
                .debugStatusAccessibilityValue(model.debugSessionStatus)
                #endif
        case .waiting:
            ReviewStatusLayout {
                EmptyStateView(Text("Waiting for Learning", bundle: .module), systemImage: "clock",
                    description: Text("Your next learning card is not ready yet.", bundle: .module)) {
                        Button { model.retry() } label: { Text("Check Now", bundle: .module) }.accessibilityIdentifier("review.session.refresh")
                            #if os(macOS)
                            .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                            .contentShape(Rectangle())
                            #endif
                    }
            }
            #if DEBUG
            .debugStatusAccessibilityValue(model.debugSessionStatus)
            #endif
        case .finished:
            ReviewStatusLayout {
                EmptyStateView(Text("Finished for Now", bundle: .module), systemImage: "checkmark.circle",
                    description: Text("Your answers have been recorded. You can go back to your last answer or close the session.", bundle: .module)) {
                        Button(action: close) { Text("Done", bundle: .module) }.accessibilityIdentifier("review.session.done")
                            #if os(macOS)
                            .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                            .contentShape(Rectangle())
                            #endif
                    }
            }
            #if DEBUG
            .debugStatusAccessibilityValue(model.debugSessionStatus)
            #endif
        case .error:
            ReviewStatusLayout {
                ErrorStateView(Text("Review Paused", bundle: .module), message: Text(verbatim: model.notice ?? ""),
                    retryIdentifier: "review.session.retry", retry: model.retry)
            }
            #if DEBUG
            .debugStatusAccessibilityValue(model.debugSessionStatus)
            #endif
        case .ended:
            ReviewStatusLayout { Text("Session Ended", bundle: .module) }
                #if DEBUG
                .debugStatusAccessibilityValue(model.debugSessionStatus)
                #endif
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

struct ReviewStatusLayout<Content: View>: View {
    @ViewBuilder let content: () -> Content
    #if DEBUG
    private var diagnosticValue: String?

    init(@ViewBuilder content: @escaping () -> Content) {
        self.content = content
    }

    func debugStatusAccessibilityValue(_ value: String) -> Self {
        var view = self
        view.diagnosticValue = value
        return view
    }
    #endif

    var body: some View {
        GeometryReader { viewport in
            ScrollView(.vertical) {
                content()
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity)
                    .frame(minHeight: max(0, viewport.size.height))
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("review.session.status")
            #if DEBUG
            .accessibilityValue(Text(verbatim: diagnosticValue ?? ""))
            #endif
        }
    }
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
    var status: AnyView? = nil
    var timerPresentation: (() -> ReviewTimerPresentation)? = nil
    @State private var noticeContentHeight: CGFloat?
    @ViewBuilder let card: () -> Card
    var body: some View {
        #if os(macOS)
        macBody
        #else
        touchBody
        #endif
    }
    #if os(macOS)
    /// The window's own toolbar holds the corners (N3); the card sits between it and the answers.
    private var macBody: some View {
        VStack(spacing: 0) {
            cardRegion
            ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate)
        }
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button(action: close) {
                    Label { Text("Back to Decks", bundle: .module) } icon: { Image(systemName: "chevron.backward") }
                        .labelStyle(.iconOnly)
                }
                .help(Text("Back to Decks", bundle: .module))
                .accessibilityIdentifier("review.session.close")
            }
            ToolbarSpacer()
            ToolbarItemGroup(placement: .primaryAction) {
                timer
                status
            }
        }
        .toolbar(removing: .title)
        .toolbarBackgroundVisibility(.hidden, for: .windowToolbar)
        .background(.token(.cardPage))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("review.session.screen")
        .accessibilityAction(named: Text("Show Answer", bundle: .module)) { reveal() }
        .accessibilityAction(named: Text(verbatim: undoDescription)) { if canUndo { undo() } }
        .accessibilityAction(named: Text("Redo Answer", bundle: .module)) { if canRedo { redo() } }
    }
    #else
    /// System bars hold the corners, so they move to the side on a foldable (R10).
    /// One container in every fold position keeps the card's page alive through a fold (R576).
    private var touchBody: some View {
        NavigationStack {
            VStack(spacing: 0) {
                VStack(spacing: Spacing.xSmall) {
                    HStack(spacing: Spacing.small) {
                        sessionProgress
                        timer
                    }
                    .padding(.horizontal, Spacing.medium)
                    cardRegion
                }
                ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate)
            }
            .background(.token(.cardPage))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(action: close) {
                        Label { Text("Close Review", bundle: .module) } icon: { Image(systemName: "xmark") }
                    }
                    .help(Text("Close Review", bundle: .module))
                    .accessibilityIdentifier("review.session.close")
                }
                ToolbarItemGroup(placement: .primaryAction) {
                    Button { if canUndo { undo() } } label: {
                        Label { Text(verbatim: undoDescription) } icon: { Image(systemName: "arrow.uturn.backward") }
                    }
                    .disabled(!canUndo)
                    .help(undoDescription)
                    .accessibilityIdentifier("review.session.back")
                    if canRedo {
                        Button { if canRedo { redo() } } label: {
                            Label { Text("Redo Answer", bundle: .module) } icon: { Image(systemName: "arrow.uturn.forward") }
                        }
                        .help(Text("Redo Answer", bundle: .module))
                        .accessibilityIdentifier("review.session.redo")
                    }
                }
                if let status {
                    ToolbarItem(placement: .primaryAction) { status }
                }
            }
            .navigationBarTitleDisplayMode(.inline)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("review.session.screen")
    }
    #endif
    /// The card under any notice; a long notice scrolls within its limit, so the answers stay in reach.
    private var cardRegion: some View {
        GeometryReader { middle in
            VStack(spacing: 0) {
                if let notice {
                    let noticeLimit = max(0, middle.size.height) * 0.4
                    ScrollView(.vertical) {
                        Text(verbatim: notice).font(.callout).foregroundStyle(.token(.statusWarning))
                            .padding(Spacing.small)
                            .fixedSize(horizontal: false, vertical: true)
                            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { noticeContentHeight = $0 }
                            .accessibilityIdentifier("review.session.notice")
                    }
                    .scrollIndicators(.visible)
                    .frame(height: min(noticeContentHeight ?? noticeLimit, noticeLimit))
                }
                card().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }
    private var sessionProgress: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: remaining).font(.caption)
                .fixedSize(horizontal: false, vertical: true)
            ProgressView(value: progress)
                .tint(.token(.progressFill))
        }
        .foregroundStyle(.token(.textPrimary))
        .layoutPriority(1)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text("Session Progress", bundle: .module))
        .accessibilityValue(remaining)
        .accessibilityIdentifier("review.session.progress")
        .help(remaining)
    }
    @ViewBuilder private var timer: some View {
        if let timerPresentation {
            OnScreenTimerView(read: timerPresentation)
        }
    }
}

/// One reserved bottom region for question, answer and transition; terminal states need none.
public struct ReviewAnswerBar: View {
    #if !os(macOS)
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    #endif
    public let phase: ReviewCoordinator.Phase
    public let states: ReviewSchedulingStates?
    public let enabled: Bool
    public let reveal: () -> Void
    public let rate: (ReviewRating) -> Void
    #if !os(macOS)
    @State private var fold = FoldGeometry()
    #endif
    public init(phase: ReviewCoordinator.Phase, states: ReviewSchedulingStates?, enabled: Bool,
                reveal: @escaping () -> Void, rate: @escaping (ReviewRating) -> Void) {
        self.phase = phase; self.states = states; self.enabled = enabled; self.reveal = reveal; self.rate = rate
    }
    public var body: some View {
        #if os(macOS)
        if phase == .question || phase == .answer || phase == .transition { macBar }
        #else
        touchBar
        #endif
    }
    #if os(macOS)
    private func macRating(_ value: ReviewRating, _ title: LocalizedStringKey, _ label: ReviewAnswerLabel?, again: Bool = false, prominent: Bool = false) -> some View {
        let interval = label.map(Self.interval) ?? "—"
        return Button { rate(value) } label: {
            HStack(spacing: Spacing.xSmall) {
                Text(title, bundle: .module)
                    .foregroundStyle(again ? AnyShapeStyle(.token(.ratingAgain)) : AnyShapeStyle(.foreground))
                Text(verbatim: interval)
                    .monospacedDigit()
                    .foregroundStyle(prominent ? AnyShapeStyle(.foreground) : AnyShapeStyle(.secondary))
            }
            .frame(width: ContentWidth.answerButton)
        }
        .frame(minHeight: ControlMetrics.minimumTarget)
        .contentShape(Rectangle())
        .disabled(!enabled || phase != .answer)
        .accessibilityLabel(Text("\(Text(title, bundle: .module)), \(interval)"))
        .accessibilityIdentifier("review.session.rate.\(ReviewCoordinator.name(value))")
    }
    private var macRatings: some View {
        HStack {
            macRating(.again, "Again", states?.again.label, again: true).buttonStyle(.bordered)
            macRating(.hard, "Hard", states?.hard.label).buttonStyle(.bordered)
            macRating(.good, "Good", states?.good.label, prominent: true).buttonStyle(.borderedProminent)
            macRating(.easy, "Easy", states?.easy.label).buttonStyle(.bordered)
        }
    }
    private var macBar: some View {
        ZStack {
            // Reserved on both sides so reveal never moves the card (N3).
            if phase == .answer { macRatings.allowsHitTesting(enabled) }
            else { macRatings.hidden().allowsHitTesting(false).accessibilityHidden(true) }
            if phase == .question {
                Button(action: reveal) { Text("Show Answer", bundle: .module) }
                    .buttonStyle(.borderedProminent)
                    .frame(minHeight: ControlMetrics.minimumTarget)
                    .contentShape(Rectangle())
                    .disabled(!enabled)
                    .accessibilityIdentifier("review.session.reveal")
            }
        }
        .padding()
        .frame(maxWidth: .infinity)
    }
    #endif
    #if !os(macOS)
    /// The answers either side of a fold running through the row (R9, N13).
    @ViewBuilder private var touchBar: some View {
        if phase == .question || phase == .answer || phase == .transition {
            ZStack {
                // Kept in layout on both sides, including at large text sizes.
                if phase == .answer {
                    ratings.allowsHitTesting(enabled)
                } else {
                    ratings.hidden()
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
                if phase == .question {
                    if dynamicTypeSize.isAccessibilitySize, fold.acrossWidth == nil {
                        revealButton
                    } else {
                        FoldAvoidingRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) {
                            Color.clear.frame(height: 0)
                            revealButton
                        }
                    }
                }
            }
            .onGeometryChange(for: FoldGeometry.self) { FoldGeometry($0) } action: { fold = $0 }
            .padding(Spacing.small)
            .frame(maxWidth: fold.acrossWidth == nil ? ContentWidth.readable : nil)
            .frame(maxWidth: .infinity)
            .fixedSize(horizontal: false, vertical: true)
            .layoutPriority(1)
        }
    }
    private var revealButton: some View {
        Button(action: reveal) {
            Text("Show Answer", bundle: .module)
                .font(.subheadline.weight(.semibold))
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
        }
        .buttonStyle(.bordered)
        .buttonBorderShape(.roundedRectangle)
        .tint(ColorToken.textPrimary.color)
        .disabled(!enabled)
        .accessibilityIdentifier("review.session.reveal")
    }
    @ViewBuilder private var ratings: some View {
        let again = rating(.again, title: "Again", label: states?.again.label, color: .ratingAgain)
        let hard = rating(.hard, title: "Hard", label: states?.hard.label, color: .textPrimary)
        let good = rating(.good, title: "Good", label: states?.good.label, color: .ratingGood)
        let easy = rating(.easy, title: "Easy", label: states?.easy.label, color: .textPrimary)
        if dynamicTypeSize.isAccessibilitySize, fold.acrossWidth == nil {
            VStack(spacing: Spacing.xSmall) { again; hard; good; easy }
        } else if dynamicTypeSize.isAccessibilitySize {
            FoldAvoidingRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) {
                VStack(spacing: Spacing.xSmall) { again; hard }
                VStack(spacing: Spacing.xSmall) { good; easy }
            }
        } else {
            FoldAvoidingRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) {
                HStack(spacing: Spacing.xSmall) { again; hard }
                HStack(spacing: Spacing.xSmall) { good; easy }
            }
        }
    }
    private func rating(_ rating: ReviewRating, title: String, label: ReviewAnswerLabel?, color: ColorToken) -> some View {
        Button { rate(rating) } label: {
            Group {
                if dynamicTypeSize.isAccessibilitySize {
                    HStack(spacing: Spacing.small) {
                        Text(LocalizedStringKey(title), bundle: .module).font(.headline)
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: Spacing.small)
                        Text(verbatim: label.map(Self.interval) ?? "—").font(.caption).foregroundStyle(.token(.textPrimary))
                            .fixedSize(horizontal: false, vertical: true)
                    }
                } else {
                    VStack(spacing: 0) {
                        Text(LocalizedStringKey(title), bundle: .module).font(.headline)
                            .fixedSize(horizontal: false, vertical: true)
                        Text(verbatim: label.map(Self.interval) ?? "—").font(.caption).foregroundStyle(.token(.textPrimary))
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .multilineTextAlignment(.center)
                }
            }
            .foregroundStyle(.token(color))
            .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
            .fixedSize(horizontal: false, vertical: true)
        }
        .buttonStyle(.bordered)
        .buttonBorderShape(.roundedRectangle)
        .disabled(!enabled || phase != .answer)
        .accessibilityLabel("\(title), \(label.map(Self.interval) ?? "")")
        .accessibilityIdentifier("review.session.rate.\(ReviewCoordinator.name(rating))")
    }
    #endif
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
