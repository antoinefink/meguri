import CoreBridge
import Foundation
import Observation

/// The Statistics page's model (plan U294; `stats/statistics-page` S3, S4, S7, S10, S11, S12):
/// every value the page shows, written from the core's parts, with no UI. One per window: its
/// deck, period and switches are the window's, and the device keeps the last used (S3.3).
///
/// One refresh is one request for every part (H5, H15); the parts fill the boxes as they arrive
/// by tier, each box from one refresh and only for the scope on screen. Kept results show at
/// once and are replaced in place; a box whose parts have not arrived shows its progress. A
/// period change within the year re-bins with no core call (S3.2). Nothing here waits on the
/// core on the main actor, and nothing writes the collection (S17).
@Observable @MainActor
public final class StatsPageModel {
    // MARK: State

    /// The window's deck, period and switches.
    public private(set) var selection: StatsPageSelection
    /// The deck tree the page knows (the deck list's).
    public private(set) var decks: StatsDeckDirectory = .unknown
    /// A collection is open and ready (S12.1 otherwise).
    public private(set) var collectionOpen = false
    /// The page is on screen: it refreshes, and live refreshes run (S11).
    public private(set) var visible = false

    // MARK: Output

    /// What the page shows in place of its sections, or `.content`.
    public private(set) var state: StatsPageState = .noCollection
    /// Every box's content, always present (a box keeps its place, S8.5). Not observed as a
    /// whole: each box's view observes its own cell (`box(_:)`), so a part, a progress tick or a
    /// switch redraws only the boxes it changes (R2078). `boxesChanged` hears every change.
    @ObservationIgnored public private(set) var boxes: [StatsBoxID: StatsBoxContent] = [:] {
        didSet {
            guard boxes != oldValue else { return }
            for id in StatsBoxID.allCases {
                if let cell = cells[id], cell.content != boxes[id] { cell.content = boxes[id] }
            }
            boxesChanged?()
        }
    }
    public private(set) var strip: [StatsStripID: StatsStripFigure] = [:]
    /// "<n> cards in <m> decks" with S10.4's status; nil while the count is unknown.
    public private(set) var subtitle: String?
    /// S12.6's line under the strip for a filtered deck.
    public private(set) var filteredDeckNote: String?
    /// Each box's selection, carried across a refresh and cleared by a switch or the period (S8.3).
    /// Not observed as a whole either: a hover over one chart redraws that chart alone.
    @ObservationIgnored public private(set) var selections: [StatsBoxID: StatsChartSelection] = [:] {
        didSet {
            for id in StatsBoxID.allCases {
                let selection = selections[id] ?? StatsChartSelection.none
                if let cell = cells[id], cell.selection != selection { cell.selection = selection }
            }
        }
    }
    /// Each box's content and selection, observed on their own.
    @ObservationIgnored private let cells = Dictionary(uniqueKeysWithValues: StatsBoxID.allCases.map { ($0, StatsBoxCell()) })
    /// Runs after any box's content changed (the page's intervals and `filled`).
    @ObservationIgnored public var boxesChanged: (@MainActor () -> Void)?

    /// The toolbar's controls respond (S12.1, S12.2 disable them).
    public var controlsEnabled: Bool { state != .noCollection && state != .noCards }

    /// The deck pop-up's rows after All Decks (S3.1).
    public var deckMenu: [StatsDeckDirectory.MenuItem] { decks.menu }

    public func box(_ id: StatsBoxID) -> StatsBoxContent {
        cells[id]?.content ?? skeletonContent(id, phase: .loading(fraction: nil))
    }
    public func strip(_ id: StatsStripID) -> StatsStripFigure { strip[id] ?? .loading }
    public func selection(for box: StatsBoxID) -> StatsChartSelection { cells[box]?.selection ?? .none }

    // MARK: Refresh state

    private struct Running {
        enum Stage { case single, allFirst, allSecond }
        let refresh: UInt64
        let scope: StatsPageScope
        let stage: Stage
        var set: StatsPartSet
        var progress: [StatsPart: StatsProgress] = [:]
        var task: Task<Void, Never>?
        var stream: StatsPageStream?
    }

