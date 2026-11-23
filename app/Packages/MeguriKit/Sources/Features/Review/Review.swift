import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Edit Note's touch entry and the form it presents (N20.8). The shell
/// supplies the form, so this module does not depend on the editor.
public struct ReviewEditNote {
    public let flow: EditNoteFlow
    public let open: () -> Void
    public let form: AnyView
    public init(flow: EditNoteFlow, open: @escaping () -> Void, form: AnyView) {
        self.flow = flow; self.open = open; self.form = form
    }
}

public struct ReviewView: View {
    public let model: ReviewCoordinator
    public let close: () -> Void
    public let status: AnyView?
    public let editNote: ReviewEditNote?
    public let visibilityChanged: (Bool) -> Void
    /// Today's Study (N24.3): the walk whose open deck `model` is. One review for the
    /// whole walk: its progress, its finished screen, and nothing between its decks.
    public let walk: StudyWalk?
    @Environment(\.colorScheme) private var colorScheme
    public init(model: ReviewCoordinator, walk: StudyWalk? = nil, status: AnyView? = nil, editNote: ReviewEditNote? = nil,
                visibilityChanged: @escaping (Bool) -> Void = { _ in }, close: @escaping () -> Void) {
        self.status = status; self.walk = walk; self.editNote = editNote
        self.model = model; self.visibilityChanged = visibilityChanged; self.close = close
    }
    public var body: some View {
        ReviewLayout(progress: walk?.progress ?? model.sessionProgress, notice: model.notice,
            undoDescription: undoDescription, canUndo: model.canUndo, canRedo: model.canRedo,
            phase: model.phase, states: model.shownItem?.nextStates, enabled: !model.paused,
            close: close, undo: { model.restore() }, redo: { model.restore(redo: true) },
            reveal: model.reveal, rate: model.rate,
            redoDescription: redoDescription, status: status,
            actions: moreActions, leech: model.leechNotice) {
            ZStack {
                if let pair = model.pair {
                    // A walk's next deck brings its own pages: a new host for them, while
                    // the review around it stays (N24.3).
                    #if os(macOS)
                    ReviewCardUnderAnswers { covered in
                        ReviewCardHost(model: model, pair: pair, close: close, actions: moreActions, obscuredBottom: covered)
                    }
                        .opacity(model.phase == .question || model.phase == .answer || model.phase == .transition ? 1 : 0)
                        .id(ObjectIdentifier(model))
                    #else
                    ReviewCardUnderAnswers { covered in
                        ReviewCardHost(model: model, pair: pair, close: close, actions: moreActions, obscuredBottom: covered)
                    }
                        .accessibilityIdentifier("review.session.card")
                        .accessibilityAction(named: Text("Show Answer", bundle: .module)) { model.reveal() }
                        .accessibilityAction(.magicTap) { model.reveal() }
                        .opacity(model.phase == .question || model.phase == .answer || model.phase == .transition ? 1 : 0)
                        .id(ObjectIdentifier(model))
                    #endif
                }
                stateContent
            }
        }
        #if os(iOS)
        .modifier(ReviewEditNotePresentation(editNote: editNote))
        #else
        // N20.8: on the Mac the form is a panel over the card, which owns the
        // keys while it shows (the review's input is held, N11.1).
        .overlay {
            if let editNote, editNote.flow.isPresented { editNote.form }
        }
        #endif
        .onChange(of: model.leechAnnouncement?.id) { _, id in
            guard id != nil, let text = model.leechAnnouncement?.text else { return }
            AccessibilityNotification.Announcement(text).post()
        }
        // N20.8: no banner or toast; VoiceOver says what happened.
        .onChange(of: model.cardActionAnnouncement?.id) { _, id in
            guard id != nil, let action = model.cardActionAnnouncement?.action else { return }
            AccessibilityNotification.Announcement(Self.announcement(action)).post()
        }
        // S5: "Answer undone", "Edit Note redone"; nothing when the restore failed.
        .onChange(of: model.restoreAnnouncement?.id) { _, id in
            guard id != nil, let restored = model.restoreAnnouncement else { return }
            let name = ReviewStepNames.display(restored.step)
            let text = restored.redo ? String(localized: "\(name) redone", bundle: .module)
                : String(localized: "\(name) undone", bundle: .module)
            AccessibilityNotification.Announcement(text).post()
        }
        .onAppear { visibilityChanged(true) }
        .onDisappear { visibilityChanged(false) }
        .task(id: ObjectIdentifier(model)) { await appearance() }
        .onChange(of: colorScheme) { _, _ in Task { await appearance() } }
    }
    /// Between a walk's decks: one session has come to its end and the next is starting.
    private var betweenDecks: Bool {
        guard let walk else { return false }
        switch model.phase {
        case .waiting, .finished, .ended: return walk.advancing
        case .loading: return walk.turns > 1
        case .question, .answer, .transition, .error: return false
        }
    }
    @ViewBuilder private var stateContent: some View {
        if betweenDecks {
            // No screen, title or pause between a walk's decks (N24.3).
            ProgressView()
        } else {
            phaseContent
        }
    }
    @ViewBuilder private var phaseContent: some View {
        switch model.phase {
        case .loading:
            // Shown only for a start that takes longer than Study's wait (N16.8):
            // the system indicator alone, no title.
            ReviewStatusLayout { ProgressView() }
                #if DEBUG
                .debugStatusAccessibilityValue(model.debugSessionStatus)
                #endif
        case .waiting, .finished:
            ReviewStatusLayout {
                if let walk {
                    // The walk's one finished screen: its cards and time with the open deck's
                    // Keep Going, never Waiting for Learning (N24.3).
                    ReviewTerminalContent(waiting: false, summary: walk.summary,
                        continueAvailable: model.continueAvailable, canContinue: model.canContinue,
                        canRefresh: model.canRefresh, canFinish: model.canFinish,
                        refresh: model.retry, continueReview: model.continueReview, close: close)
                } else {
                    ReviewTerminalContent(waiting: model.phase == .waiting, summary: model.sessionSummary,
                        continueAvailable: model.continueAvailable, canContinue: model.canContinue,
                        canRefresh: model.canRefresh, canFinish: model.canFinish,
                        refresh: model.retry, continueReview: model.continueReview, close: close)
                }
            }
            #if DEBUG
            .debugStatusAccessibilityValue(model.debugSessionStatus)
            #endif
        case .error:
            ReviewStatusLayout {
                ReviewStateMessage(title: Text("Review Paused", bundle: .module),
                    message: model.notice.map { Text(verbatim: $0) }) {
                    Button(action: model.retry) { Text("Try Again", bundle: .module).reviewStateLabel() }
                        .accessibilityIdentifier("review.session.retry")
                        .reviewStateTarget()
                }
            }
            #if DEBUG
            .debugStatusAccessibilityValue(model.debugSessionStatus)
            #endif
        case .ended:
            ReviewStatusLayout { ReviewStateMessage(title: Text("Session Ended", bundle: .module)) }
                #if DEBUG
                .debugStatusAccessibilityValue(model.debugSessionStatus)
                #endif
        case .transition:
            if model.item == nil { ProgressView() }
        case .question, .answer: EmptyView()
        }
    }
    private var moreActions: ReviewMoreActions {
        let editNote = editNote, model = model
        return ReviewMoreActions(editNoteEnabled: { editNote?.flow.canOpen ?? false }, editNote: { editNote?.open() },
                                 cardActionsEnabled: { model.canActOnCard }, act: model.act)
    }
    static func announcement(_ action: ReviewCardAction) -> String {
        switch action {
        case .bury: String(localized: "Card Buried", bundle: .module)
        case .suspend: String(localized: "Card Suspended", bundle: .module)
        }
    }
    /// S5: "Redo <step>" by the step's display name.
    private var redoDescription: String {
        if case .available(let step) = model.redoStatus {
            return String(localized: "Redo \(ReviewStepNames.display(step.name))", bundle: .module)
        }
        return String(localized: "Redo", bundle: .module)
    }
    private var undoDescription: String {
        switch model.undoStatus {
        case .available(let step): String(localized: "Undo \(ReviewStepNames.display(step.name))", bundle: .module)
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

/// The card's page runs on under the answer area, to the window's bottom
/// edge, and keeps clear the part the answers cover, so its last line can
/// always scroll above them (N16.9).
struct ReviewCardUnderAnswers<Content: View>: View {
    @Environment(\.reviewAnswerAreaTop) private var answerAreaTop
    @State private var bottom: CGFloat?
    @ViewBuilder let content: (CGFloat) -> Content
    var body: some View {
        content(covered)
            .onGeometryChange(for: CGFloat.self) { $0.frame(in: .global).maxY } action: { bottom = $0 }
            .ignoresSafeArea(.container, edges: .bottom)
    }
    private var covered: CGFloat {
        guard let answerAreaTop, let bottom else { return 0 }
        return max(0, bottom - answerAreaTop)
    }
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
                    .frame(maxWidth: Self.columnWidth ?? .infinity)
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
    /// iPad and Mac keep a state's text in a centred column, the touch
    /// answer row's width; iPhone keeps the full width (N16.4, N16.6).
    private static var columnWidth: CGFloat? {
        #if os(macOS)
        ContentWidth.readable
        #else
        // By idiom, as the shell chooses its layout (N13.1).
        UIDevice.current.userInterfaceIdiom == .pad ? ContentWidth.readable : nil
        #endif
    }
}


struct ReviewLayout<Card: View>: View {
    let progress: ReviewSessionProgress
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
    var redoDescription = "Redo"
    var status: AnyView? = nil
    var actions = ReviewMoreActions()
    /// N16.11: the leech notice, as one icon first in the top controls.
    var leech: ReviewLeechNotice? = nil
    @State private var noticeContentHeight: CGFloat?
    @State private var answerAreaTop: CGFloat?
    @State private var bottomInset: CGFloat = 0
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
        cardRegion
        // The card runs on under the answers (N16.9); the window has no bottom
        // inset, so the progress bar is the window's last row (N16.1).
        .safeAreaBar(edge: .bottom, spacing: 0) { answerArea }
        .scrollEdgeEffectStyle(.soft, for: .bottom)
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
            // N16.11: first among the trailing items, before the sync status.
            if let leech {
                ToolbarItem(placement: .primaryAction) { ReviewLeechButton(notice: leech) }
            }
            if let status {
                ToolbarItem(placement: .primaryAction) { status }
            }
        }
        .toolbar(removing: .title)
        .toolbarBackgroundVisibility(.hidden, for: .windowToolbar)
        .background(.token(.cardPage))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("review.session.screen")
        .accessibilityAction(named: Text("Show Answer", bundle: .module)) { reveal() }
        .accessibilityAction(named: Text(verbatim: undoDescription)) { if canUndo { undo() } }
        .accessibilityAction(named: Text(verbatim: redoDescription)) { if canRedo { redo() } }
    }
    #else
    /// System bars hold the corners, so they move to the side on a foldable (R10).
    /// One container in every fold position keeps the card's page alive through a fold (R576).
    private var touchBody: some View {
        NavigationStack {
            cardRegion
            // The card runs on under the answers, which float over it in the
            // system's bottom bar, just above the home indicator (N16.9).
            .safeAreaBar(edge: .bottom, spacing: 0) { answerArea }
            // Soft only: no line where the card passes under the answers (N16.9).
            .scrollEdgeEffectStyle(.soft, for: .bottom)
            .background(.token(.cardPage))
            // On the screen's very bottom edge, under the home indicator, in
            // every fold position; not a control, so it may cross a fold (N16.1).
            .overlay {
                if showsProgress {
                    Color.clear
                        .overlay(alignment: .bottom) { ReviewProgressBar(progress: progress) }
                        .ignoresSafeArea(.container, edges: .bottom)
                        .allowsHitTesting(false)
                }
            }
            .onGeometryChange(for: CGFloat.self) { $0.safeAreaInsets.bottom } action: { bottomInset = $0 }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    // N20.5: the xmark's name is "Back to Decks", as the sidebar layouts' button.
                    Button(action: close) {
                        Label { Text("Back to Decks", bundle: .module) } icon: { Image(systemName: "xmark") }
                    }
                    .help(Text("Back to Decks", bundle: .module))
                    .accessibilityIdentifier("review.session.close")
                }
                ToolbarItemGroup(placement: .primaryAction) {
                    // N16.11: the leech icon comes first: [leech][undo][more].
                    if let leech { ReviewLeechButton(notice: leech) }
                    Button { if canUndo { undo() } } label: {
                        Label { Text(verbatim: undoDescription) } icon: { Image(systemName: "arrow.uturn.backward") }
                    }
                    .disabled(!canUndo)
                    .help(undoDescription)
                    .accessibilityIdentifier("review.session.back")
                    if canRedo {
                        Button { if canRedo { redo() } } label: {
                            Label { Text(verbatim: redoDescription) } icon: { Image(systemName: "arrow.uturn.forward") }
                        }
                        .help(redoDescription)
                        .accessibilityIdentifier("review.session.redo")
                    }
                    // N20.8: one More menu after Undo, before the sync status.
                    ReviewMoreMenu(actions: actions)
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
    /// Both take the window's full width on every device: card styling is written
    /// for a card that fills the window (N16.4, changed 2026-11-20).
    private var cardRegion: some View {
        GeometryReader { middle in
            VStack(spacing: 0) {
                // N16.11: a leech answer's notice is the icon in the top controls instead.
                if let notice, leech == nil {
                    let noticeLimit = max(0, middle.size.height) * 0.4
                    ScrollView(.vertical) {
                        Label {
                            Text(verbatim: notice).foregroundStyle(.token(.textPrimary))
                        } icon: {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .symbolRenderingMode(.multicolor)
                                .accessibilityHidden(true)
                        }
                        .font(.callout)
                        .padding(Spacing.small)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity)
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { noticeContentHeight = $0 }
                        .accessibilityIdentifier("review.session.notice")
                    }
                    .scrollIndicators(.visible)
                    .frame(height: min(noticeContentHeight ?? noticeLimit, noticeLimit))
                }
                card().frame(maxWidth: .infinity, maxHeight: .infinity)
                    .environment(\.reviewAnswerAreaTop, answerAreaTop)
            }
        }
    }
    private var showsProgress: Bool { phase == .question || phase == .answer || phase == .transition }

    /// The answers, then one standard spacing, then the progress bar (N16.1,
    /// N16.9). Its top edge tells the card how much of it lies underneath.
    private var answerArea: some View {
        Group {
            #if os(macOS)
            // The window has no bottom inset: 8 pt, then the bar as the last row.
            VStack(spacing: Spacing.xSmall) {
                ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate)
                if showsProgress { ReviewProgressBar(progress: progress).allowsHitTesting(false) }
            }
            #else
            // The buttons' bottom is the safe area's: just clear of the home
            // indicator, whose strip holds the progress bar at the very edge.
            // With no bottom inset, 8 pt and the bar's thickness stay free.
            ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate)
                .padding(.bottom, bottomInset > 0 ? 0 : Spacing.xSmall + ReviewProgressBar.thickness)
            #endif
        }
        // A bar: a touch between or beside its buttons stays in it, so it
        // reaches neither the card's page nor its tap zones (N16.9).
        .contentShape(Rectangle())
        .onGeometryChange(for: CGFloat.self) { $0.frame(in: .global).minY } action: { answerAreaTop = $0 }
    }
}

