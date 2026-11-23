import CoreBridge
import Foundation
import Testing

@testable import Stats

/// The page shell (plan U296): its sections and grid (S5, N25.3), the strip's sections (S6),
/// the collection identity of the kept results (S10.2), and the sample collection the routes use.
@MainActor
@Suite struct StatsPageShellTests {
    @Test func theSectionsHoldEveryBoxOnceInPageOrder() {
        let boxes = StatsSectionID.allCases.flatMap(\.boxes)
        #expect(boxes == StatsBoxID.allCases)
        #expect(StatsSectionID.allCases.map(\.lead) == [.knowledgeOverTime, .reviewsOverTime, .cardsOverTime, .reviewsDue, nil, nil, .byDeck])
    }

    @Test func theStripScrollsToItsSections() {
        #expect(StatsStripID.allCases.map(\.section) == [.reviews, .knowledge, .comingUp, .reviews, .cards])
    }

    @Test(arguments: [(300.0, 1), (559.0, 1), (560.0, 2), (1_249.0, 2), (1_250.0, 3), (1_600.0, 3)])
    func theGridHasOneTwoOrThreeColumns(width: Double, columns: Int) {
        #expect(StatsBoxGrid.columns(for: width) == columns)
    }

    @Test func theIdentityIsKeptPerDeviceAndRenewedOnReplacement() throws {
        let suite = "StatsPageShellTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let first = StatsCollectionIdentity(defaults: defaults)
        let again = StatsCollectionIdentity(defaults: defaults)
        #expect(first.current == again.current, "A relaunch keeps the collection's identity")
        first.renew()
        #expect(first.current != again.current)
        #expect(StatsCollectionIdentity(defaults: defaults).current == first.current, "The renewed identity is kept")
        // It names nothing about the collection's file.
        #expect(UUID(uuidString: first.current) != nil)
        #expect(StatsCollectionIdentity(defaults: nil).current != StatsCollectionIdentity(defaults: nil).current)
    }

    private func samplePage(_ service: StatsSampleService, deck: Int64? = nil) -> StatsPage {
        let model = StatsPageModel(service: service, kept: StatsKeptResults(directory: nil), store: StatsSelectionStore(defaults: nil),
                                   selection: StatsPageSelection(deck: deck, period: .oneYear))
        model.collectionOpened(identity: "sample")
        model.decksChanged(StatsSampleService.decks)
        return StatsPage(model: model)
    }

    private func waitFilled(_ page: StatsPage) async {
        for _ in 0..<400 where !StatsPage.filled(page.model) {
            try? await Task.sleep(for: .milliseconds(5))
        }
    }

    @Test func theSampleCollectionFillsEveryBoxTierByTier() async {
        let page = samplePage(StatsSampleService(tierDelay: .milliseconds(1)))
        var reported: [Bool] = []
        page.filledChanged = { reported.append($0) }
        page.appear()
        #expect(!StatsPage.filled(page.model), "Nothing is kept: the boxes show their progress first (S10.3)")
        await waitFilled(page)
        page.boxesChanged()
        #expect(page.model.state == .content)
        #expect(StatsPage.filled(page.model))
        #expect(page.filled)
        #expect(reported.last == true)
        for id in StatsBoxID.allCases {
            #expect(page.model.box(id).phase != .failed, "\(id)")
        }
        #expect(page.model.subtitle?.isEmpty == false)
        page.disappear()
        #expect(!page.filled, "A page off screen is not filled")
    }

    @Test func aStalledTierKeepsItsBoxesLoadingAndTheEarlierOnesFilled() async {
        let page = samplePage(StatsSampleService(ending: .stall(.reviewLog), tierDelay: .milliseconds(1)))
        page.appear()
        for _ in 0..<400 where !StatsPage.filled(page.model, StatsPageTimings.presentBoxes) {
            try? await Task.sleep(for: .milliseconds(5))
        }
        #expect(StatsPage.filled(page.model, StatsPageTimings.presentBoxes))
        #expect(page.model.box(.reviewsOverTime).isLoading)
        #expect(page.model.box(.knowledgeOverTime).isLoading)
        #expect(!StatsPage.filled(page.model))
        page.disappear()
    }

    @Test func aDeckTheSampleDoesNotHaveIsGone() async {
        let page = samplePage(StatsSampleService(tierDelay: .milliseconds(1)), deck: 42)
        page.appear()
        #expect(page.model.state == .deckMissing)
        page.disappear()
    }

    @Test func goToSectionWaitsForThePageToShow() {
        let page = samplePage(StatsSampleService(tierDelay: .milliseconds(1)))
        page.go(to: .memory)
        #expect(page.requestedSection == nil)
        page.appear()
        page.go(to: .memory)
        #expect(page.requestedSection == .memory)
        page.disappear()
    }
}