    /// How the last refresh of this visit ended, for S10.4's status.
    private enum Ended { case none, completed, interrupted, failed }

    /// A page-level end of a request (S12.7, S12.8), for the scope it was read for.
    private enum PageEnd { case deckMissing, unavailable }

    @ObservationIgnored private var running: Running?
    @ObservationIgnored private var refreshes: UInt64 = 0
    @ObservationIgnored private var ended: Ended = .none
    @ObservationIgnored private var completedAt: Date?
    @ObservationIgnored private var pageEnd: (scope: StatsPageScope, end: PageEnd)?
    @ObservationIgnored private var live: StatsTimerToken?
    @ObservationIgnored private var justNow: StatsTimerToken?
    @ObservationIgnored private var lastBins: [StatsBoxID: [StatsPeriod]] = [:]
    /// The presenters of the sets shown, kept while the inputs they were made for hold: a set's
    /// axis, formatters and streaks are made once, not on every part.
    @ObservationIgnored private var presenters: [PresenterKey: StatsPresenter] = [:]
    @ObservationIgnored private var inputs: UInt64 = 0
    /// A refresh waits for the device's kept sets of its scope (S10.3 decides on them).
    @ObservationIgnored private var refreshAfterLoad = false
    /// The collection this model was told is open (the kept results are shared by every window).
    @ObservationIgnored private var openedIdentity: String?

    private struct PresenterKey: Hashable {
        let set: UUID
        let parts: Int
        let inputs: UInt64
        let extras: [UUID]
    }

    // MARK: Seams

    @ObservationIgnored private let service: any StatsService
    @ObservationIgnored public let kept: StatsKeptResults
    @ObservationIgnored private let store: StatsSelectionStore
    @ObservationIgnored private let timers: any StatsTimers
    @ObservationIgnored private let calendar: Calendar
    @ObservationIgnored private let locale: Locale
    @ObservationIgnored private var text: StatsText { StatsText(locale: locale) }

    /// S11: a visible page refreshes this long after the last change.
    static let liveDelay = Duration.milliseconds(500)
    /// S10.4: "Updated just now" lasts a minute.
    static let justNowSpan: TimeInterval = 60

    /// - Parameters:
    ///   - selection: the window's restored selection (scene state); nil starts from the
    ///     device's last one (S3.3).
    ///   - kept: the kept results, shared by every window.
    public init(service: any StatsService, kept: StatsKeptResults, store: StatsSelectionStore,
                selection: StatsPageSelection? = nil, timers: any StatsTimers = StatsSystemTimers(),
                calendar: Calendar = .autoupdatingCurrent, locale: Locale = .autoupdatingCurrent) {
        self.service = service
        self.kept = kept
        self.store = store
        self.timers = timers
        self.calendar = calendar
        self.locale = locale
        self.selection = selection ?? store.load()
        rebuild()
    }

    isolated deinit {
        kept.show(nil, by: ObjectIdentifier(self))
        // A window that goes away without leaving the page still ends its request.
        running?.task?.cancel()
        if let stream = running?.stream { Task.detached { await stream.cancel() } }
    }

    // MARK: Inputs from the shell

    /// A collection is open and ready; `identity` names it, so kept results never cross
    /// collections (S10.2).
    public func collectionOpened(identity: String) {
        let changed = !collectionOpen || kept.collection != identity
        // Another collection in place of the open one: its deck tree is not this one's.
        if collectionOpen, openedIdentity != identity { forgetDecks() }
        openedIdentity = identity
        collectionOpen = true
        kept.open(collection: identity)
        kept.show(selection.scope, by: ObjectIdentifier(self))
        guard changed else { return }
        cancelRunning()
        pageEnd = nil
        ended = .none
        completedAt = nil
        prepareKept()
        rebuild()
        if visible { refresh() }
    }

