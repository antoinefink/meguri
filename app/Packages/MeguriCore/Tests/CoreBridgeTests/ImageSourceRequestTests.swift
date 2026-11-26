import Dispatch
import Foundation
import Testing
@testable import CoreBridge

@Suite struct ImageSourceRequestTests {
    @Test func cancellationFencesAnImageWriteWaitingForItsLane() async throws {
        let entered = AsyncStream<Void>.makeStream()
        let release = DispatchSemaphore(value: 0)
        let holder = Task {
            try await InteractiveLane.shared.call("test.holdImageWrite") {
                entered.continuation.yield(())
                release.wait()
            }
        }
        defer { release.signal() }
        for await _ in entered.stream { break }
        let request = ImageSourceRequest()
        let write = Task { try await request.add(name: "never-written.svg", bytes: Data(), generation: 1) }
        request.cancel()
        release.signal()
        try await holder.value
        await #expect(throws: BridgeError.cancelled) { try await write.value }
        #expect(request.isCancelled)
        // No open host is needed: cancellation must precede binding/writer
        // admission, rather than be noticed only after the write returned.
    }
}
