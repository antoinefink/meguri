@testable import CoreBridge
import Testing
@testable import AppModel

/// N16.11: a leech answer's notice is shown as one icon in the top controls,
/// for exactly as long as the notice lasts (`review/reviewer-strings#S3`).
extension ReviewCoordinatorTests {
    @Test(arguments: [false, true])
    func leechAnswerIsShownAsTheIcon(suspended: Bool) async throws {
        let fixture = Fixture()
        await fixture.model.start()
        #expect(fixture.model.leechNotice == nil)
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.head(2), marks: [], removed: [1]),
            leech: true, suspended: suspended, rolledOver: false)
        fixture.model.rate(.again); await fixture.model.settled()
        // The answer path's own text maps to the icon and its variant.
        let notice = try #require(fixture.model.leechNotice)
        #expect(notice.suspended == suspended)
        // Another notice is its own banner, and the icon goes.
        fixture.model.showNotice("Another notice")
        #expect(fixture.model.leechNotice == nil)
        #expect(await fixture.model.end())
    }

    @Test func answersThatAreNotLeechesShowNoIcon() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.again); await fixture.model.settled()
        #expect(fixture.model.leechNotice == nil)
        #expect(ReviewLeechNotice(notice: "Card changed, please answer again") == nil)
        #expect(await fixture.model.end())
    }
}
