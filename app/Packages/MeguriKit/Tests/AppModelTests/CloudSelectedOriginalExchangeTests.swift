#if os(macOS) && DEBUG
@testable import AppModel
@testable import CoreBridge
import CryptoKit
import Foundation
import Testing

// Uses the existing real Service/Core/coordinator fixture. Roles and original
// receipts stay in process; no record identifiers or native correlation logs.
extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func selectedOriginalRevisionACKAndSettledPeerApplyUseRealService(earlierAutomaticACK: Bool) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-original-exchange-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let input = try await selectedBootstrapSource(root: root)
        let sourceRole = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
        let peerRole = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
        let service = root.appending(path: "service")
        try FileManager.default.createDirectory(at: service, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: service)
        var active: ServiceAppHarness?, exchange: Task<Void, any Error>?
        let held = SelectedOriginalReplyGate()
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let a = try ServiceAppHarness(root: root.appending(path: "A"), wire: wire)
            let b = try ServiceAppHarness(root: root.appending(path: "B"), wire: wire)
            defer { a.defaults.removePersistentDomain(forName: a.suite); b.defaults.removePersistentDomain(forName: b.suite) }
            for directory in [a.root, b.root] {
                try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
                try input.bytes.write(to: directory.appending(path: "collection.anki2"))
            }
            active = a
            try await selectedBootstrapPublish(a)
            try #require(await a.owner.close())
            // Establish B's original binding before A's edit. The later peer
            // step must use ordinary incremental fetched/apply, not adoption.
            active = b
            await b.owner.start(); await b.sync.prepareConnectionAuthority()
            try await b.owner.prepareCloudConnectionPreview(); await b.sync.waitForConnectionPreview()
            guard case .ready(_, let preview) = b.sync.preview else { throw ServiceWorkflowFailure.wire }
            try #require(preview.decision == .chooseCollection || preview.decision == .confirmAdoption)
            await b.sync.confirmAdoption(); await b.sync.waitForConnectionPreview(); await b.sync.waitForReplacement()
            await b.owner.waitForCloudActivation(); await b.sync.prepareConnectionAuthority(); await b.sync.waitForInputs()
            let adopted = try await SelectedCloudControlInspection().current()
            try #require(adopted.status == .proven && adopted.provenance == .retainedAdoption)
            try #require(adopted.activation == .proven && adopted.ownedBackup == .proven && adopted.requiredScanComplete)
            try #require(try await CoreNoteService().readNote(id: input.noteID).fields[0] == "Generated-Q")
            try #require(await b.owner.close())

            active = a
            try await a.reopen()
            let owned = try await sourceRole.validateOwned() // First binding of this retained role.
            try #require(!owned.qChanged && owned.generation == a.owner.generation)
            if earlierAutomaticACK {
                a.sync.setDeviceSettings(paused: false, automatic: true, media: true)
                await a.sync.waitForInputs(); await a.sync.drainOperations()
            }
            let sdk = try #require(await a.factory.engines.last)
            await sdk.waitForAutomaticSend()
            await sdk.holdNextSentResult(zone: nil) { await held.hold() }
            let receiptsBefore = await wire.receipts.count
            let edit = try await sourceRole.fixedEdit()
            let revision = try #require(edit.localRevision)
            try #require(revision > 0 && edit.updatedNotes == 1 && edit.source == .userUndoable)
            let backend = try a.backend
            var requests: [UInt64] = [], records: [CloudPolicyExchangeRecord] = []
            let recorded = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            defer { recorded.continuation.finish(); backend.onCommands = nil; backend.afterExecution = nil }
            backend.onCommands = { commands in
                for command in commands { if case .runExchange(let id, _) = command.action { requests.append(id) } }
            }
            backend.afterExecution = { command, result in
                if case .recordExchange(let record) = command.action, result.error == nil, !result.deferred {
                    records.append(record); recorded.continuation.yield(true)
                }
            }
            if !earlierAutomaticACK { exchange = Task { try await a.exchange() } }
            try #require(try await serviceAppSignal(held.entered, stage: "selected original Service Saved before ACK"))
            let heldBatch = try #require(await sdk.heldSentBatchID)
            let beforeACK = try await sourceRole.observe()
            try #require(beforeACK.observedSeq == edit.seq && beforeACK.entity.revision == revision)
            try #require(beforeACK.entity.digest == edit.noteDigest && !beforeACK.entity.acknowledged)
            try #require(beforeACK.entity.ackedRevision < revision)
            let actual = try #require(await wire.receipts.dropFirst(receiptsBefore).last { $0.record.name == beforeACK.entity.recordName })
            try #require(actual.record.envelope != Data() && actual.record.systemFields != nil)
            #expect(!(try await backend.workflow.observations()).contains(.savedPendingRemoved(name: beforeACK.entity.recordName, requestID: heldBatch)))
            await held.release()
            if earlierAutomaticACK {
                let joined = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                let task = Task { await sdk.waitForAutomaticSend(); joined.continuation.yield(true); joined.continuation.finish() }
                try #require(try await serviceAppSignal(joined.stream, stage: "selected earlier automatic ACK joined"))
                await task.value
                try await sdk.waitForEventBookkeeping()
                let earlier = try await sourceRole.observe()
                try selectedOriginalACK(earlier, edit: edit, receipt: actual)
                try #require(requests.isEmpty && records.isEmpty)
                exchange = Task { try await a.exchange() }
            }
            try await exchange?.value
            try #require(try await serviceAppSignal(recorded.stream, stage: "selected original manual recordExchange"))
            await a.sync.drainOperations(); await a.sync.waitForInputs()
            try await sdk.waitForEventBookkeeping()
            try #require(requests.count == 1 && records.count == 1 && requests.first == records.first?.exchange)
            guard case .succeeded = records[0].outcome else { throw ServiceWorkflowFailure.refused }
            let acknowledged = try await sourceRole.observe()
            try selectedOriginalACK(acknowledged, edit: edit, receipt: actual)
            let ackFacts = try #require(try await backend.workflow.entityFacts(name: acknowledged.entity.recordName))
            try #require(ackFacts.ackedSystemFields == actual.record.systemFields && ackFacts.payloadDigest == ackFacts.ackedPayloadDigest)
            let observations = try await backend.workflow.observations()
            try #require(observations.contains(.savedPendingRemoved(name: acknowledged.entity.recordName, requestID: heldBatch)))
            try #require(observations.contains(.handlerReturned(requestID: heldBatch)))
            try #require(await sdk.deliveredSentBatches.contains { $0.id == heldBatch })
            let noteReceipts = await wire.receipts.dropFirst(receiptsBefore).filter { $0.record.name == acknowledged.entity.recordName }
            try #require(noteReceipts.count == 1, "One fixed edit and original ACK must not require an extra edit or send")
            let canonical = try await sourceRole.validateOwned()
            try #require(canonical.qChanged && canonical.noteDigest == edit.noteDigest)
            let sourceRow = try await CoreNoteService().readNote(id: input.noteID)
            try #require(await a.owner.close())

            active = b
            try await b.reopen()
            let baseline = try await peerRole.validateOwned() // First binding of independent peer capability.
            try #require(!baseline.qChanged && baseline.noteDigest == peerRole.facts.noteDigest)
            // A's retained native Host is gone. Equal bytes on B do not transfer it.
            await #expect(throws: (any Error).self) { try await sourceRole.observe() }
            let peerBackend = try b.backend
            let peerSDK = try #require(await b.factory.engines.last)
            let peerBefore = try await peerRole.observe()
            try #require(peerBefore.appliedSeq == nil && !peerBefore.qChanged)
            var peerExchanges = 0
            peerBackend.onCommands = { commands in
                for command in commands { if case .runExchange = command.action { peerExchanges += 1 } }
            }
            try await b.exchange()
            await b.sync.drainOperations(); await b.sync.waitForInputs()
            try await peerSDK.waitForEventBookkeeping()
            try #require(peerExchanges == 1)
            let peer = try await peerRole.observe()
            let applied = try #require(peer.appliedSeq)
            // This producer joins actual Incoming run, removed intent/inbox and
            // its held-snapshot WAL receipt. Digest covers canonical metadata,
            // including modification time; nothing is masked for comparison.
            try #require(peer.qChanged && applied > 0 && applied == peer.observedSeq && applied <= peer.seq)
            try #require(peer.entity.digest == canonical.noteDigest && peer.entity.digest == acknowledged.entity.digest)
            try #require(peer.entity.ackedEnvelopeDigest == acknowledged.entity.ackedEnvelopeDigest)
            await #expect(throws: (any Error).self) { try await sourceRole.observe() }
            try #require(peer.entity.mediaBytes == nil && peer.entity.mediaSHA256 == nil)
            try #require(try await CoreNoteService().readNote(id: input.noteID) == sourceRow)
            let durable = try await peerBackend.workflow.durableSnapshot()
            try #require(durable.inboxNames.isEmpty && !durable.trackingStopped)
            let drains = try await peerBackend.workflow.fetchDrains()
            try #require(drains.contains { $0.fetched })
            peerBackend.onCommands = nil
            try #require(await b.owner.close())
            try #require(try Data(contentsOf: root.appending(path: "source/collection.anki2")) == input.bytes)
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
        } catch {
            await held.release()
            if let active { await active.cleanupAfterServiceFailure() }
            else { #expect(await process.cancel().joined) }
            _ = try? await exchange?.value
            throw error
        }
    }

    @Test(arguments: ["failed", "later-rewrite"])
    func selectedOriginalHeldFailedAndMismatchedACKRefuseRealServiceProof(mode: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-original-refusal-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let input = try await selectedBootstrapSource(root: root)
        let role = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
        let service = root.appending(path: "service")
        try FileManager.default.createDirectory(at: service, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: service)
        var active: ServiceAppHarness?, operation: CloudKitNetworkOperation?
        let held = SelectedOriginalReplyGate()
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let app = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire)
            active = app
            defer { app.defaults.removePersistentDomain(forName: app.suite) }
            try FileManager.default.createDirectory(at: app.root, withIntermediateDirectories: false)
            try input.bytes.write(to: app.root.appending(path: "collection.anki2"))
            try await selectedBootstrapPublish(app)
            _ = try await role.validateOwned()
            let sdk = try #require(await app.factory.engines.last)
            if mode == "failed" {
                let quota = try await wire.call(.object(["SetQuota": .object(["bytes": .integer(0)])]))
                try #require(try quota.ok().text == "Done")
            }
            let edit = try await role.fixedEdit()
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let backend = try app.backend
            let current = try await role.observe()
            try #require(!current.entity.acknowledged && current.entity.revision == edit.localRevision)
            let receiptsBefore = await wire.receipts.count
            await sdk.holdNextSentResult(zone: nil) { await held.hold() }
            // Use the already admitted normal SDK path; the Service produces
            // the actual Saved or quota failure. No OpResult/ACK replacement.
            let zone = try #require((await wire.receipts).first { $0.record.name == current.entity.recordName }?.zone)
            let prepared = try await sdk.prepareSend(zones: [zone])
            operation = try prepared.enqueue(validate: {})
            try #require(try await serviceAppSignal(held.entered, stage: "selected original refusal Service reply held"))
            let batch = try #require(await sdk.heldSentBatchID)
            if mode == "failed" { try #require(!(await sdk.heldSavedNames).contains(current.entity.recordName)) }
            else { try #require((await sdk.heldSavedNames).contains(current.entity.recordName)) }
            let whileHeld = try await role.observe()
            try #require(!whileHeld.entity.acknowledged && whileHeld.entity.ackedRevision < whileHeld.entity.revision)
            if mode == "later-rewrite" {
                let row = try await CoreNoteService().readNote(id: input.noteID)
                _ = try await CoreNoteService().updateNote(.init(id: row.id,
                    fields: ["Generated-later-rewrite-Q", row.fields[1]], tags: row.tags))
                try await CoreCollectionService().flushForBackground()
                await app.sync.waitForInputs(); await app.sync.drainOperations()
            }
            await held.release(); try await operation?.drain()
            try await sdk.waitForEventBookkeeping()
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let facts = try #require(try await backend.workflow.entityFacts(name: current.entity.recordName))
            try #require(facts.mark != nil && facts.ackedRev < facts.localRev && facts.inFlight.isEmpty)
            if mode == "failed" {
                let refused = try await role.observe()
                try #require(!refused.entity.acknowledged && refused.entity.revision == edit.localRevision)
                try #require((await wire.receipts).count == receiptsBefore)
                #expect(!(try await backend.workflow.observations()).contains(.savedPendingRemoved(name: current.entity.recordName, requestID: batch)))
            } else {
                let original = try #require(await wire.receipts.dropFirst(receiptsBefore).last { $0.record.name == current.entity.recordName })
                try #require(facts.ackedRev == edit.localRevision && facts.ackedSystemFields == original.record.systemFields)
                try #require(facts.localRev > facts.ackedRev && facts.payloadDigest != facts.ackedPayloadDigest)
                await #expect(throws: (any Error).self) { try await role.observe() }
            }
            try #require(await app.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
        } catch {
            await held.release(); await operation?.cancelAndDrain()
            if let active { await active.cleanupAfterServiceFailure() }
            else { #expect(await process.cancel().joined) }
            throw error
        }
    }
}

