#if os(macOS)
@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

// The existing suite serializes the process-global Core owner.
extension ReviewCollectionStartupTests {
    @Test
    func serviceSDKSavedRemovalRequiresOriginalHandlerReturnAndActualBookkeeping() async throws {
        try await withAutomaticServiceApp { app, fixture, wire in
            let sdk = try #require(await app.factory.engines.last)
            let zone = try #require(fixture.records.first?.zone)
            let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
            let handler = ServiceAutomaticGate()
            var original: CloudKitNetworkOperation?
            do {
                await sdk.holdNextSentHandlerReturn { await handler.hold() }
                try await automaticServiceEdit(app, id: seed.id, suffix: " Saved removal boundary")
                original = try await sdk.prepareSend(zones: [zone]).enqueue(validate: {})
                try #require(try await serviceAppSignal(handler.entered, stage: "original Saved handler held before SDK removal"))
                let request = try #require(await sdk.heldSentBatchID)
                let names = await sdk.heldSavedNames
                try #require(!names.isEmpty)
                let workflow = try app.backend.workflow
                let before = try await workflow.observations()
                let receipts = await wire.receipts
                for name in names {
                    let ack = try #require(try await workflow.entityFacts(name: name))
                    let actual = try #require(receipts.last { $0.zone == zone && $0.record.name == name })
                    try #require(ack.ackedRev == ack.localRev && ack.ackedPayloadDigest == ack.payloadDigest)
                    try #require(ack.ackedSystemFields == actual.record.systemFields)
                    try #require(await sdk.pendingRecordZoneChanges()[name] == .save)
                    try #require(!before.contains(.savedPendingRemoved(name: name, requestID: request)))
                }
                try #require(!before.contains(.handlerReturned(requestID: request)))
                await handler.release()
                try await original?.drain()
                try await sdk.waitForEventBookkeeping()
                let after = try await workflow.observations()
                let returned = try #require(after.firstIndex(of: .handlerReturned(requestID: request)))
                for name in names {
                    let removed = try #require(after.firstIndex(of: .savedPendingRemoved(name: name, requestID: request)))
                    try #require(returned < removed)
                    try #require(await sdk.pendingRecordZoneChanges()[name] == nil)
                }
            } catch {
                await handler.release()
                await original?.cancelAndDrain()
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func serviceSDKAutomaticSendUsesAdditionalOriginalBatchesForHandedBackRevision(automatic: Bool) async throws {
        try await withAutomaticServiceApp { app, fixture, wire in
            let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
            let first = ServiceAutomaticGate(), second = ServiceAutomaticGate()
            var manual: CloudKitNetworkOperation?
            var joined: Task<Void, Never>?
            do {
                if automatic {
                    app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
                    await app.sync.waitForInputs(); await app.sync.drainOperations()
                }
                let sdk = try #require(await app.factory.engines.last)
                await sdk.waitForAutomaticSend()
                let runsBefore = await sdk.automaticRuns
                await sdk.holdNextSentResult(zone: nil) { await first.hold() }
                try await automaticServiceEdit(app, id: seed.id, suffix: " First original revision")
                if !automatic {
                    let zone = try #require(fixture.records.first?.zone)
                    let prepared = try await sdk.prepareSend(zones: [zone])
                    manual = try prepared.enqueue(validate: {})
                }
                try #require(try await serviceAppSignal(first.entered, stage: "original Service reply held"))
                try #require(await sdk.automaticSchedulingEnabled == automatic)
                let firstBatch = try #require(await sdk.heldSentBatchID)
                let zone = try #require(await sdk.heldSentZone)
                let names = await sdk.heldSavedNames
                let bits = UInt64(bitPattern: seed.id)
                let prefix: [UInt8] = [8, 1, 16, 1, 26, 8] + (0..<8).reversed().map { UInt8(truncatingIfNeeded: bits >> ($0 * 8)) }
                let noteName = try #require(fixture.records.first { $0.encrypted.starts(with: prefix) }?.name)
                try #require(names.contains(noteName))
                let backend = try app.backend
                var firstRevisions: [String: UInt64] = [:]
                for name in [noteName] {
                    let facts = try #require(try await backend.workflow.entityFacts(name: name))
                    firstRevisions[name] = facts.localRev
                    try #require(facts.inFlight.contains { $0.localRev == facts.localRev })
                }
                // From here, only an ordinary durable local edit and its
                // TrackingChanged update may drive work. No second SDK call,
                // app RequestSend, Sync Now, or fabricated batch/ACK follows.
                var appNetworkRequests = 0
                backend.onCommands = { commands in
                    for command in commands {
                        switch command.action {
                        case .requestSend, .runExchange: appNetworkRequests += 1
                        default: break
                        }
                    }
                }
                defer { backend.onCommands = nil }
                try await automaticServiceEdit(app, id: seed.id, suffix: " Later dirty revision")
                if automatic {
                    await sdk.holdNextSentResult(zone: zone) { await second.hold() }
                }
                await first.release()
                if automatic {
                    try #require(try await serviceAppSignal(second.entered, stage: "automatic additional real Core batch and Service reply"))
                    let nextBatch = try #require(await sdk.heldSentBatchID)
                    try #require(nextBatch != firstBatch)
                    let nextNames = await sdk.heldSavedNames
                    let repeated = Set(names).intersection(nextNames)
                    try #require(repeated.contains(noteName))
                    for name in [noteName] {
                        let facts = try #require(try await backend.workflow.entityFacts(name: name))
                        try #require(facts.ackedRev == firstRevisions[name] && facts.localRev > facts.ackedRev)
                        try #require(facts.inFlight.contains { $0.localRev == facts.localRev })
                    }
                    await second.release()
                } else {
                    try await manual?.drain()
                }
                // Bounded wait on the original owned run. An empty-batch spin
                // cannot satisfy it; failure cleanup cancels and joins that run.
                let completion = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                joined = Task {
                    await sdk.waitForAutomaticSend()
                    completion.continuation.yield(true); completion.continuation.finish()
                }
                try #require(try await serviceAppSignal(completion.stream, stage: "automatic SDK original run joined without empty-batch spin"))
                await joined?.value
                await app.sync.waitForInputs()
                try #require(!app.sync.operationFailed)
                let removalObservations = try await backend.workflow.observations()
                for name in names {
                    try #require(removalObservations.contains(.savedPendingRemoved(name: name, requestID: firstBatch)))
                }
                let delivered = await sdk.deliveredSentBatches.filter { $0.zone == zone }
                try #require(delivered.contains { $0.id == firstBatch })
                let receipts = await wire.receipts
                for name in [noteName] {
                    let ack = try #require(try await backend.workflow.entityFacts(name: name))
                    let saved = receipts.filter { $0.zone == zone && $0.record.name == name }
                    if automatic {
                        try #require(delivered.count >= 2 && saved.count >= 2)
                        try #require(saved.suffix(2).first?.record.envelope != saved.last?.record.envelope)
                        try #require(ack.mark == nil && ack.inFlight.isEmpty && ack.ackedRev == ack.localRev)
                        try #require(ack.ackedSystemFields == saved.last?.record.systemFields)
                    } else {
                        try #require(delivered.count == 1)
                        try #require(ack.ackedRev == firstRevisions[name] && ack.localRev > ack.ackedRev)
                        try #require(ack.mark != nil && ack.inFlight.isEmpty)
                        try #require(await sdk.pendingRecordZoneChanges()[name] == .save)
                    }
                }
                try #require(appNetworkRequests == 0)
                try #require(await sdk.automaticRuns == runsBefore + (automatic ? 1 : 0))
                let requests = await sdk.batchRequests, empty = await sdk.emptyBatchRequests
                await sdk.waitForAutomaticSend()
                try await sdk.waitForEventBookkeeping()
                try #require(await sdk.batchRequests == requests)
                try #require(await sdk.emptyBatchRequests == empty)
                try #require(empty <= 4, "One terminal empty pass is bounded by the two admitted zones")
            } catch {
                await first.release(); await second.release()
                _ = await app.sync.cancelAndDrain()
                await manual?.cancelAndDrain()
                await joined?.value
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func serviceSDKTerminalEmptyPassRetainsOnlyNewRealPendingAddition(addition: Bool) async throws {
        try await withAutomaticServiceApp { app, fixture, wire in
            app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(await app.factory.engines.last)
            await sdk.waitForAutomaticSend()
            let terminal = ServiceAutomaticGate(), sent = ServiceAutomaticGate()
            var joined: Task<Void, Never>?
            var originalEmpty: CloudKitNetworkOperation?
            do {
                let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
                let backend = try app.backend
                let zone = try #require(fixture.records.first?.zone)
                await sdk.holdNextEmptyTerminalReturn { await terminal.hold() }
                let prepared = try await sdk.prepareSend(zones: [zone, "meguri-media"])
                originalEmpty = try prepared.enqueue(validate: {})
                try #require(try await serviceAppSignal(terminal.entered, stage: "original last nil batch terminal callback held"))
                let before = await sdk.deliveredSentBatches
                let requests = await sdk.batchRequests, empty = await sdk.emptyBatchRequests
                let runs = await sdk.automaticRuns, updates = await sdk.pendingAdditionUpdates
                let receipts = await wire.receipts.count
                var appNetworkRequests = 0
                backend.onCommands = { commands in
                    for command in commands {
                        switch command.action {
                        case .requestSend, .runExchange: appNetworkRequests += 1
                        default: break
                        }
                    }
                }
                defer { backend.onCommands = nil }
                if addition {
                    await sdk.holdNextSentResult(zone: zone) { await sent.hold() }
                    let added = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                    await sdk.inspectNextPendingAddition { added.continuation.yield(true) }
                    try await automaticServiceEdit(app, id: seed.id, suffix: " New addition during terminal callback")
                    try #require(try await serviceAppSignal(added.stream, stage: "normal pending addition applied during terminal callback"))
                    added.continuation.finish()
                    // Prove normal TrackingChanged reached this SDK before
                    // releasing nil's terminal bookkeeping, not merely Core.
                    try #require(await sdk.pendingAdditionUpdates > updates)
                }
                await terminal.release()
                if addition {
                    try #require(try await serviceAppSignal(sent.entered, stage: "new wake's actual Core batch and Service response"))
                    let actual = try #require(await sdk.heldSentBatchID)
                    try #require(!before.contains { $0.zone == zone && $0.id == actual })
                    try #require(await sdk.automaticRuns == runs + 1)
                    await sent.release()
                }
                let completion = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                let emptyOperation = originalEmpty
                joined = Task {
                    do {
                        try await emptyOperation?.drain()
                        await sdk.waitForAutomaticSend()
                        completion.continuation.yield(true)
                    } catch { Issue.record(error); completion.continuation.yield(false) }
                    completion.continuation.finish()
                }
                try #require(try await serviceAppSignal(completion.stream, stage: "terminal empty automatic runs joined without spin"))
                await joined?.value
                await app.sync.waitForInputs()
                let delivered = await sdk.deliveredSentBatches
                try #require(delivered.count == before.count + (addition ? 1 : 0))
                try #require(await sdk.automaticRuns == runs + (addition ? 1 : 0))
                if addition {
                    let saved = await wire.receipts.dropFirst(receipts)
                    try #require(!saved.isEmpty)
                    for receipt in saved {
                        let ack = try #require(try await backend.workflow.entityFacts(name: receipt.record.name))
                        try #require(ack.mark == nil && ack.inFlight.isEmpty && ack.ackedRev == ack.localRev)
                        try #require(ack.ackedSystemFields == receipt.record.systemFields)
                    }
                    try #require(await sdk.emptyBatchRequests <= empty + 3)
                } else {
                    try #require(await sdk.batchRequests == requests)
                    try #require(await sdk.emptyBatchRequests == empty)
                }
                try #require(appNetworkRequests == 0 && !app.sync.operationFailed)
                let finalRequests = await sdk.batchRequests, finalEmpty = await sdk.emptyBatchRequests
                await sdk.waitForAutomaticSend(); try await sdk.waitForEventBookkeeping()
                try #require(await sdk.batchRequests == finalRequests)
                try #require(await sdk.emptyBatchRequests == finalEmpty)
            } catch {
                await terminal.release(); await sent.release()
                _ = await app.sync.cancelAndDrain()
                await joined?.value
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func serviceSDKCancellationAndReleaseJoinOriginalAutomaticCallbackAndFenceAdmission(release: Bool) async throws {
        try await withAutomaticServiceApp { app, fixture, _ in
            app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(await app.factory.engines.last)
            try #require(await sdk.automaticSchedulingEnabled)
            await sdk.waitForAutomaticSend()
            let zone = try #require(fixture.records.first?.zone)
            let stale = try await sdk.prepareSend(zones: [zone])
            let handler = ServiceAutomaticGate()
            let cancellation = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let stopped = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let boundary = ServiceAutomaticGate()
            let backend = try app.backend
            await sdk.holdNextSentHandlerReturn { await handler.hold() }
            var stop: Task<Bool, Never>?
            var freshOperation: CloudKitNetworkOperation?
            var freshJoin: Task<Void, Never>?
            var returned = false
            backend.beforeQuiesce = {
                // cancelAndDrain has fenced app admission and qualified any
                // original command cancellation before reaching this hook.
                await sdk.inspectNextCancellation { cancellation.continuation.yield(true) }
                if release { await sdk.release() } else { await sdk.cancelOperations() }
                returned = true
                stopped.continuation.yield(true)
                await boundary.hold()
            }
            defer { backend.beforeQuiesce = nil; cancellation.continuation.finish(); stopped.continuation.finish() }
            do {
                let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
                let deliveredBefore = await sdk.deliveredSentBatches.count
                try await automaticServiceEdit(app, id: seed.id, suffix: " Original automatic callback")
                try #require(try await serviceAppSignal(handler.entered, stage: "original automatic sent handler held"))
                stop = Task { @MainActor in await app.sync.cancelAndDrain() }
                try #require(try await serviceAppSignal(cancellation.stream, stage: "original automatic cancellation fence"))
                try #require(!returned)
                // A newer real revision can arrive during drain, but may not
                // create a fresh automatic operation on the cancelled SDK.
                try await automaticServiceEdit(app, id: seed.id, suffix: " Dirty while original callback drains")
                await handler.release()
                try #require(try await serviceAppSignal(stopped.stream, stage: "original SDK cancellation or release returned"))
                try #require(returned)
                await sdk.waitForAutomaticSend()
                try #require(await sdk.deliveredSentBatches.count == deliveredBefore + 1)
                let requests = await sdk.batchRequests
                do {
                    let refused = try stale.enqueue(validate: {})
                    try await refused.drain()
                    Issue.record("Pre-cancellation SDK generation admitted a later network call")
                } catch {}
                if release {
                    do {
                        _ = try await sdk.prepareSend(zones: [zone])
                        Issue.record("Released SDK accepted new preparation")
                    } catch { /* retired original SDK */ }
                }
                try await automaticServiceEdit(app, id: seed.id, suffix: " Later update after drain")
                await sdk.waitForAutomaticSend()
                try #require(await sdk.batchRequests == requests)
                try #require(await sdk.deliveredSentBatches.count == deliveredBefore + 1)
                if !release {
                    let fresh = try await sdk.prepareSend(zones: [zone])
                    let operation = try fresh.enqueue(validate: {})
                    freshOperation = operation
                    let completion = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                    freshJoin = Task {
                        do {
                            try await operation.drain()
                            await sdk.waitForAutomaticSend()
                            completion.continuation.yield(true)
                        } catch { Issue.record(error); completion.continuation.yield(false) }
                        completion.continuation.finish()
                    }
                    try #require(try await serviceAppSignal(completion.stream, stage: "fresh post-cancellation SDK authority joined"))
                    await freshJoin?.value
                    try #require(await sdk.batchRequests > requests)
                    let freshRequests = await sdk.batchRequests
                    do {
                        try await stale.enqueue(validate: {}).drain()
                        Issue.record("Fresh authority revived a pre-cancellation SDK preparation")
                    } catch {}
                    try #require(await sdk.batchRequests == freshRequests)
                }
                try #require(!app.sync.operationFailed)
                await boundary.release()
                try #require(await stop?.value == true)
                try #require(!app.sync.operationFailed)
            } catch {
                await handler.release(); await boundary.release()
                if let stop { _ = await stop.value }
                else { _ = await app.sync.cancelAndDrain() }
                await freshOperation?.cancelAndDrain()
                await freshJoin?.value
                throw error
            }
        }
    }
}

@MainActor private func withAutomaticServiceApp(_ body: @MainActor (ServiceAppHarness, InitialCloudFixture, ServiceWorkflowWire) async throws -> Void) async throws {
    let fixture = try InitialCloudFixture.load()
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-sdk-automatic-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    defer { try? FileManager.default.removeItem(at: root) }
    let serviceHome = root.appending(path: "service")
    try FileManager.default.createDirectory(at: serviceHome, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    let process = try await CloudServiceProcess.open(stateDirectory: serviceHome)
    var active: ServiceAppHarness?
    do {
        let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
        let app = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire, clock: .system)
        active = app
        defer { app.defaults.removePersistentDomain(forName: app.suite) }
        try await wire.seed(fixture)
        try await app.adopt(); try await app.exchange()
        // Join the original exchange's follow-up commands before a settings
        // change retires that SDK. No preexisting request belongs to our holds.
        await app.sync.waitForInputs(); await app.sync.drainOperations()
        try #require(!app.sync.operationFailed)
        try #require(!app.sync.settings.automatic)
        try await body(app, fixture, wire)
        try #require(await app.owner.close())
        let exit = await process.close()
        try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
    } catch {
        if let active { await active.cleanupAfterServiceFailure() }
        else { _ = await process.cancel() }
        throw error
    }
}

@MainActor private func automaticServiceEdit(_ app: ServiceAppHarness, id: Int64, suffix: String) async throws {
    let note = try await CoreNoteService().readNote(id: id)
    var fields = note.fields; fields[0] += suffix
    let mutation = try await CoreNoteService().updateNote(.init(id: id, fields: fields, tags: note.tags))
    try #require(mutation.written && mutation.id == id)
    try await CoreCollectionService().flushForBackground()
    try await app.backend.workflow.awaitTracking(generation: mutation.summary.generation, seq: mutation.summary.seq)
    await app.sync.waitForInputs()
}

private actor ServiceAutomaticGate {
    nonisolated let entered: AsyncStream<Bool>
    private let continuation: AsyncStream<Bool>.Continuation
    private var waiter: CheckedContinuation<Void, Never>?
    private var released = false
    init() { (entered, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1)) }
    func hold() async {
        continuation.yield(true)
        if !released { await withCheckedContinuation { waiter = $0 } }
    }
    func release() { released = true; waiter?.resume(); waiter = nil; continuation.finish() }
}
#endif