    /// No collection is open (the welcome screen, an opening, a failed open): S12.1.
    public func collectionClosed() {
        cancelRunning()
        live?.cancel()
        live = nil
        collectionOpen = false
        openedIdentity = nil
        kept.show(nil, by: ObjectIdentifier(self))
        kept.close()
        // The next collection's tree is read when it opens; until then no deck is known, so the
        // pop-up never offers the closed collection's decks (the page shows S12.1 meanwhile).
        forgetDecks()
        rebuild()
    }

    private func forgetDecks() {
        guard decks != .unknown else { return }
        decks = .unknown
        inputs &+= 1
    }

    /// The deck list's tree with its counts (C5), each time it is read.
    public func decksChanged(_ nodes: [CountedDeckNode]) {
        let next = StatsDeckDirectory(nodes: nodes)
        guard next != decks else { return }
        decks = next
        inputs &+= 1
        // A deck restored (an undo, a sync) is shown again.
        if let pageEnd, pageEnd.end == .deckMissing, case let .deck(id) = pageEnd.scope, next.deck(id) != nil {
            self.pageEnd = nil
            if visible, pageEnd.scope == selection.scope { refresh() }
        }
        rebuild()
    }

    /// The page appears: every open refreshes (R1294, S11).
    public func appear() {
        visible = true
        prepareKept()
        refresh()
    }

    /// The page is left: its request is cancelled (H16) and what it showed is kept.
    public func disappear() {
        visible = false
        live?.cancel()
        live = nil
        if running != nil {
            cancelRunning()
            ended = .interrupted
        }
        kept.persist(selection.scope)
        rebuild()
    }

    /// G10's staleness: a visible page refreshes half a second after the last one (S11); a
    /// hidden page refreshes when it appears. `CollectionReplaced` drops the kept results.
    public func receive(_ staleness: StatsStaleness) {
        if staleness == .collectionReplaced {
            cancelRunning()
            kept.collectionReplaced()
            pageEnd = nil
            ended = .none
            completedAt = nil
            rebuild()
        }
        guard visible, collectionOpen else { return }
        live?.cancel()
        live = timers.schedule(after: Self.liveDelay) { [weak self] in
            self?.live = nil
            self?.refresh()
        }
    }

    /// A memory warning: the core's caches go (G9.5, H14.4) through `StatsCacheSignals`; the
    /// model keeps only the shown scope's kept sets in memory.
    public func memoryWarning() {
        kept.trim()
        // The presenters hold their sets; the next rebuild makes the shown ones again.
        presenters = [:]
    }

    // MARK: Controls

    /// The deck pop-up, a By Deck row or a way in from a deck (S3.1, S13): the period stays.
    public func selectDeck(_ id: Int64?) {
        guard id != selection.deck else { return }
        cancelRunning()
        selection.deck = id
        store.save(selection)
        if collectionOpen { kept.show(selection.scope, by: ObjectIdentifier(self)) }
        inputs &+= 1
        selections = [:]
        lastBins = [:]
        pageEnd = nil
        ended = .none
        completedAt = nil
        prepareKept()
        rebuild()
        if visible { refresh() }
    }

    /// S3.2: a change among 1M to 1Y re-bins what is there; a change to or from All is a new
    /// refresh (S10.5).
    public func selectPeriod(_ period: StatsPagePeriod) {
        let previous = selection.period
        guard period != previous else { return }
        selection.period = period
        store.save(selection)
        inputs &+= 1
        selections = [:]
        if period.needsNewRequest(after: previous) {
            cancelRunning()
            rebuild()
            if visible { refresh() }
        } else {
            rebuild()
        }
    }

    public func setReviewsMode(_ mode: StatsReviewsMode) {
        guard mode != selection.reviewsMode else { return }
        selection.reviewsMode = mode
        switched(.reviewsOverTime)
    }

    public func setReviewsMeasure(_ measure: StatsReviewsMeasure) {
        guard measure != selection.reviewsMeasure else { return }
        selection.reviewsMeasure = measure
        switched(.reviewsOverTime)
    }

