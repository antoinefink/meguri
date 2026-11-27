import MeguriCoreBindings
import Testing

@testable import CoreBridge

@Suite struct ProgressRelayTests {
    @Test func newerCompletionSurvivesOlderAttachmentReplayBeforeIteration() async {
        let (stream, continuation) = AsyncStream<CoreProgress>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let relay = ProgressRelay(continuation)
        // Both callbacks arrive before Jobs.track returns and iteration starts.
        relay.onProgress(update: .init(sequence: 9, jobId: 37,
            routineSync: [.collectionStarted, .serverAcknowledged, .applyStarted, .applyCompleted],
            syncFilesLeft: nil, phase: .committing, etaMillis: nil, completed: 1, total: nil,
            collectionMediaPrepared: nil))
        relay.onProgress(update: .init(sequence: 1, jobId: 37, routineSync: [.collectionStarted],
            syncFilesLeft: nil, phase: .planning, etaMillis: nil, completed: 0, total: nil,
            collectionMediaPrepared: nil))
        // A phase callback captured before the first routine marker is stale too.
        relay.onProgress(update: .init(sequence: 0, jobId: 37, routineSync: [], syncFilesLeft: nil,
            phase: .planning, etaMillis: nil, completed: 0, total: nil, collectionMediaPrepared: nil))
        continuation.finish()
        var updates: [CoreProgress] = []
        for await update in stream { updates.append(update) }
        #expect(updates.count == 1)
        #expect(updates.first?.sequence == 9)
        #expect(updates.first?.routineSync == [.collectionStarted, .serverAcknowledged, .applyStarted, .applyCompleted])
    }

    @Test func entryTypedSnapshotCanRepeatTheGenericProgressSequence() async {
        let (stream, continuation) = AsyncStream<CoreProgress>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let relay = ProgressRelay(continuation)
        relay.onProgress(update: .init(sequence: 1, jobId: 71, routineSync: [], syncFilesLeft: nil,
            phase: .named(name: "media"), etaMillis: nil, completed: 0, total: nil, collectionMediaPrepared: nil))
        relay.onProgress(update: .init(sequence: 1, jobId: 71, routineSync: [], syncFilesLeft: nil,
            phase: .named(name: "media"), etaMillis: nil, completed: 0, total: nil,
            collectionMediaPrepared: .init(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1)))
        continuation.finish()
        var updates: [CoreProgress] = []
        for await update in stream { updates.append(update) }
        #expect(updates.count == 1 && updates.first?.collectionMediaPrepared?.added == 3)
    }
}