extension EnvironmentValues {
    /// Where the answer area begins, in global coordinates: the card's page
    /// keeps that much of its bottom clear of it (N16.9).
    @Entry var reviewAnswerAreaTop: CGFloat? = nil
}

#if os(iOS)
/// Edit Note's form as Add Note's is placed (N17.1): a sheet, or the whole
/// screen on an iPhone at regular width. A fold moves the same draft and
/// page to the other presentation; the swipe down asks as Cancel does.
private struct ReviewEditNotePresentation: ViewModifier {
    let editNote: ReviewEditNote?
    @Environment(\.horizontalSizeClass) private var sizeClass
    private var coversScreen: Bool {
        AddNotePresentation.coversScreen(phone: UIDevice.current.userInterfaceIdiom == .phone, regularWidth: sizeClass == .regular)
    }
    private func shown(cover: Bool) -> Binding<Bool> {
        Binding(get: { editNote?.flow.isPresented == true && coversScreen == cover },
                set: { if !$0, coversScreen == cover, let flow = editNote?.flow, flow.isPresented { Task { await flow.requestCancel() } } })
    }
    func body(content: Content) -> some View {
        content
            .sheet(isPresented: shown(cover: false)) { editNote?.form }
            .fullScreenCover(isPresented: shown(cover: true)) { editNote?.form }
    }
}
#endif

