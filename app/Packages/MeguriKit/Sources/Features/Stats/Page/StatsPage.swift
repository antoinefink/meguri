import CoreBridge
import Foundation
import Instrumentation
import Observation

/// One Statistics page (plan U296): its model, what the window's commands ask of it (View › Go
/// to Section, S5), whether a collection is still opening (S12.1), and the page's timing
/// intervals (S19). Held above the layout switch, for the app's run, by `StatsCollectionHost`, or
/// by a test-hook route with a sample model.
@Observable @MainActor
public final class StatsPage {
    public let model: StatsPageModel
    /// A section a command or a strip figure asked to scroll to; the page clears it once scrolled.
    public var requestedSection: StatsSectionID?
    /// The page is on screen: View › Go to Section is enabled (N25.11).
    public private(set) var shown = false
    /// A collection is opening (S12.1): the page shows nothing rather than "No Collection".
    public var opening = false
    @ObservationIgnored private let timings = StatsPageTimings()
    /// Whether every box showed its value at the last update (the test hooks' performance tests
    /// wait on it).
    public private(set) var filled = false
    /// Runs when `filled` changes.
    @ObservationIgnored public var filledChanged: (@MainActor (Bool) -> Void)?

    public init(model: StatsPageModel) {
        self.model = model
    }

    /// The page's views on screen: SwiftUI can show a new one before the old one goes (a
    /// rebuilt stack), so the page is shown from the first appearance to the last disappearance.
    @ObservationIgnored private var appearances = 0

    /// The page appeared: it refreshes (R1294), and its first frame is timed (S19).
    public func appear() {
        appearances += 1
        guard appearances == 1 else { return }
        shown = true
        model.appear()
        timings.shown(kept: Self.filled(model, settled: false))
        timings.refreshStarted()
        boxesChanged()
    }

    public func disappear() {
        appearances = max(0, appearances - 1)
        guard appearances == 0 else { return }
        shown = false
        model.disappear()
        timings.cancel()
        boxesChanged()
    }

    /// View › Go to Section, a strip figure (S5, S6).
    public func go(to section: StatsSectionID) {
        guard shown else { return }
        requestedSection = section
    }

    /// The deck pop-up: the interval starts before the model acts on the input.
    public func selectDeck(_ id: Int64?) {
        if shown, id != model.selection.deck { timings.scopeChanged() }
        model.selectDeck(id)
    }

    /// The period control: a change to or from All is a new refresh, one within the year a re-bin.
    public func selectPeriod(_ period: StatsPagePeriod) {
        let old = model.selection.period
        if shown, period != old {
            if period.needsNewRequest(after: old) { timings.scopeChanged() } else { timings.rebinned() }
        }
        model.selectPeriod(period)
    }

    /// The window's selection changed: a deck or a change to or from All starts a refresh, timed
    /// as a scope change (R1838); a switch or a period within the year only re-bins (R2079).
    func selectionChanged(from old: StatsPageSelection, to new: StatsPageSelection) {
        guard shown else { return }
        if old.deck != new.deck || new.period.needsNewRequest(after: old.period) {
            timings.scopeChanged()
        } else if old != new {
            timings.rebinned()
        }
        boxesChanged()
    }

    /// The boxes the model shows changed: the intervals whose boxes are all filled end.
    func boxesChanged() {
        timings.update(model)
        let now = shown && Self.filled(model)
        if now != filled {
            filled = now
            filledChanged?(now)
        }
    }

    /// Every box of `boxes` shows a value (or its state's line), and with `settled` none waits for a
    /// newer refresh.
    static func filled(_ model: StatsPageModel, _ boxes: [StatsBoxID] = StatsBoxID.allCases, settled: Bool = true) -> Bool {
        guard model.state == .content else { return model.state != .noCollection }
        return boxes.allSatisfy { id in
            let box = model.box(id)
            return !box.isLoading && !(settled && box.updating)
        }
    }
}

/// The page's intervals (`IntervalNames`, S19): each begins where the page asks and ends on the
/// first update that shows its boxes filled.
@MainActor
final class StatsPageTimings {
    private var open: IntervalToken?
    private var present: IntervalToken?
    private var log: IntervalToken?
    private var scope: IntervalToken?
    private var selection: IntervalToken?

    static let presentBoxes: [StatsBoxID] = StatsBoxID.allCases.filter { $0.parts(reviewsMode: .byType).allSatisfy { $0.tier == .present } }
    static let logBoxes: [StatsBoxID] = StatsBoxID.allCases.filter { $0.parts(reviewsMode: .byType).allSatisfy { $0.tier <= .reviewLog } }

    func shown(kept: Bool) {
        cancel()
        open = Signposts.begin(kept ? IntervalNames.statsKeptOpen : IntervalNames.statsColdOpen)
    }

    func refreshStarted() {
        present = present ?? Signposts.begin(IntervalNames.statsPresentTier)
        log = log ?? Signposts.begin(IntervalNames.statsLogTier)
    }

    func scopeChanged() {
        scope = scope ?? Signposts.begin(IntervalNames.statsScopeChange)
        refreshStarted()
    }

    func rebinned() {
        selection = selection ?? Signposts.begin(IntervalNames.statsSelection)
    }

    func update(_ model: StatsPageModel) {
        end(&present, when: StatsPage.filled(model, Self.presentBoxes))
        let logFilled = StatsPage.filled(model, Self.logBoxes)
        end(&log, when: logFilled)
        end(&scope, when: logFilled)
        // The page's first frame with every box shown, kept results included (R2076, R2077).
        end(&open, when: StatsPage.filled(model, settled: false))
        // A re-bin draws in the update it causes (S3.2: no core call).
        if let token = selection {
            selection = nil
            DispatchQueue.main.async { Signposts.end(token) }
        }
    }

    /// Leaving the page ends nothing: an interval it did not finish is dropped.
    func cancel() {
        open = nil
        present = nil
        log = nil
        scope = nil
        selection = nil
    }

    private func end(_ token: inout IntervalToken?, when filled: Bool) {
        guard filled, let started = token else { return }
        token = nil
        // The frame that shows the update follows it on the main queue.
        DispatchQueue.main.async { Signposts.end(started) }
    }
}
