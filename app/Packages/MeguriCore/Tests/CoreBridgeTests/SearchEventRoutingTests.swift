import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

/// R1084's second guard, on events made here: an event of a request older
/// than the newest one started is dropped, every other one passes in order.
@Test func searchEventsOfOlderRequestsAreDropped() async {
    let (stream, input) = AsyncStream.makeStream(of: MeguriCoreBindings.BrowseSearchEvent.self)
    let (events, output) = AsyncStream.makeStream(of: CoreBridge.BrowseSearchEvent.self)
    let newest = NewestRequest()
    newest.seq.withLock { $0 = 2 }
    input.yield(.cancelled(seq: 1))
    input.yield(.firstPage(seq: 2, handle: 9, ids: [5, -5]))
    input.yield(.failed(seq: 3, error: .core(error: .Browse(failure: .notComplete))))
    input.yield(.failed(seq: 4, error: .search(error: .init(kind: "invalid_state", provided: "bogus", context: nil,
                                                             message: "m", start: 3, end: 11))))
    input.finish()
    await CoreBridge.BrowseSearchSession.route(stream, newest: newest, to: output)
    var got: [CoreBridge.BrowseSearchEvent] = []
    for await event in events { got.append(event) }
    #expect(got == [.firstPage(seq: 2, handle: CoreBridge.BrowseHandle(rawValue: 9), ids: [5, -5]),
                    .failed(seq: 3, error: .browse(.notComplete)),
                    .failed(seq: 4, error: .browse(.search(CoreBridge.BrowseSearchError(
                        .init(kind: "invalid_state", provided: "bogus", context: nil, message: "m", start: 3, end: 11)))))])
}