/// The session's progress as a thin grey bar: no text, no accent colour,
/// and not a control (N16.1).
struct ReviewProgressBar: View {
    let progress: ReviewSessionProgress
    var body: some View {
        #if os(macOS)
        element(bar)
        #else
        // The system indicator's own element reported the whole answer area
        // as its frame; the track itself carries the element instead.
        bar.accessibilityHidden(true)
            .overlay { element(Color.clear) }
        #endif
    }
    private func element(_ view: some View) -> some View {
        view
            // Read as text: it takes no input, and as a progress control the
            // audit asks a full hit region of this thin bar.
            .accessibilityElement(children: .ignore)
            .accessibilityAddTraits(.isStaticText)
            .accessibilityLabel(Text("Session Progress", bundle: .module))
            .accessibilityValue(Text("\(progress.answered) of \(progress.total)", bundle: .module))
            .accessibilityIdentifier("review.session.progress")
    }
    /// About the system indicator's track.
    static let thickness: CGFloat = 4
    #if os(macOS)
    /// Drawn on the Mac (exception, N16.1): the system's linear indicator
    /// draws its rounded track inset in its frame, so it could not lie on
    /// the window's bottom edge or reach its sides. This bar fills its frame:
    /// a track in a quaternary fill, the value in the secondary one.
    private var bar: some View {
        Rectangle()
            .fill(.quaternary)
            .overlay(alignment: .leading) {
                GeometryReader { track in
                    Rectangle()
                        .fill(.secondary)
                        .frame(width: track.size.width * min(1, max(0, progress.fraction)))
                }
            }
            .frame(maxWidth: .infinity)
            .frame(height: Self.thickness)
    }
    #else
    /// The system's linear indicator lies on the screen's bottom edge as it is.
    private var bar: some View {
        ProgressView(value: progress.fraction)
            .progressViewStyle(.linear)
            .tint(.secondary)
    }
    #endif
}

