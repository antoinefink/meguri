import AppModel
import CoreBridge
import Foundation
import SwiftUI
import Testing

@testable import Stats

/// S10.2, S10.5, S11, S19: count presenter evaluations on the page's normal paths,
/// rather than a separate cache benchmark. All records are synthetic. These counts exclude
/// Swift Charts/layout, which still need the VM performance suites.
@MainActor
@Suite struct StatsPresentationWorkTests {
    let base = StatsPageModelTests()

    private func keptModel(period: StatsPagePeriod = .all, parts: [StatsPart] = StatsPart.allCases,
                           history: StatsHistory = .allHistory) -> StatsPageModel {
        base.kept.open(collection: "collection-a")
        base.kept.keep(Fixture.set(parts, history: history))
        return base.model(StatsPageSelection(deck: nil, period: period))
    }

    private func cardTotal(_ model: StatsPageModel) -> String? {
        guard case let .cardCounts(table)? = model.box(.cardCounts).detail else { return nil }
        return table.total
    }

    @Test(arguments: [AppSection.home, .browse])
    func warmNavigationReusesEveryBoxAndStillRefreshesEveryVisit(via section: AppSection) async throws {
        let model = keptModel()
        let page = StatsPage(model: model)
        let navigation = AppNavigation()
        // A model over a kept set presents it once, before the first appearance.
        #expect(model.boxBuilds == StatsBoxID.allCases.count)
        #expect(model.synchronousBoxBuilds == StatsBoxID.allCases.count)
        let pin = StatsChartSelection(bin: 0, pinned: true)
        model.select(pin, in: .retention)
        let builds = model.boxBuilds
        let original = model.boxes
        for visit in 1...3 {
            navigation.select(.section(.stats))
            _ = StatisticsView(page: page, navigation: navigation).body
            #expect(base.service.requests.count == visit - 1)
            #expect(model.boxBuilds == builds, "constructing the destination starts no presenter")
            page.appear()
            let request = try await base.request(visit)
            #expect(request.period == .all && request.parts == StatsPart.allCases)
            #expect(StatsPage.filled(model, settled: false))
            #expect(StatsBoxID.allCases.allSatisfy { model.box($0).updating })
            #expect(model.boxBuilds == builds, "refresh start changes status, not the cached values")
            navigation.select(.section(section))
            page.disappear()
            await settle { base.service.cancelled.contains(request.refresh) }
            #expect(base.service.cancelled.contains(request.refresh))
            #expect(model.boxBuilds == builds, "leaving never re-presents unchanged records")
            #expect(model.selection(for: .retention) == pin)
            for id in StatsBoxID.allCases {
                #expect(model.box(id).data == original[id]?.data)
                #expect(!model.box(id).updating)
            }
        }
        print("stats.presentationWork: via=\(section.rawValue), initial=\(builds), warmVisits=3, additional=\(model.boxBuilds - builds)")
    }

    @Test func coldNavigationAndContentReadsDoNoBoxWorkBeforePartsArrive() async throws {
        let model = base.model()
        let page = StatsPage(model: model)
        let navigation = AppNavigation()
        for section in [AppSection.browse, .home, .stats] {
            navigation.select(.section(section))
        }
        _ = StatisticsView(page: page, navigation: navigation).body
        for id in StatsBoxID.allCases { _ = model.box(id) }
        #expect(base.service.requests.isEmpty && model.boxBuilds == 0)
        page.appear()
        let request = try await base.request(1)
        #expect(model.boxBuilds == 0)
        base.service.answer(request, [.cardCounts])
        await settle { model.box(.cardCounts).phase == .ready }
        #expect(model.boxBuilds == 1 && model.synchronousBoxBuilds == 0)
        #expect(model.box(.retention).isLoading)
        page.disappear()
    }

    @Test func freshAndRepeatedPartsInvalidatePresentationWithoutChangingTheirCount() async throws {
        let model = keptModel(period: .oneMonth)
        let page = StatsPage(model: model)
        page.appear()
        let request = try await base.request(1)
        let builds = model.boxBuilds
        let context = Fixture.context(now: (Fixture.nextDayAt - 30) * 1_000)
        base.service.answer(request, [.cardCounts], context: context,
                            values: [.cardCounts: .cardCounts(Fixture.cardCounts(new: 20))])
        await settle { cardTotal(model) == "110" }
        #expect(cardTotal(model) == "110")
        #expect(model.box(.cardCounts).source?.context == context)
        #expect(model.boxBuilds == builds + 1)
        // Same set, header and number of parts, but a new value must never hit the cache.
        base.service.answer(request, [.cardCounts], context: context,
                            values: [.cardCounts: .cardCounts(Fixture.cardCounts(new: 30))])
        await settle { cardTotal(model) == "120" }
        #expect(cardTotal(model) == "120")
        #expect(model.boxBuilds == builds + 2)
        #expect(model.synchronousBoxBuilds == StatsBoxID.allCases.count)
        page.disappear()
    }

