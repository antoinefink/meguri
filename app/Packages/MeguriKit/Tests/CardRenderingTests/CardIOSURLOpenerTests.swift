#if os(iOS)
@testable import CardRendering
import Foundation
import Testing

extension WebKitSuites {
    @Suite struct CardIOSURLOpenerTests {
        @Test func undeclaredQueryIsUnknownAndPublicOpenOutcomeDecidesAttempt() throws {
            let url = try #require(URL(string: "synthetic-device-app://entry"))
            var queries = 0, attempts = 0
            var outcome: Bool?
            var reply: (@MainActor (Bool) -> Void)?
            let adapter = CardIOSURLOpener(queryEligible: { _ in false }, query: { _ in
                queries += 1
                return false // The false an undeclared OS query would return.
            }, performOpen: { received, completion in
                #expect(received == url)
                attempts += 1
                reply = completion
            })
            let native = NativeCardBehaviorPresenter()
            native.externalURLs = adapter
            #expect(native.availability(of: url) == .unknown)
            adapter.open(url) { outcome = $0 }
            #expect(queries == 0 && attempts == 1 && outcome == nil)
            reply?(true)
            #expect(outcome == true)
            outcome = nil
            adapter.open(url) { outcome = $0 }
            reply?(false)
            #expect(queries == 0 && attempts == 2 && outcome == false)
        }

        @Test func eligibleNegativeQueryAndFileRefuseButPositiveQueryStillUsesOpenOutcome() throws {
            let url = try #require(URL(string: "synthetic-listed-app://entry"))
            let file = try #require(URL(string: "file:///refused"))
            var installed = false, queries = 0, attempts = 0
            var outcome: Bool?
            let adapter = CardIOSURLOpener(queryEligible: { _ in true }, query: { _ in
                queries += 1
                return installed
            }, performOpen: { _, completion in attempts += 1; completion(false) })
            #expect(adapter.availability(of: url) == .unavailable)
            adapter.open(url) { outcome = $0 }
            #expect(attempts == 0 && outcome == false)
            installed = true
            #expect(adapter.availability(of: url) == .available)
            adapter.open(url) { outcome = $0 }
            #expect(attempts == 1 && outcome == false, "a successful query is not an OS open result")
            let beforeFile = queries
            adapter.open(file) { outcome = $0 }
            #expect(adapter.availability(of: file) == .unavailable && queries == beforeFile && attempts == 1 && outcome == false)
        }
    }
}
#endif