/// A review state said in plain text: a title, an optional message and its
/// actions, centred, with no symbol (N16.6).
struct ReviewStateMessage<Actions: View>: View {
    let title: Text
    var message: Text?
    @ViewBuilder let actions: () -> Actions
    var body: some View {
        VStack(spacing: Spacing.large) {
            VStack(spacing: Spacing.xSmall) {
                title.font(.title2.bold()).foregroundStyle(.token(.textPrimary))
                    .reviewStateHeading()
                message?.foregroundStyle(.token(.textSecondary))
            }
            .multilineTextAlignment(.center)
            ReviewStateActions(content: actions)
        }
        .padding()
        .frame(maxWidth: .infinity)
    }
}
extension ReviewStateMessage where Actions == EmptyView {
    init(title: Text, message: Text? = nil) { self.init(title: title, message: message) { EmptyView() } }
}

/// State actions side by side, stacked at the accessibility text sizes:
/// standard bordered buttons with a full touch target. One copy of each
/// button: under ViewThatFits the Dynamic Type audit fails them (N14.3).
struct ReviewStateActions<Content: View>: View {
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @ViewBuilder let content: () -> Content
    var body: some View {
        let layout = dynamicTypeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(spacing: Spacing.small)) : AnyLayout(HStackLayout(spacing: Spacing.small))
        layout { content() }
        .buttonStyle(.bordered)
        #if !os(macOS)
        .controlSize(.large)
        .buttonBorderShape(.roundedRectangle)
        #endif
    }
}
extension Text {
    /// The answer row's fill and text colour: the system's bordered fill
    /// with primary text, not a tinted fill.
    func reviewStateLabel() -> some View {
        #if os(macOS)
        self
        #else
        foregroundStyle(.token(.textPrimary))
        #endif
    }
}
extension View {
    /// A heading on touch. On the Mac the trait turns the text into a heading
    /// element, which is no longer a static text: tests and scripts that
    /// look the title up as text would miss it, so it stays plain text there.
    func reviewStateHeading() -> some View {
        #if os(macOS)
        self
        #else
        accessibilityAddTraits(.isHeader)
        #endif
    }
    /// The Mac's standard button is shorter than the minimum pointer target.
    func reviewStateTarget() -> some View {
        #if os(macOS)
        frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget).contentShape(Rectangle())
        #else
        self
        #endif
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
        .padding([.horizontal, .top])
        .frame(maxWidth: .infinity)
    }
    #endif
    #if !os(macOS)
    /// The answers either side of a fold running through the row (R9, N13).
    @ViewBuilder private var touchBar: some View {
        if phase == .question || phase == .answer || phase == .transition {
            // Bottom-aligned, so Show Answer sits where the last answer row will be.
            ZStack(alignment: .bottom) {
                // Kept in layout on both sides, including at large text sizes.
                if phase == .answer {
                    ratings.allowsHitTesting(enabled)
                } else {
                    ratings.hidden()
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
                if phase == .question {
                    if fold.acrossWidth != nil {
                        // Under the card's centre at its half-row size, across the
                        // fold: an exception to R9 (N16.3, 2026-11-22).
                        FoldCentredRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) { revealButton }
                    } else if dynamicTypeSize.isAccessibilitySize {
                        revealButton
                    } else {
                        // Centred, two answers wide (N16.3).
                        CentredHalfRow(spacing: Spacing.xSmall) { revealButton }
                    }
                }
            }
            .onGeometryChange(for: FoldGeometry.self) { FoldGeometry($0) } action: { fold = $0 }
            .padding(.horizontal)
            .padding(.top, Spacing.small)
            .frame(maxWidth: fold.acrossWidth == nil ? ContentWidth.readable : nil)
            .frame(maxWidth: .infinity)
            .fixedSize(horizontal: false, vertical: true)
            .layoutPriority(1)
        }
    }
    private var revealButton: some View {
        Button(action: reveal) {
            Text("Show Answer", bundle: .module)
                .font(.headline)
                .foregroundStyle(.token(.textPrimary))
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
        }
        .buttonStyle(.bordered)
        .buttonBorderShape(.roundedRectangle)
        .answerAreaBacking()
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
        .answerAreaBacking()
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

#if !os(macOS)
extension View {
    /// N16.9: the card passes under the answer buttons. Each button stands on
    /// the card page's own colour, clipped to its rounded rectangle, under its
    /// grey fill: card text never shows through, and over a plain card the
    /// button looks exactly as before. A blurring material was tried first:
    /// the system fill turns vibrant over it, lighter in light mode and darker
    /// in dark, so the buttons no longer looked the same. The page colour has
    /// its increased-contrast variants and needs nothing for Reduce Transparency.
    func answerAreaBacking() -> some View {
        background(.token(.cardPage), in: ButtonBorderShape.roundedRectangle)
    }
}

/// One child, centred, as wide as one of two equal columns in the row.
private struct CentredHalfRow: Layout {
    let spacing: CGFloat
    private func childWidth(_ width: CGFloat) -> CGFloat { max(0, width - spacing) / 2 }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let child = subviews.first else { return .zero }
        guard let width = proposal.width, width.isFinite else { return child.sizeThatFits(proposal) }
        let size = child.sizeThatFits(ProposedViewSize(width: childWidth(width), height: proposal.height))
        return CGSize(width: width, height: size.height)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: CGPoint(x: bounds.midX, y: bounds.minY), anchor: .top,
            proposal: ProposedViewSize(width: childWidth(bounds.width), height: bounds.height))
    }
}
#endif