    public func setRetentionKind(_ kind: StatsRetentionKind) {
        guard kind != selection.retentionKind else { return }
        selection.retentionKind = kind
        switched(.retention)
    }

    private func switched(_ box: StatsBoxID) {
        store.save(selection)
        inputs &+= 1
        selections[box] = nil
        rebuild()
    }

    /// The page started to scroll: a hovered bar is let go (R2078), a pinned one stays.
    public func endHover() {
        let next = selections.filter(\.value.pinned)
        if next != selections { selections = next }
    }

    /// The box's selection, from the chart (pointer, touch, keys).
    public func select(_ selection: StatsChartSelection, in box: StatsBoxID) {
        selections[box] = selection == .none ? nil : selection
    }

    /// A click outside every chart: no box keeps a pinned bar (S8.3).
    public func unpinAll() {
        let next = selections.filter { !$0.value.pinned }
        if next != selections { selections = next }
    }

    // MARK: Refresh

    /// Starts a refresh of the scope and period on screen, cancelling the one before (H16).
    public func refresh() {
        guard collectionOpen, visible else { return }
        live?.cancel()
        live = nil
        let scope = selection.scope
        if decks.isMissing(scope) {
            cancelRunning()
            rebuild()
            return
        }
        cancelRunning()
        // S10.3 decides on what is kept: wait for the device's sets of the scope.
        if kept.isLoading(scope) {
            refreshAfterLoad = true
            return
        }
        refreshAfterLoad = false
        // S10.3: All with nothing kept reads the last year first, then all history.
        if selection.period == .all, !kept.hasComplete(scope, .allHistory), !kept.hasComplete(scope, .last12Months) {
            start(scope: scope, period: .oneYear, stage: .allFirst)
        } else {
            start(scope: scope, period: selection.period, stage: .single)
        }
    }

    private func start(scope: StatsPageScope, period: StatsPagePeriod, stage: Running.Stage) {
        refreshes += 1
        let refresh = refreshes
        let request = StatsPageRequest(scope: scope, period: period, parts: StatsPart.allCases, refresh: refresh)
        running = Running(refresh: refresh, scope: scope, stage: stage,
                          set: StatsPartSet(scope: scope, history: period.history, refresh: refresh))
        let service = service
        running?.task = Task { [weak self] in
            let stream: StatsPageStream
            do throws(BridgeError) {
                stream = try await service.page(request)
            } catch {
                self?.finish(refresh, .failed(error))
                return
            }
            guard self?.running?.refresh == refresh else {
                await stream.cancel()
                return
            }
            self?.running?.stream = stream
            var ended = false
            for await message in stream {
                // Held only for one message: a window that goes away is not kept by its request.
                guard let model = self, model.running?.refresh == refresh else { break }
                switch message {
                case let .part(result): model.received(result, refresh: refresh)
                case let .progress(part, progress): model.progressed(part, progress, refresh: refresh)
                case let .end(end):
                    ended = true
                    model.finish(refresh, end)
                }
            }
            // A stream left before its end (the window went away) cancels its request.
            if !ended { await stream.cancel() }
        }
        rebuild()
    }

    private func cancelRunning() {
        guard let running else { return }
        self.running = nil
        running.task?.cancel()
        if let stream = running.stream { Task { await stream.cancel() } }
    }

    private func received(_ result: StatsPartResult, refresh: UInt64) {
        guard var current = running, current.refresh == refresh else { return }
        guard current.set.receive(result) else { return }
        current.progress[result.part] = nil
        running = current
        kept.keep(current.set)
        // A part returned: the scope reads again (a repaired row, a restored deck).
        if pageEnd?.scope == current.scope { pageEnd = nil }
        rebuild(after: result.part)
    }

    private func progressed(_ part: StatsPart, _ progress: StatsProgress, refresh: UInt64) {
        guard running?.refresh == refresh, running?.set.parts[part] == nil else { return }
        running?.progress[part] = progress
        updateLoadingPhases()
    }

