import MeguriCoreBindings
import Testing

@testable import CoreBridge

@Suite struct CancelTests {
    @Test func aLongCallCancelledAfter50msReturnsCancelledWithin100ms() async throws {
        let token = try await CoreBridge.CancelToken.make()
        let clock = ContinuousClock()
        let loop = Task { () -> (Result<UInt64, BridgeError>, ContinuousClock.Instant) in
            let result: Result<UInt64, BridgeError>
            do throws(BridgeError) {
                // Up to 10 s of 1 ms steps unless cancelled.
                result = .success(
                    try await MaintenanceLane.shared.call("test.cancellableLoop") {
                        try testCancellableLoop(token: token.raw, maxIterations: 10_000)
                    })
            } catch {
                result = .failure(error)
            }
            return (result, clock.now)
        }
        try await Task.sleep(for: .milliseconds(50))
        let cancelledAt = clock.now
        try await token.cancel()
        let (result, returnedAt) = await loop.value
        #expect(throws: BridgeError.cancelled) { try result.get() }
        let latency = returnedAt - cancelledAt
        #expect(latency < .milliseconds(100), "returned \(latency) after cancel")
        #expect(try await token.isCancelled())
    }

    @Test func swiftTaskCancellationAloneDoesNotCancelTheCall() async throws {
        let token = try await CoreBridge.CancelToken.make()
        let loop = Task {
            try await MaintenanceLane.shared.call("test.cancellableLoop") {
                try testCancellableLoop(token: token.raw, maxIterations: 30)
            }
        }
        loop.cancel()
        // The Rust loop ran to its end: only the explicit token cancels it.
        #expect(try await loop.value == 30)
        #expect(try await !token.isCancelled())
    }
}