/// What the review's More menu and the Mac's card context menu act with (N20.8).
/// The states are read when asked, so a key handler or a menu made once
/// still sees the current card (N4).
struct ReviewMoreActions {
    var editNoteEnabled: () -> Bool = { false }
    var editNote: () -> Void = {}
    var cardActionsEnabled: () -> Bool = { false }
    var act: (ReviewCardAction) -> Void = { _ in }
    var canEditNote: Bool { editNoteEnabled() }
    var canActOnCard: Bool { cardActionsEnabled() }
}

/// N20.8: the touch review's More button (`ellipsis`, "More"), a pull-down
/// menu in groups, each item with its system symbol. Later card actions slot
/// into these groups or new ones after them.
/// N16.11: the leech notice (`review/reviewer-strings#S3`) as one yellow
/// warning icon in the top controls, in place of a banner above the card; a
/// press shows what it means.
struct ReviewLeechButton: View {
    let notice: ReviewLeechNotice
    @State private var shown = false
    var body: some View {
        Button { shown = true } label: {
            Label { Text("review.leech.title", bundle: .module) } icon: {
                // The symbol's own colours: the system's yellow triangle in light,
                // dark and Increase Contrast.
                Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor)
            }
        }
        .help(Text("review.leech.title", bundle: .module))
        .accessibilityHint(Text("review.leech.hint", bundle: .module))
        .accessibilityIdentifier("review.session.leech")
        .popover(isPresented: $shown) { ReviewLeechDetails(notice: notice) }
    }
}

