@testable import CoreBridge
import Testing
@testable import AppModel

/// N16.11: a leech answer's notice is one icon in the top controls, not a
/// banner above the card; it lasts as the banner did (`review/reviewer-strings#S3`).
extension ReviewCoordinatorTests {
    @Test(arguments: [false, true])
    func leechAnswerShowsTheIconAndNoBanner(suspended: Bool) async throws {
        let fixture = Fixture()
        await fixture.model.start()
        #expect(fixture.model.leechNotice == nil)
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.head(2), marks: [], removed: [1]),
            leech: true, suspended: suspended, rolledOver: false)
        fixture.model.rate(.again); await fixture.model.settled()
        let notice = try #require(fixture.model.leechNotice)
        #expect(notice.suspended == suspended)
        #expect(fixture.model.notice == nil, "no banner above the card")
        // The next answer that is not a leech keeps it, as the banner stayed.
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.head(3), marks: [], removed: [2]),
            leech: false, suspended: false, rolledOver: false)
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.model.leechNotice == notice)
        // Another notice replaces it, as it replaced the banner.
        fixture.model.showNotice("Another notice")
        #expect(fixture.model.leechNotice == nil && fixture.model.notice == "Another notice")
        #expect(await fixture.model.end())
    }

    @Test func answersThatAreNotLeechesShowNoIcon() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.again); await fixture.model.settled()
        #expect(fixture.model.leechNotice == nil && fixture.model.notice == nil)
        #expect(await fixture.model.end())
    }
}