@MainActor private func selectedOriginalACK(_ observed: OriginalCloudObservation, edit: OriginalCloudEditFacts,
                                           receipt: ServiceSavedReceipt) throws {
    try #require(observed.generation == edit.generation && observed.seq >= edit.seq && observed.observedSeq == edit.seq)
    try #require(observed.qChanged && observed.entity.acknowledged && observed.entity.revision == edit.localRevision)
    try #require(observed.entity.ackedRevision == edit.localRevision && observed.entity.digest == edit.noteDigest)
    try #require(observed.entity.ackedEnvelopeDigest == Data(SHA256.hash(data: receipt.record.envelope)))
    try #require(receipt.record.name == observed.entity.recordName && receipt.record.systemFields != nil)
    try #require(observed.entity.mediaBytes == nil && observed.entity.mediaSHA256 == nil)
}

private actor SelectedOriginalReplyGate {
    nonisolated let entered: AsyncStream<Bool>
    private let reached: AsyncStream<Bool>.Continuation
    private var waiter: CheckedContinuation<Void, Never>?
    private var released = false
    init() { (entered, reached) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1)) }
    func hold() async {
        reached.yield(true)
        if !released { await withCheckedContinuation { waiter = $0 } }
    }
    func release() { released = true; waiter?.resume(); waiter = nil; reached.finish() }
}
#endif