    private func finish(_ refresh: UInt64, _ end: StatsPageEnd) {
        guard let current = running, current.refresh == refresh else { return }
        running = nil
        var set = current.set
        switch end {
        case .complete:
            set.complete = true
            if pageEnd?.scope == current.scope { pageEnd = nil }
            kept.keep(set)
            kept.persist(current.scope)
            if current.stage == .allFirst, selection.period == .all, selection.scope == current.scope, visible {
                start(scope: current.scope, period: .all, stage: .allSecond)
                return
            }
            ended = .completed
            completedAt = timers.now()
            justNow?.cancel()
            justNow = timers.schedule(after: .seconds(Self.justNowSpan)) { [weak self] in
                self?.justNow = nil
                self?.rebuild()
            }
        case .contextChanged:
            // The parts that returned hold for their context; the page reads again (G10).
            kept.keep(set)
            kept.persist(current.scope)
            ended = .interrupted
            rebuild()
            if live == nil, visible { self.refresh() }
            return
        case .cancelled:
            ended = .interrupted
        case .scopeError(.unknownDeck):
            pageEnd = (current.scope, .deckMissing)
            ended = .failed
        case .scopeError(.search), .unreadableRow:
            pageEnd = (current.scope, .unavailable)
            ended = .failed
        case .failed:
            kept.keep(set)
            ended = .failed
        }
        rebuild()
    }

    private func prepareKept() {
        guard collectionOpen else { return }
        kept.prepare(selection.scope) { [weak self] in
            guard let self else { return }
            self.rebuild()
            if self.refreshAfterLoad, self.visible, !self.kept.isLoading(self.selection.scope) { self.refresh() }
        }
    }

    // MARK: Building the content

    /// The newest set of the scope that has `parts`, preferring one complete for the period.
    private func chosenSet(_ parts: [StatsPart]) -> StatsPartSet? {
        let usable = kept.sets(for: selection.scope).filter { $0.has(parts) }
        return usable.first { isComplete($0, parts) } ?? usable.first
    }

    /// A `last12Months` set is partial on All's axis for the parts that depend on the window.
    private func isComplete(_ set: StatsPartSet, _ parts: [StatsPart]) -> Bool {
        selection.period != .all || set.history == .allHistory || !parts.contains(where: \.dependsOnWindow)
    }

    /// The newest set of the scope with data for `part`: a box's extra value (Retrievability's
    /// desired retention, Reviews' streak) when its own set lacks it.
    private func extra(_ part: StatsPart) -> StatsPartSet? {
        kept.sets(for: selection.scope).first { $0.value(part) != nil }
    }

    private func presenter(_ set: StatsPartSet) -> StatsPresenter? {
        let extras = [StatsPart.retentionByDay, .activityDays].compactMap { set.value($0) == nil ? extra($0) : nil }
        let key = PresenterKey(set: set.id, parts: set.parts.count, inputs: inputs, extras: extras.map(\.id))
        if let cached = presenters[key] { return cached }
        let made = StatsPresenter(set: set, period: selection.period, selection: selection, decks: decks, calendar: calendar,
                                  locale: locale, extras: extras)
        if presenters.count > 24 { presenters = [:] }
        presenters[key] = made
        return made
    }

    /// The progress the running request reports for the parts of a box it has not returned.
    private func fraction(_ parts: [StatsPart]) -> Double? {
        guard let running else { return nil }
        let waiting = parts.filter { running.set.parts[$0] == nil }
        var fractions: [Double] = []
        for part in waiting {
            switch running.progress[part] {
            case let .fraction(value)?: fractions.append(min(max(value, 0), 1))
            case .indeterminate?: return nil
            case nil: continue
            }
        }
        return fractions.min()
    }

    private func skeletonContent(_ id: StatsBoxID, phase: StatsBoxPhase) -> StatsBoxContent {
        let kit = StatsSeriesKit(text: text)
        var content = StatsBoxContent(phase: phase, data: kit.skeleton(id, selection: selection),
                                      figures: kit.skeletonFigures(id, selection: selection))
        if id == .activity, case .loading = phase, let years = activityYears() { content.detail = .activityYears(years) }
        return content
    }