/// What a leech notice means, with what the answer did to the card.
struct ReviewLeechDetails: View {
    let notice: ReviewLeechNotice
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.small) {
            Text("review.leech.title", bundle: .module)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            Text(notice.suspended ? "review.leech.suspended" : "review.leech.tagged", bundle: .module)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding()
        // About a primary action's width in a popover; the text wraps.
        .frame(idealWidth: ContentWidth.primaryAction, maxWidth: ContentWidth.readable, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("review.session.leech.details")
        #if !os(macOS)
        .presentationDetents([.medium])
        #endif
    }
}

struct ReviewMoreMenu: View {
    let actions: ReviewMoreActions
    var body: some View {
        Menu {
            ReviewCardActionItems(actions: actions)
        } label: {
            Label { Text("More", bundle: .module) } icon: { Image(systemName: "ellipsis") }
        }
        .help(Text("More", bundle: .module))
        .accessibilityIdentifier("review.session.more")
    }
}

/// The items of the More menu and the Mac card's context menu, in groups:
/// Edit Note | Bury Card, Suspend Card.
struct ReviewCardActionItems: View {
    let actions: ReviewMoreActions
    var body: some View {
        Section {
            Button(action: actions.editNote) {
                Label { Text("Edit Note", bundle: .module) } icon: { Image(systemName: "pencil") }
            }
            .disabled(!actions.canEditNote)
            .accessibilityIdentifier("review.menu.editNote")
        }
        Section {
            Button { actions.act(.bury) } label: {
                Label { Text("Bury Card", bundle: .module) } icon: { Image(systemName: "moon") }
            }
            .disabled(!actions.canActOnCard)
            .accessibilityIdentifier("review.menu.buryCard")
            Button { actions.act(.suspend) } label: {
                Label { Text("Suspend Card", bundle: .module) } icon: { Image(systemName: "pause.circle") }
            }
            .disabled(!actions.canActOnCard)
            .accessibilityIdentifier("review.menu.suspendCard")
        }
        .labelStyle(.titleAndIcon)
    }
}
