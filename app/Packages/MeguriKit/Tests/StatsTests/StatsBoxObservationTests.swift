import CoreBridge
import Foundation
import Observation
import Testing

@testable import Stats

/// R2078 (the owner's report of 2026-11-23, scrolling the drawn page): each box's view observes
/// its own box. A hover over one chart, a part that another box reads or a progress tick for a
/// loading box never invalidates the views of the boxes it leaves unchanged.
@MainActor
@Suite struct StatsBoxObservationTests {
    let base = StatsPageModelTests()

    /// Whether reading `read` registered an observation that `change` then fired.
    private func fires(_ read: () -> Void, after change: () -> Void) -> Bool {
        let fired = Fired()
        withObservationTracking(read) { fired.value = true }
        change()
        return fired.value
    }

    @Test func aSelectionInOneBoxLeavesTheOtherBoxesUnobserved() {
        let model = base.model()
        let read = { _ = model.box(.retention); _ = model.selection(for: .retention) }
        #expect(!fires(read) { model.select(StatsChartSelection(bin: 2), in: .forgotten) })
        #expect(fires(read) { model.select(StatsChartSelection(bin: 2), in: .retention) })
        #expect(model.selection(for: .retention) == StatsChartSelection(bin: 2))
        #expect(model.selections[.retention] == StatsChartSelection(bin: 2))
    }

    @Test func aPartRedrawsOnlyTheBoxesThatReadIt() async throws {
        let model = base.model()
        model.appear()
        let request = try await base.request(1)
        let present = { _ = model.box(.cardCounts) }
        let log = { _ = model.box(.reviewsOverTime) }
        let presentFired = Fired(), logFired = Fired()
        withObservationTracking(present) { presentFired.value = true }
        withObservationTracking(log) { logFired.value = true }
        base.service.answer(request, base.tier1)
        await settle { model.box(.cardCounts).phase == .ready }
        #expect(presentFired.value)
        #expect(!logFired.value)
        #expect(model.box(.reviewsOverTime).isLoading)
    }

    @Test func everyChangeOfTheBoxesIsHeard() async throws {
        let model = base.model()
        var heard = 0
        model.boxesChanged = { heard += 1 }
        model.appear()
        let request = try await base.request(1)
        await base.complete(request, model)
        #expect(heard > 0)
        #expect(StatsBoxID.allCases.allSatisfy { model.box($0) == model.boxes[$0] })
    }
}

nonisolated private final class Fired: @unchecked Sendable {
    nonisolated(unsafe) var value = false
}

/// S4's monthly bins keep every chart small at All (R2078): on the sample collection of three
/// years, every box filled, each chart draws at most 400 marks, counted as `StatsChartPlot`
/// draws them (a stacked segment with a value, a bar, a line point).
@MainActor
@Suite struct StatsMarkCountTests {
    @Test func everyChartDrawsAFewHundredMarksAtMostAtAll() async {
        let model = StatsPageModel(service: StatsSampleService(tierDelay: .zero), kept: StatsKeptResults(directory: nil),
                                   store: StatsSelectionStore(defaults: nil),
                                   selection: StatsPageSelection(deck: nil, period: .all))
        model.collectionOpened(identity: "sample")
        model.decksChanged(StatsSampleService.decks)
        model.appear()
        await settle { StatsBoxID.allCases.allSatisfy { model.box($0).phase == .ready } }
        var counts: [String] = []
        for id in StatsBoxID.allCases {
            let data = model.box(id).data
            let stacked = data.stackedSegments().filter { $0.upper > $0.lower }.count
            let single = data.series.filter { !$0.isStacked }.reduce(0) { $0 + $1.values.count { $0 != nil } }
            let marks = stacked + single
            counts.append("\(id.rawValue)=\(marks) (\(data.bins.count) bins)")
            #expect(marks <= 400, "\(id.rawValue) draws \(marks) marks")
        }
        print("stats.markCounts: " + counts.joined(separator: ", "))
    }
}
