#if DEBUG && MEGURI_TEST_HOOKS
import Foundation
import Dispatch
import Darwin

// Delivery notification only, with no polling timer, retry or sync deadline.
// Install before reading so a durable append during execution is not lost.
@MainActor final class CloudKitAgentDeliveries {
    let stream: AsyncStream<Void>
    private let source: any DispatchSourceFileSystemObject
    init(directory: URL) throws {
        let fd = open(directory.appendingPathComponent("commands.jsonl").path, O_EVTONLY | O_NOFOLLOW)
        guard fd >= 0 else { throw AgentFailure.storage }
        let events = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        stream = events.stream
        let continuation = events.continuation
        source = DispatchSource.makeFileSystemObjectSource(fileDescriptor: fd, eventMask: [.write, .extend, .delete, .rename], queue: .global())
        source.setEventHandler { continuation.yield(()) }
        source.setCancelHandler { _ = Darwin.close(fd); continuation.finish() }
        source.resume()
        continuation.yield(())
    }
    func cancel() { source.cancel() }
}
#endif
