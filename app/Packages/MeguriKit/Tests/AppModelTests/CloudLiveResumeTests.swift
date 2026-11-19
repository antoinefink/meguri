#if os(macOS)
@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func activeCoreResumeAndLocalReviewFinishWithOriginalSDKResultHeld(media: Bool) async throws {
        let seed = try InitialCloudFixture.load()
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-live-resume-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let serviceHome = root.appending(path: "service")
        try FileManager.default.createDirectory(at: serviceHome, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        let process = try await CloudServiceProcess.open(stateDirectory: serviceHome, transport: .binaryAssetsV1)
        let network = LiveResumeGate(), account = LiveResumeGate()
        var active: ServiceAppHarness?
        var local: Task<Void, Never>?, foreground: Task<Void, Never>?, drain: Task<Void, Never>?
        let probe = InitialCloudEvents()
        defer { ChangeFanout.shared.unsubscribe(probe); probe.finish() }
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let app = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire)
            active = app
            defer { app.defaults.removePersistentDomain(forName: app.suite) }
            try await wire.seed(seed)
            try await app.adopt()
            if app.owner.policy != .ok { await app.owner.enableFsrs() }
            let backend = try app.backend
            let sdk = try #require(await app.factory.engines.last)
            let backendCount = app.backends.count, sdkCount = await app.factory.engines.count
            let generation = app.owner.generation
            let references = ["retained-current.png", "retained-head.mp3"]
            app.sync.visibleMedia(references, generation: generation)
            await app.sync.waitForInputs()
            await app.sync.drainOperations() // baseline only, before installing either hold
            try #require(!app.sync.operationFailed)
            let originalGuard = try await backend.workflow.currentGuard()
            let reads = await app.factory.accountReadCount
            await sdk.holdNextSentResult(zone: media ? "meguri-media" : nil) { await network.hold() }
            let mediaBytes = try #require(Data(base64Encoded:
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aM1sAAAAASUVORK5CYII="))
            var registeredMediaName: String?
            if media {
                registeredMediaName = try await CoreMediaService().addMedia(name: "synthetic-live-resume.png", bytes: Array(mediaBytes))
            } else {
                let seedNote = try #require(seed.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
                let note = try await CoreNoteService().readNote(id: seedNote.id)
                var fields = note.fields; fields[0] += " Synthetic live-resume original send"
                _ = try await CoreNoteService().updateNote(.init(id: note.id, fields: fields, tags: note.tags))
            }
            try await CoreCollectionService().flushForBackground()
            app.sync.receive(.network(reachable: true)); app.sync.syncNow()
            try #require(try await serviceAppSignal(network.entered, stage: "actual original SDK Service result held"))
            let batch = try #require(await sdk.heldSentBatchID)
            let heldZone = try #require(await sdk.heldSentZone)
            let names = await sdk.heldSavedNames
            try #require(!names.isEmpty)
            var heldRevisions: [String: UInt64] = [:]
            if !media {
                for name in names {
                    let facts = try #require(try await backend.workflow.entityFacts(name: name))
                    heldRevisions[name] = facts.localRev
                    #expect(facts.ackedRev < facts.localRev)
                }
            }
            #expect(await sdk.deliveredSentBatches.filter { $0.id == batch && $0.zone == heldZone }.isEmpty)
            try await backend.workflow.validateSDK(sdk.instanceID)
            await app.factory.holdNextAccountRead { await account.hold() }
            try await ChangeFanout.shared.subscribe(probe)
            let beforeResume = probe.events.last?.seq ?? 0
            let localDone = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            local = Task { @MainActor in
                do {
                    let report = try await CoreCollectionService().resume()
                    #expect(!report.externalChange && !report.rolledOver)
                    func flatten(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] { nodes.flatMap { [$0] + flatten($0.children) } }
                    let deck = try #require(flatten(app.owner.decks).first { !$0.filtered && $0.counts.new > 0 })
                    await app.owner.startStudy(deckID: deck.id)
                    let review = try #require(app.owner.review)
                    try #require(review.phase == .question)
                    review.reveal(); await review.settled()
                    try #require(review.phase == .answer)
                    review.rate(.easy); await review.settled()
                    try #require(review.record.answers.count == 1)
                    await app.owner.finishReview()
                    try #require(app.owner.previousRecord?.answers.allSatisfy(\.durable) == true)
                    localDone.continuation.yield(true)
                } catch {
                    Issue.record(error); localDone.continuation.yield(false)
                }
                localDone.continuation.finish()
            }
            // A held real network operation must not become a pre-Core drain.
            try #require(try await serviceAppSignal(localDone.stream, stage: "active Core resume and local review before original network release"))
            await local?.value
            _ = try await probe.next(after: beforeResume, stage: "Available before foreground account proof") {
                if case .available = $0.kind { return true }; return false
            }
            #expect(await app.factory.accountReadCount == reads)
            let networkHeld = await network.isHeld, networkReleased = await network.isReleased
            #expect(networkHeld && !networkReleased)
            #expect(try await backend.workflow.currentGuard() == originalGuard)
            try await backend.workflow.validateSDK(sdk.instanceID)
            let priorityDone = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var completedReferences: [[String]] = []
            backend.afterExecution = { command, result in
                if case .prioritizeMedia(let refs) = command.action {
                    completedReferences.append(refs)
                    priorityDone.continuation.yield(result.error == nil && !result.deferred)
                }
                if case .pauseMedia = command.action { #expect(result.error == nil && !result.deferred) }
            }
            defer { backend.afterExecution = nil; priorityDone.continuation.finish() }
            // First deliver actual settings/visible work in Available's window,
            // before Swift foreground entry can perform any repair or gate.
            let windowReferences = ["available-current.png", "available-head.mp3"]
            app.sync.setDeviceSettings(paused: false, automatic: false)
            app.sync.visibleMedia(windowReferences, generation: generation)
            await app.sync.waitForInputs()
            try #require(try await serviceAppSignal(priorityDone.stream, stage: "Available settings and visible media ACK before Swift foreground entry"))
            #expect(completedReferences == [windowReferences])
            #expect(await app.factory.accountReadCount == reads)
            // Then keep the ordinary single foreground read unresolved across
            // the unchanged screen's replay and the original SDK result/ACK.
            foreground = Task { @MainActor in await app.sync.foregroundAfterLocalResume() }
            try #require(try await serviceAppSignal(account.entered, stage: "ordinary foreground account read held"))
            app.sync.setDeviceSettings(paused: false, automatic: false)
            await app.sync.waitForInputs()
            try #require(try await serviceAppSignal(priorityDone.stream, stage: "replayed visible media ACK before foreground proof"))
            #expect(completedReferences == [windowReferences, windowReferences])
            let accountHeld = await account.isHeld, accountReleased = await account.isReleased
            #expect(accountHeld && !accountReleased)
            #expect(try await backend.workflow.currentGuard() == originalGuard)
            try await backend.workflow.validateSDK(sdk.instanceID)
            #expect(!app.sync.operationFailed)
            #expect(await sdk.deliveredSentBatches.filter { $0.id == batch && $0.zone == heldZone }.isEmpty)
            await network.release()
            let drained = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            drain = Task { @MainActor in
                await app.sync.drainOperations()
                drained.continuation.yield(true); drained.continuation.finish()
            }
            try #require(try await serviceAppSignal(drained.stream, stage: "original SDK result and command ACK drain while account proof held"))
            await drain?.value
            #expect(await sdk.deliveredSentBatches.filter { $0.id == batch && $0.zone == heldZone }.count == 1)
            for (name, revision) in heldRevisions {
                let ack = try #require(try await backend.workflow.entityFacts(name: name))
                #expect(ack.ackedRev >= revision)
            }
            try await backend.workflow.validateSDK(sdk.instanceID)
            #expect(try await backend.workflow.currentGuard() == originalGuard)
            #expect(app.backends.count == backendCount && app.owner.generation == generation)
            let currentBackend = try app.backend
            #expect(currentBackend === backend)
            #expect(await app.factory.engines.count == sdkCount)
            #expect(await app.factory.accountReadCount == reads + 1)
            #expect(!app.sync.operationFailed)
            await account.release()
            await foreground?.value
            await app.sync.prepareConnectionAuthority()
            app.sync.visibleMedia(windowReferences, generation: generation)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            #expect(completedReferences == [windowReferences, windowReferences]) // unchanged-name dedup
            if let registeredMediaName {
                // MediaName can follow only the accepted original blob result.
                // A later exchange cannot hide a refused result by reuploading it.
                try await app.exchange()
                try await backend.waitForInstalledMedia(name: registeredMediaName, bytes: mediaBytes)
                let proof = try await serviceMediaProof(generation: generation)
                let tracked = try #require(proof.names.first { $0.name == registeredMediaName })
                let receipts = await wire.receipts
                let savedName = try #require(receipts.first { $0.matchesEntity(kind: 9, key: Data(tracked.caseKey.utf8)) })
                let ack = try #require(try await backend.workflow.entityFacts(name: savedName.record.name))
                #expect(ack.ackedRev == ack.localRev && ack.ackedRev > 0)
                for name in names { #expect(receipts.filter { $0.record.name == name }.count == 1) }
                #expect(await sdk.deliveredSentBatches.filter { $0.id == batch && $0.zone == heldZone }.count == 1)
            }
            #expect(!app.sync.operationFailed)
            try #require(await app.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
        } catch {
            await network.release(); await account.release()
            await local?.value; await foreground?.value; await drain?.value
            if let active { await active.cleanupAfterServiceFailure() }
            else { let exit = await process.cancel(); #expect(exit.joined) }
            throw error
        }
    }
}

private actor LiveResumeGate {
    nonisolated let entered: AsyncStream<Bool>
    private let continuation: AsyncStream<Bool>.Continuation
    private var waiter: CheckedContinuation<Void, Never>?
    private(set) var isHeld = false
    private(set) var isReleased = false
    init() { (entered, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1)) }
    func hold() async {
        isHeld = true
        continuation.yield(true)
        if !isReleased { await withCheckedContinuation { waiter = $0 } }
    }
    func release() { isReleased = true; waiter?.resume(); waiter = nil; continuation.finish() }
}
#endif