    /// Activity's year blocks from the running request's header (H9's oldest review: every year
    /// back to it, the current one included), before `activity_days` returns (S10.3).
    private func activityYears() -> Int? {
        guard let running, running.scope == selection.scope, let header = running.set.header else { return nil }
        guard case let .day(oldest) = header.oldestDay else { return nil }
        let days = StatsDays(context: header.context, calendar: calendar)
        return max(1, days.year(ofDay: 0) - days.year(ofDay: min(oldest, 0)) + 1)
    }

    private func content(_ id: StatsBoxID) -> StatsBoxContent {
        let parts = id.parts(reviewsMode: selection.reviewsMode)
        guard collectionOpen, let set = chosenSet(parts), let presenter = presenter(set) else {
            // Nothing for this box yet: its progress while a request runs, else S12.8's line
            // after a request that failed.
            let phase: StatsBoxPhase = running == nil && ended == .failed ? .failed : .loading(fraction: fraction(parts))
            return skeletonContent(id, phase: phase)
        }
        let updating = running.map { $0.set.id != set.id } ?? false
        if parts.contains(where: { if case .failed = set.parts[$0] { true } else { false } }) {
            var failed = skeletonContent(id, phase: .failed)
            failed.updating = updating
            failed.source = presenter.source
            return failed
        }
        switch presenter.box(id) {
        case .unavailable:
            var loading = skeletonContent(id, phase: unfinished(parts))
            loading.updating = updating
            return loading
        case var .content(content):
            if !isComplete(set, parts) {
                // Partial (S10.3's first refresh of All): the values stay in `data`, the box
                // shows its progress, never the year as all history.
                content.phase = unfinished(parts)
            }
            content.updating = updating
            return content
        }
    }

    /// A box whose value is not complete yet: its progress while a request runs, S12.8's line
    /// once the request that would complete it has failed (the partial values stay in `data`).
    private func unfinished(_ parts: [StatsPart]) -> StatsBoxPhase {
        running == nil && ended == .failed ? .failed : .loading(fraction: fraction(parts))
    }

    private func stripFigure(_ id: StatsStripID) -> StatsStripFigure {
        guard collectionOpen else { return .loading }
        if id == .remainingToday {
            // The deck list's counts are the app's; tomorrow's count comes with Reviews Due.
            if let figure = chosenSet([.futureDue]).flatMap(presenter)?.strip(id) { return figure }
            guard let remaining = decks.remaining(selection.scope) else { return .loading }
            let shown = remaining > 0 ? StatsChartFormat(locale: locale, calendar: calendar, now: timers.now()).count(Double(remaining))
                : text.none
            return StatsStripFigure(value: shown, unit: nil, detail: nil, loading: false, spoken: shown)
        }
        guard let set = chosenSet(id.parts) else { return .loading }
        if case .failed = set.parts[id.parts[0]] {
            return StatsStripFigure(value: text.noValue, unit: nil, detail: nil, loading: false, spoken: text.noValue)
        }
        return presenter(set)?.strip(id) ?? .loading
    }

    /// Writes every value from the sets, the period, the switches and the decks.
    /// Writes every value from the sets, the period, the switches and the decks; after a part,
    /// only the boxes and figures that read it (their "updating" mark aside, which a part never
    /// changes for another box).
    private func rebuild(after part: StatsPart? = nil) {
        let scope = selection.scope
        let nextState = pageState(scope)
        if nextState != state { state = nextState }
        let note: String? = {
            guard case let .deck(id) = scope, decks.deck(id)?.filtered == true else { return nil }
            return text.filteredDeckNote
        }()
        if note != filteredDeckNote { filteredDeckNote = note }
        var next = boxes
        // Activity while it has no detail yet: the first part's header sizes its years (S10.3).
        for id in StatsBoxID.allCases where part.map({ id.reads($0, reviewsMode: selection.reviewsMode) }) ?? true
            || (id == .activity && boxes[id]?.detail == nil) {
            let content = content(id)
            next[id] = content
            carrySelection(id, to: content)
        }
        if next != boxes { boxes = next }
        var figures = strip
        for id in StatsStripID.allCases where part.map({ id.parts.contains($0) || $0 == .futureDue }) ?? true {
            figures[id] = stripFigure(id)
        }
        if figures != strip { strip = figures }
        let line = subtitleText()
        if line != subtitle { subtitle = line }
    }

