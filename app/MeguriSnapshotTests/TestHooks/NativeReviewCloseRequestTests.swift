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

    #if os(macOS)
    private func phaseFields() -> [String: String] {
        ["version": "1", "case": "owner-case", "platform": "mac", "freezeSHA256": String(repeating: "a", count: 64),
            "freezeBytes": "100", "inputSHA256": String(repeating: "b", count: 64), "inputBytes": "200",
            "owner": UUID().uuidString, "nonce": UUID().uuidString, "phase": "accepted"]
    }
    private func phaseReply(_ object: String) throws -> String {
        var fields = try NativeReviewInput.PhaseTransport.decode(object)
        fields["data"] = Data("{ \"original\": true }\n".utf8).base64EncodedString()
        return try NativeReviewInput.PhaseTransport.encode(fields, reply: true)
    }

    @Test func phaseRequestDeliversOneExactBoundReply() async throws {
        let fields = phaseFields()
        let object = try NativeReviewInput.PhaseTransport.encode(fields)
        let expected = try phaseReply(object)
        let request = TestHooks.NativeReviewPhaseRequest(read: { received in
            #expect(received == object)
            return expected
        })
        var responses = [String]()
        await request.request(object) { responses.append($0) }
        #expect(responses == [expected])
        let decoded = try NativeReviewInput.PhaseTransport.decode(try #require(responses.first), reply: true)
        #expect(Data(base64Encoded: try #require(decoded["data"])) == Data("{ \"original\": true }\n".utf8))
    }

    @Test(arguments: ["case", "platform", "freezeSHA256", "freezeBytes", "inputSHA256", "inputBytes", "owner", "nonce", "phase", "extra", "duplicate", "oversize", "error"])
    func phaseRequestRefusesMismatchedAndInvalidReplies(change: String) async throws {
        let object = try NativeReviewInput.PhaseTransport.encode(phaseFields())
        var fields = try NativeReviewInput.PhaseTransport.decode(phaseReply(object), reply: true)
        switch change {
        case "case": fields[change] = "other-case"
        case "platform": fields[change] = "ipad"
        case "freezeSHA256", "inputSHA256": fields[change] = String(repeating: "c", count: 64)
        case "freezeBytes", "inputBytes": fields[change] = "1"
        case "owner", "nonce": fields[change] = UUID().uuidString
        case "phase": fields[change] = "closed"
        case "extra": fields["extra"] = "refuse"
        case "oversize": fields["data"] = Data(repeating: 0x61, count: 128 * 1024 + 1).base64EncodedString()
        default: break
        }
        var response = String(decoding: try JSONSerialization.data(withJSONObject: fields,
            options: [.sortedKeys, .withoutEscapingSlashes]), as: UTF8.self)
        if change == "duplicate" { response = "{\"nonce\":\"\(UUID().uuidString)\"," + response.dropFirst() }
        let frozenResponse = response
        let request = TestHooks.NativeReviewPhaseRequest(read: { _ in
            if change == "error" { throw TestHookError("Missing phase file") }
            return frozenResponse
        })
        var responses = [String]()
        await request.request(object) { responses.append($0) }
        #expect(responses.isEmpty)
    }

    @Test(arguments: ["cancel", "task-cancel", "rebind", "concurrent"])
    func phaseRequestRetiresInFlightWorkWithoutLateCallback(change: String) async throws {
        let object = try NativeReviewInput.PhaseTransport.encode(phaseFields())
        let response = try phaseReply(object)
        let suspended = SuspendedPhaseRead()
        let request = TestHooks.NativeReviewPhaseRequest(read: { _ in await suspended.read() })
        var responses = [String]()
        let pending = Task { await request.request(object) { responses.append($0) } }
        await suspended.waitUntilStarted()
        switch change {
        case "cancel": request.cancel()
        case "task-cancel": pending.cancel()
        case "rebind": request.bind(input: nil)
        default: await request.request(object) { responses.append($0) }
        }
        suspended.finish(response)
        await pending.value
        #expect(responses == (change == "concurrent" ? [response] : []))
    }

    @Test func phaseRequestsAreBoundedAndMissingHookIsInert() async throws {
        let object = try NativeReviewInput.PhaseTransport.encode(phaseFields())
        var reads = 0
        let request = TestHooks.NativeReviewPhaseRequest(read: { received in
            reads += 1; return try self.phaseReply(received)
        })
        var responses = 0
        for _ in 0..<5 {
            let fresh = try NativeReviewInput.PhaseTransport.encode(phaseFields())
            await request.request(fresh) { _ in responses += 1 }
        }
        #expect(reads == 4 && responses == 4)
        request.bind(input: nil)
        #expect(!request.enabled)
        await request.request(object) { _ in Issue.record("Retired request replied") }
        let inert = TestHooks.NativeReviewPhaseRequest()
        #expect(!inert.enabled)
        await inert.request(object) { _ in Issue.record("Missing hook replied") }
        #expect(reads == 4)
    }

    @MainActor private final class SuspendedPhaseRead {
        private var started = false
        private var waiter: CheckedContinuation<Void, Never>?
        private var result: CheckedContinuation<String, Never>?
        func read() async -> String {
            await withCheckedContinuation { continuation in
                result = continuation; started = true
                waiter?.resume(); waiter = nil
            }
        }
        func waitUntilStarted() async {
            if started { return }
            await withCheckedContinuation { waiter = $0 }
        }
        func finish(_ object: String) { result?.resume(returning: object); result = nil }
    }
    #endif

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
