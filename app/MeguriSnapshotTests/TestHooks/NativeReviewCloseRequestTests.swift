#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import Testing
@testable import Meguri

@MainActor @Suite struct NativeReviewCloseRequestTests {
    @Test func acknowledgmentFollowsCloseCompletion() async {
        var events = [String]()
        let request = TestHooks.NativeReviewCloseRequest(close: {
            events.append("close")
            await Task.yield()
            events.append("flushed")
        })
        await request.request { result in
            #expect(result == .saved)
            events.append("ack")
        }
        #expect(events == ["close", "flushed", "ack"])
    }

    @Test func errorAcknowledgmentIsObservable() async {
        let request = TestHooks.NativeReviewCloseRequest(close: { throw TestHookError("close failed") })
        var answers = [TestHooks.NativeReviewCloseRequest.Result]()
        await request.request { answers.append($0) }
        #expect(answers == [.failed])
    }

    @Test func ordinaryCollectionAndMissingHookAreInert() async {
        let request = TestHooks.NativeReviewCloseRequest()
        let collection = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation())
        request.bind(collection: collection)
        #expect(!request.enabled)
        var acknowledged = false
        await request.request { _ in acknowledged = true }
        #expect(!acknowledged)
        #expect(collection.state == .unopened)
        request.bind(collection: nil)
        await request.request { _ in acknowledged = true }
        #expect(!acknowledged)
    }
}
#endif