    @Test func partialAllKeepsItsValuesAndUpdatesItsFailurePhaseWithoutRebinning() async throws {
        let model = keptModel(parts: [.cardCounts, .retentionByDay], history: .last12Months)
        let page = StatsPage(model: model)
        let partial = model.box(.retention)
        #expect(partial.isLoading && !partial.data.bins.isEmpty)
        let builds = model.boxBuilds
        page.appear()
        let request = try await base.request(1)
        #expect(request.period == .all)
        base.service.send(request.refresh, .end(.failed(.internal("synthetic"))))
        await settle { model.box(.retention).phase == .failed }
        #expect(model.box(.retention).phase == .failed)
        #expect(model.box(.retention).data == partial.data)
        #expect(model.box(.cardCounts).phase == .ready)
        #expect(model.boxBuilds == builds)
        page.disappear()
    }

    @Test func aChangedExtraPartInvalidatesTheKeptBoxThatUsesIt() async throws {
        let model = keptModel(period: .oneMonth, parts: base.tier1)
        let page = StatsPage(model: model)
        page.appear()
        let request = try await base.request(1)
        for desired in [Float(0.8), Float(0.7)] {
            let retention = StatsRetentionByDay(days: Fixture.map([0: Fixture.retentionDay(4, 1, 0, 0)]),
                                                retentions: [desired], several: false)
            base.service.answer(request, [.retentionByDay], values: [.retentionByDay: .retentionByDay(retention)])
            let detail = StatsBoxDetail.retrievability(desiredRetention: Double(desired))
            await settle { model.box(.retrievability).detail == detail }
            #expect(model.box(.retrievability).detail == detail)
        }
        page.disappear()
    }

    @Test func changedInputsAndCollectionReplacementCannotReuseOldPresentation() async throws {
        let model = keptModel(period: .oneMonth)
        let page = StatsPage(model: model)
        let builds = model.boxBuilds
        model.selectPeriod(.oneYear)
        #expect(model.boxBuilds > builds)
        #expect(model.box(.retention).data.bins.count == 52)
        let year = model.boxBuilds
        model.memoryWarning()
        page.appear()
        let old = try await base.request(1)
        await settle { model.box(.cardCounts).updating }
        #expect(model.boxBuilds > year, "memory pressure releases presentation too")
        model.receive(.collectionReplaced)
        model.collectionOpened(identity: "collection-b")
        let fresh = try await base.request(2)
        #expect(model.decks == .unknown)
        #expect(model.box(.cardCounts).source == nil)
        base.service.answer(old, StatsPart.allCases)
        await settle()
        #expect(model.box(.cardCounts).source == nil, "late work from the old collection stays fenced")
        base.service.answer(fresh, [.cardCounts], values: [.cardCounts: .cardCounts(Fixture.cardCounts(
            new: 0, learning: 0, relearning: 0, young: 0, mature: 0, suspended: 0, buried: 0, notes: 0))])
        await settle { model.state == .noCards }
        #expect(model.state == .noCards)
        page.disappear()
    }

    @Test func presentationVersionsDoNotChangeTheKeptFileFormat() throws {
        let set = Fixture.set()
        #expect(set.revision == UInt64(StatsPart.allCases.count))
        let encoded = try PropertyListEncoder().encode(set)
        let propertyList = try PropertyListSerialization.propertyList(from: encoded, format: nil)
        let object = try #require(propertyList as? [String: Any])
        #expect(object["revision"] == nil)
        #expect(object["presentationID"] == nil)
        #expect(object["presentation"] == nil)
        let decoded = try PropertyListDecoder().decode(StatsPartSet.self, from: encoded)
        #expect(decoded.id == set.id && decoded.parts == set.parts)
        #expect(decoded.revision == 0)
        #expect(StatsSetVersion(decoded) != StatsSetVersion(set))
        let decodedAgain = try PropertyListDecoder().decode(StatsPartSet.self, from: encoded)
        #expect(StatsSetVersion(decodedAgain) != StatsSetVersion(decoded))
        #expect(decodedAgain == decoded)
        #expect(Set([decoded, decodedAgain]).count == 1)
        // Receiving an identical part changes only the presentation revision.
        var repeated = decoded
        let header = try #require(decoded.header)
        let outcome = try #require(decoded.parts[.cardCounts])
        let accepted = repeated.receive(StatsPartResult(part: .cardCounts, header: header, outcome: outcome))
        #expect(accepted)
        #expect(StatsSetVersion(repeated) != StatsSetVersion(decoded))
        #expect(repeated == decoded)
        #expect(Set([decoded, decodedAgain, repeated]).count == 1)
    }
}
