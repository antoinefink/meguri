#if os(iOS)
@testable import CardRendering
import Foundation
import Testing

extension WebKitSuites {
    @Suite struct CardIOSURLOpenerTests {
        @Test(arguments: ["synthetic-listed-app://entry", "synthetic-unlisted-app://entry", "Arbitrary+Device.27-App://entry"])
        func validSchemesStayUnknownAndPublicOpenCompletionDecidesAttempt(_ value: String) async throws {
            let url = try #require(URL(string: value))
            var attempts = 0, completions = 0
            var outcome: Bool?
            var reply: (@MainActor (Bool) -> Void)?
            let adapter = CardIOSURLOpener(performOpen: { received, completion in
                #expect(received == url)
                attempts += 1
                reply = completion
            })
            let native = NativeCardBehaviorPresenter()
            native.externalURLs = adapter
            #expect(adapter.availability(of: url) == .unknown)
            #expect(native.availability(of: url) == .unknown)
            #expect(attempts == 0 && completions == 0)
            for succeeded in [true, false] {
                outcome = nil
                reply = nil
                adapter.open(url) { outcome = $0; completions += 1 }
                #expect(attempts == completions + 1 && outcome == nil)
                await Task.yield()
                #expect(outcome == nil && attempts == completions + 1)
                let completion = try #require(reply)
                completion(succeeded)
                #expect(outcome == succeeded && completions == attempts)
                #expect(adapter.availability(of: url) == .unknown)
                #expect(native.availability(of: url) == .unknown)
            }
            #expect(attempts == 2 && completions == 2)
        }

        @Test func fileAndInvalidOrMissingSchemesRefuseWithoutNativeAttempt() throws {
            var attempts = 0
            let adapter = CardIOSURLOpener(performOpen: { _, completion in attempts += 1; completion(true) })
            let native = NativeCardBehaviorPresenter()
            native.externalURLs = adapter
            for value in ["file:///refused", "FILE:///refused", "relative/path", "//missing-scheme/entry",
                          "bad_scheme%3A//entry", String(repeating: "a", count: 129) + "://entry"] {
                let url = try #require(URL(string: value))
                var outcome: Bool?
                var completions = 0
                #expect(adapter.availability(of: url) == .unavailable)
                #expect(native.availability(of: url) == .unavailable)
                adapter.open(url) { outcome = $0; completions += 1 }
                #expect(outcome == false && completions == 1 && attempts == 0)
            }
        }
    }
}
#endif