    private func carrySelection(_ id: StatsBoxID, to content: StatsBoxContent) {
        guard content.phase == .ready else { return }
        let bins = content.data.bins
        if let selection = selections[id] {
            let carried = selection.carried(from: lastBins[id] ?? [], to: bins)
            let next: StatsChartSelection? = carried == .none ? nil : carried
            if next != selection { selections[id] = next }
        }
        lastBins[id] = bins
    }

    /// Progress alone moves only the boxes that are loading.
    private func updateLoadingPhases() {
        var next = boxes
        for (id, content) in boxes {
            guard case .loading = content.phase else { continue }
            var changed = content
            changed.phase = .loading(fraction: fraction(id.parts(reviewsMode: selection.reviewsMode)))
            next[id] = changed
        }
        if next != boxes { boxes = next }
        var figures = strip
        for id in StatsStripID.allCases where strip[id]?.loading != false { figures[id] = stripFigure(id) }
        if figures != strip { strip = figures }
    }

    private func pageState(_ scope: StatsPageScope) -> StatsPageState {
        guard collectionOpen else { return .noCollection }
        if decks.isMissing(scope) { return .deckMissing }
        if let pageEnd, pageEnd.scope == scope {
            return pageEnd.end == .deckMissing ? .deckMissing : .unavailable
        }
        if scope == .allDecks, let set = chosenSet([.cardCounts]), presenter(set)?.cardTotal == 0 { return .noCards }
        return .content
    }

    // MARK: Subtitle (S3.1, S10.4)

    private func subtitleText() -> String? {
        guard collectionOpen, let set = chosenSet([.cardCounts]), let presenter = presenter(set),
              let cards = presenter.cardTotal, let decks = decks.deckCount(selection.scope) else { return nil }
        let format = StatsChartFormat(locale: locale, calendar: calendar, now: timers.now())
        let line = text.cardsInDecks(cards, format.count(Double(cards)), decks, format.count(Double(decks)))
        guard let status = statusText(format) else { return line }
        return text.withStatus(line, status)
    }

    private func statusText(_ format: StatsChartFormat) -> String? {
        let shown = StatsBoxID.allCases.compactMap { boxes[$0]?.source }
        if running != nil, !shown.isEmpty { return text.updating }
        if ended == .completed, let completedAt, timers.now().timeIntervalSince(completedAt) < Self.justNowSpan {
            return text.updatedJustNow
        }
        if ended == .interrupted || ended == .failed {
            // The shown numbers are kept ones: the time they were read.
            let readAt = shown.map { Date(timeIntervalSince1970: TimeInterval($0.context.nowMilliseconds) / 1_000) }.min()
            guard let readAt else { return nil }
            let now = timers.now()
            let written = calendar.isDate(readAt, inSameDayAs: now)
                ? readAt.formatted(Date.FormatStyle(date: .omitted, time: .shortened, locale: locale, calendar: calendar,
                                                    timeZone: calendar.timeZone))
                : format.shortDate(readAt)
            return text.updated(written)
        }
        return nil
    }
}

/// One box's content and selection (`StatsPageModel.box(_:)`, `selection(for:)`): its own
/// observable, so the view that reads one box is redrawn only when that box changes.
@Observable @MainActor
final class StatsBoxCell {
    var content: StatsBoxContent?
    var selection: StatsChartSelection = .none
}

extension StatsBoxContent {
    var isLoading: Bool {
        if case .loading = phase { return true }
        return false
    }
}
