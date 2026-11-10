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
        let process = try await CloudServiceProcess.open(stateDirectory: service, pageSize: 32)
        var active: ServiceAppHarness?, exchange: Task<Void, any Error>?
        let held = SelectedOriginalReplyGate()
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let a = try ServiceAppHarness(root: root.appending(path: "A"), wire: wire, clock: .system)
            let b = try ServiceAppHarness(root: root.appending(path: "B"), wire: wire, clock: .system)
            defer { a.defaults.removePersistentDomain(forName: a.suite); b.defaults.removePersistentDomain(forName: b.suite) }
            for directory in [a.root, b.root] {
                try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
                try input.bytes.write(to: directory.appending(path: "collection.anki2"))
            }
            active = a
            try await selectedBootstrapPublish(a)
            await a.sync.waitForInputs(); await a.sync.drainOperations(); await a.sync.waitForInputs()
            try #require(!a.sync.operationFailed)
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
            await b.sync.waitForInputs(); await b.sync.drainOperations(); await b.sync.waitForInputs()
            try #require(!b.sync.operationFailed)
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
            await a.sync.waitForInputs(); await a.sync.drainOperations(); await a.sync.waitForInputs()
            try #require(!a.sync.operationFailed)
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

    @Test(arguments: ["server-record-changed", "later-rewrite"])
    func selectedOriginalHeldFailedAndMismatchedACKRefuseRealServiceProof(mode: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-original-refusal-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let input = try await selectedBootstrapSource(root: root)
        let role = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
        let service = root.appending(path: "service")
        try FileManager.default.createDirectory(at: service, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: service, pageSize: 32)
        var active: ServiceAppHarness?, operation: CloudKitNetworkOperation?
        var drain: Task<Void, any Error>?
        let held = SelectedOriginalReplyGate()
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let app = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire, clock: .system)
            active = app
            defer { app.defaults.removePersistentDomain(forName: app.suite) }
            try FileManager.default.createDirectory(at: app.root, withIntermediateDirectories: false)
            try input.bytes.write(to: app.root.appending(path: "collection.anki2"))
            try await selectedBootstrapPublish(app)
            await app.sync.waitForInputs(); await app.sync.drainOperations(); await app.sync.waitForInputs()
            try #require(!app.sync.operationFailed)
            _ = try await role.validateOwned()
            let sdk = try #require(await app.factory.engines.last)
            await sdk.waitForAutomaticSend(); try await sdk.waitForEventBookkeeping()
            try #require(!(await sdk.automaticSchedulingEnabled))
            let backend = try app.backend
            let baselineSnapshot = try await backend.workflow.durableSnapshot()
            let batchesBefore = await sdk.batchRequests
            let deliveredBefore = await sdk.deliveredSentBatches.count
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
            let edit = try await role.fixedEdit()
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            try #require(!app.sync.operationFailed)
            let revision = try #require(edit.localRevision)
            try #require(revision > 0 && edit.updatedNotes == 1 && edit.source == .userUndoable)
            let current = try await role.observe()
            try selectedOriginalNonACK(current, edit: edit)
            let baseline = try #require(baselineSnapshot.entities.first { $0.name == current.entity.recordName })
            try #require(baseline.mark == nil && baseline.localRev == baseline.ackedRev && baseline.ackedRev < revision)
            let baselineSaved = try #require(await wire.receipts.last { $0.record.name == current.entity.recordName })
            let zone = baselineSaved.zone
            try #require(baselineSaved.record.systemFields == baseline.ackedSystemFields)
            var competing: ServiceSavedReceipt?
            if mode == "server-record-changed" {
                let read = try await wire.read(zone: zone, name: current.entity.recordName, all: true)
                let server = try ServiceRecordBytes(serviceRequired(try read.ok()["Record"]))
                try #require(server.name == baselineSaved.record.name && server.recordType == baselineSaved.record.recordType)
                try #require(server.envelope == baselineSaved.record.envelope && server.systemFields == baseline.ackedSystemFields)
                // A real conditional re-save of the exact ACK baseline advances
                // only the Service tag. Core keeps its older conditional base.
                let reply = try await wire.modify(zone: zone, changes: [.object(["Save": .object([
                    "record": .object(["name": .string(server.name), "record_type": .string(server.recordType),
                        "fields": try serviceRequired(server.raw["fields"])]),
                    "if_unchanged": .bytes(server.systemFields),
                ])])])
                let mutations = try #require(try reply.ok()["Modified"]?.items)
                try #require(mutations.count == 1 && mutations[0]["name"]?.text == server.name)
                let saved = try ServiceRecordBytes(serviceRequired(mutations[0]["result"]?["Ok"]?["Saved"]))
                try #require(saved.name == server.name && saved.recordType == server.recordType && saved.envelope == server.envelope)
                try #require(try saved.asset("pa") == server.asset("pa"))
                try #require(saved.systemFields != server.systemFields && !saved.systemFields.isEmpty)
                try await wire.recordReceipts(reply, zone: zone)
                competing = .init(original: reply, zone: zone, record: saved.record(zone: zone))
                let unchanged = try #require(try await backend.workflow.entityFacts(name: server.name))
                try #require(unchanged.ackedRev == baseline.ackedRev && unchanged.ackedSystemFields == baseline.ackedSystemFields)
            }
            let receiptsBefore = await wire.receipts.count
            let conflictsBefore = await wire.saveConflicts.count
            await app.sync.waitForInputs(); await app.sync.drainOperations(); await app.sync.waitForInputs()
            try #require(!app.sync.operationFailed)
            await sdk.holdNextSentResult(zone: nil) { await held.hold() }
            // Use the already admitted normal SDK path; the Service produces
            // the actual Saved or ServerRecordChanged. No result replacement.
            let prepared = try await sdk.prepareSend(zones: [zone])
            operation = try prepared.enqueue(validate: {})
            try #require(try await serviceAppSignal(held.entered, stage: "selected original refusal Service reply held"))
            let batch = try #require(await sdk.heldSentBatchID)
            if mode == "server-record-changed" { try #require(!(await sdk.heldSavedNames).contains(current.entity.recordName)) }
            else { try #require((await sdk.heldSavedNames).contains(current.entity.recordName)) }
            let whileHeld = try await role.observe()
            try selectedOriginalNonACK(whileHeld, edit: edit)
            try #require(whileHeld.entity.ackedEnvelopeDigest == Data(SHA256.hash(data: baselineSaved.record.envelope)))
            let handed = try #require(try await backend.workflow.entityFacts(name: current.entity.recordName))
            try #require(handed.inFlight.filter { $0.localRev == revision }.count == 1)
            try #require(handed.ackedRev == baseline.ackedRev && handed.ackedSystemFields == baseline.ackedSystemFields)
            let mark = try #require(handed.mark)
            try #require(mark.payloadDigest == handed.payloadDigest && handed.payloadDigest != baseline.ackedPayloadDigest)
            if mode == "server-record-changed" {
                let competing = try #require(competing)
                let conflicts = await wire.saveConflicts.dropFirst(conflictsBefore)
                try #require(conflicts.count == 1)
                let conflict = try #require(conflicts.first)
                try #require(conflict.code == "ServerRecordChanged" && conflict.zone == zone && conflict.name == current.entity.recordName)
                try #require(conflict.server.name == competing.record.name && conflict.server.recordType == competing.record.recordType)
                try #require(conflict.server.envelope == baselineSaved.record.envelope && conflict.server.systemFields == competing.record.systemFields)
                try #require(conflict.original.requestID > competing.original.requestID)
                try #require(conflict.original.originalRequest["caller"]?["instance"]?.text == sdk.instanceID)
                let changes = try #require(conflict.original.originalRequest["operation"]?["Modify"]?["changes"]?.items)
                let sent = try #require(changes.first { $0["Save"]?["record"]?["name"]?.text == current.entity.recordName }?["Save"])
                try #require(try sent["if_unchanged"]?.bytes() == baseline.ackedSystemFields)
                try #require(try sent["record"]?["fields"]?["e"]?["EncryptedBytes"]?.bytes() != conflict.server.envelope)
                try #require((await wire.receipts).count == receiptsBefore)
            }
            if mode == "later-rewrite" {
                let row = try await CoreNoteService().readNote(id: input.noteID)
                _ = try await CoreNoteService().updateNote(.init(id: row.id,
                    fields: ["Generated-later-rewrite-Q", row.fields[1]], tags: row.tags))
                try await CoreCollectionService().flushForBackground()
                await app.sync.waitForInputs(); await app.sync.drainOperations()
            }
            await held.release()
            let originalOperation = try #require(operation)
            let joined = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            drain = Task {
                defer { joined.continuation.finish() }
                try await originalOperation.drain()
                joined.continuation.yield(true)
            }
            try #require(try await serviceAppSignal(joined.stream, stage: "selected original refusal SDK operation drained"))
            try await drain?.value
            try await sdk.waitForEventBookkeeping()
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            try #require(!app.sync.operationFailed)
            let observations = try await backend.workflow.observations()
            try #require(observations.contains(.batchBuilt(names: [current.entity.recordName], batchID: batch)))
            try #require(observations.contains(.handlerReturned(requestID: batch)))
            let delivered = await sdk.deliveredSentBatches.dropFirst(deliveredBefore)
            try #require(delivered.count == 1 && delivered.first?.id == batch && delivered.first?.zone == zone)
            try #require(await sdk.batchRequests == batchesBefore + 1 && appNetworkRequests == 0)
            let policy = try await backend.workflow.policy()
            try #require(try await policy.status().inFlight == 0)
            let facts = try #require(try await backend.workflow.entityFacts(name: current.entity.recordName))
            try #require(facts.mark != nil && facts.ackedRev < facts.localRev)
            try #require(await sdk.pendingRecordZoneChanges()[current.entity.recordName] == .save)
            if mode == "server-record-changed" {
                let refused = try await role.observe()
                try selectedOriginalNonACK(refused, edit: edit)
                try #require(refused.entity.ackedEnvelopeDigest == whileHeld.entity.ackedEnvelopeDigest)
                // K10.3/K12.1 retain this durable handed history after failure.
                // Handler return/drain above proves active work has ended.
                try #require(facts.localRev == revision && facts.inFlight == handed.inFlight && facts.mark == handed.mark)
                try #require(facts.payloadDigest == handed.payloadDigest && facts.ackedPayloadDigest == baseline.ackedPayloadDigest)
                try #require(facts.ackedRev == baseline.ackedRev && facts.ackedSystemFields == baseline.ackedSystemFields)
                try #require((await wire.receipts).count == receiptsBefore)
                try #require(!observations.contains(.savedPendingRemoved(name: current.entity.recordName, requestID: batch)))
                let competing = try #require(competing)
                let read = try await wire.read(zone: zone, name: current.entity.recordName, all: true)
                let server = try ServiceRecordBytes(serviceRequired(try read.ok()["Record"]))
                try #require(server.name == competing.record.name && server.recordType == competing.record.recordType)
                try #require(server.envelope == baselineSaved.record.envelope && server.systemFields == competing.record.systemFields)
                let durable = try await backend.workflow.durableSnapshot()
                try #require(!durable.inboxNames.contains(current.entity.recordName) && !durable.trackingStopped)
            } else {
                let original = try #require(await wire.receipts.dropFirst(receiptsBefore).last { $0.record.name == current.entity.recordName })
                try #require(facts.ackedRev == edit.localRevision && facts.ackedSystemFields == original.record.systemFields)
                try #require(facts.localRev > facts.ackedRev && facts.payloadDigest != facts.ackedPayloadDigest)
                try #require(facts.inFlight.isEmpty)
                try #require(observations.contains(.savedPendingRemoved(name: current.entity.recordName, requestID: batch)))
                await #expect(throws: (any Error).self) { try await role.observe() }
            }
            try #require(await app.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
        } catch {
            await held.release(); await operation?.cancelAndDrain()
            _ = try? await drain?.value
            if let active { await active.cleanupAfterServiceFailure() }
            else { #expect(await process.cancel().joined) }
            throw error
        }
    }
}

@MainActor private func selectedOriginalNonACK(_ observed: OriginalCloudObservation, edit: OriginalCloudEditFacts) throws {
    try #require(observed.generation == edit.generation && observed.seq >= edit.seq && observed.observedSeq == edit.seq)
    try #require(observed.qChanged && observed.entity.revision == edit.localRevision && observed.entity.digest == edit.noteDigest)
    try #require(!observed.entity.acknowledged && observed.entity.ackedRevision < observed.entity.revision)
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
