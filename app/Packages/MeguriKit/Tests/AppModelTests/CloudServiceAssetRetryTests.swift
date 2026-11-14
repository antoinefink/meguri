#if os(macOS)
@testable import AppModel
@testable import CoreBridge
import Foundation
@testable import Instrumentation
import func MeguriCoreBindings.testCloudAssetReadFixtureObservation
import struct MeguriCoreBindings.CloudAssetReadFixtureObservation
import Testing

extension ReviewCollectionStartupTests {
    @Test func realServiceAssetInboxAutomaticallyRetriesOncePerDrainAndResetsOnReopen() async throws {
        let fixture = try InitialCloudFixture.load()
        let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { $0.fields.count == 2 })
        let notePrefix = assetRetryNotePrefix(seed.id)
        let seededRecord = try #require(fixture.records.first { $0.encrypted.starts(with: notePrefix) })
        let name = seededRecord.name, zone = seededRecord.zone
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-auto-asset-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let serviceDirectory = root.appending(path: "service")
        try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, transport: .binaryAssetsV1)
        var active: ServiceAppHarness?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let a = try ServiceAppHarness(root: root.appending(path: "A"), wire: wire)
            let b = try ServiceAppHarness(root: root.appending(path: "B"), wire: wire)
            defer {
                a.defaults.removePersistentDomain(forName: a.suite)
                b.defaults.removePersistentDomain(forName: b.suite)
            }
            try await wire.seed(fixture)
            active = a
            try await a.adopt()
            try await a.exchange(phase: .aBaseline)
            try #require(!a.sync.settings.automatic && !a.sync.settings.paused)
            let baseline = try #require(try await a.backend.workflow.entityFacts(name: name))
            let initialRead = try await wire.read(zone: zone, name: name, all: true)
            let initialServer = try ServiceRecordBytes(serviceRequired(try initialRead.ok()["Record"]))
            try #require(baseline.mark == nil && baseline.localRev == baseline.ackedRev)
            try #require(baseline.payloadDigest == baseline.ackedPayloadDigest)
            try #require(!baseline.ackedSystemFields.isEmpty && baseline.ackedSystemFields == initialServer.systemFields)
            let original = try await CoreNoteService().readNote(id: seed.id)
            let unrelated = try #require(fixture.notes.first { $0.id != seed.id })
            let unrelatedBefore = try await CoreNoteService().readNote(id: unrelated.id)
            let mediaBefore = try await serviceMediaProof(generation: a.owner.generation)
            let localField = "Synthetic retained local field"
            let localFields = [localField, original.fields[1]]
            // Automatic off is the real policy input preventing an upload.
            // The engine edit, durable Changes, and capture are all ordinary.
            try await assetRetryEdit(a, note: original, fields: localFields)
            let retained = try #require(try await a.backend.workflow.entityFacts(name: name))
            try #require(retained.mark != nil && retained.localRev > retained.ackedRev)
            try #require(retained.ackedSystemFields == baseline.ackedSystemFields)
            try #require(await a.owner.close())

            // One CoreHost at a time; B edits the same Note, origin, binding,
            // and server base through the public editor and real conditional save.
            active = b
            try await b.adopt()
            try await b.exchange(phase: .bBaseline)
            let bNote = try await CoreNoteService().readNote(id: seed.id)
            try #require(bNote.fields == original.fields && bNote.guid == original.guid)
            let remoteField = String(repeating: "remote", count: 160_000)
            let bReceiptsBefore = await wire.receipts.count
            try await assetRetryEdit(b, note: bNote, fields: [original.fields[0], remoteField])
            try await b.exchange(phase: .bRemoteSave)
            let bReceipts = Array(await wire.receipts.dropFirst(bReceiptsBefore))
            let remoteReceipt = try #require(bReceipts.last { $0.zone == zone && $0.record.name == name })
            let remoteSaved = try assetRetrySavedBytes(remoteReceipt)
            let remotePayload = try #require(try remoteSaved.asset("pa"))
            try #require(remoteSaved.envelope != initialServer.envelope && remoteSaved.systemFields != initialServer.systemFields)
            try #require(remotePayload.range(of: Data(remoteField.utf8)) != nil)
            try #require(await b.owner.close())

            try await wire.withholdAssetAndFailFiveReads(record: remoteReceipt.record, payload: remotePayload)
            active = a
            try await a.reopen()
            let firstBackend = try a.backend
            let firstSDK = try #require(await a.factory.engines.last)
            let firstSDKID = firstSDK.instanceID
            try #require(try await CoreNoteService().readNote(id: seed.id).fields == localFields)
            let reopenedRetained = try #require(try await firstBackend.workflow.entityFacts(name: name))
            try #require(reopenedRetained == retained)
            let receiptsBeforeRetries = await wire.receipts.count
            let drainBase = try await assetRetryCollectionDrains(a, zone: zone)
            var trace: [AssetRetryDrainTrace] = []
            // Five fresh SDK fetches, each with one actual collection boundary.
            // No retry/prepareExecution/Core fetch or issued-command port is called.
            for expected in [1, 2, 3, 4, 5] {
                try await assetRetryScheduledSDKFetch(a, zone: zone)
                try await firstBackend.waitForAssetRetry()
                await a.sync.waitForInputs()
                let observed = try await assetRetryObservation(name: name, noteID: seed.id)
                let reads = await wire.assetReadObservations
                let drains = try await assetRetryCollectionDrains(a, zone: zone)
                trace.append(.init(collectionDrains: drains - drainBase, reads: reads.count,
                    commands: firstBackend.assetRetryCompleted, retryCount: observed.retryCount))
                try #require(drains == drainBase + expected, "Actual drain trace: \(trace)")
                try #require(firstBackend.assetRetryStarted == expected && firstBackend.assetRetryCompleted == expected,
                    "Actual drain trace: \(trace)")
                try #require(reads.count == expected && observed.retryCount == UInt32(expected), "Actual drain trace: \(trace)")
                try #require(observed.ownedCount == 0 && observed.rowFields == Data(localFields.joined(separator: "\u{1f}").utf8))
                try #require(reads.allSatisfy { $0.failedDelivery && !$0.retainedLease
                    && $0.record.name == name && $0.zone == zone
                    && $0.record.envelope == remoteSaved.envelope && $0.record.systemFields == remoteSaved.systemFields })
            }
            try #require(await wire.withheldPayloadDeliveries == 1)
            // A sixth real drain in this open grants no sixth accepted read.
            try await assetRetryScheduledSDKFetch(a, zone: zone)
            await a.sync.waitForInputs()
            try #require(try await assetRetryCollectionDrains(a, zone: zone) == drainBase + 6)
            try #require(firstBackend.assetRetryStarted == 5 && firstBackend.assetRetryCompleted == 5)
            try #require(await wire.assetReadObservations.count == 5)
            let exhausted = try await assetRetryObservation(name: name, noteID: seed.id)
            try #require(exhausted.retryCount == 5 && exhausted.ownedCount == 0)
            let exhaustedSnapshot = try await firstBackend.workflow.durableSnapshot()
            try #require(exhaustedSnapshot.inboxNames.contains(name))
            try #require(try await firstBackend.workflow.entityFacts(name: name) == retained)
            try #require(await wire.receipts.count == receiptsBeforeRetries)
            try #require(await a.owner.close())
            try #require(firstBackend.closed && firstBackend.closeReturned == firstBackend.closeEntered)
            // Close joins all originally accepted callbacks and policy work;
            // these terminal observations also catch a late sixth dispatch.
            try #require(firstBackend.assetRetryStarted == 5 && firstBackend.assetRetryCompleted == 5)
            try #require(await wire.assetReadObservations.count == 5)

            try await wire.restoreAssetDelivery()
            try await a.reopen() // SAME A paths/defaults, not adoption or reseeding.
            let nextBackend = try a.backend
            let nextSDK = try #require(await a.factory.engines.last)
            try #require(nextSDK.instanceID != firstSDKID)
            let reset = try await assetRetryObservation(name: name, noteID: seed.id)
            try #require(reset.retryCount == 0 && reset.ownedCount == 0 && reset.rowFields == exhausted.rowFields)
            let resetSnapshot = try await nextBackend.workflow.durableSnapshot()
            try #require(resetSnapshot.binding == exhaustedSnapshot.binding && resetSnapshot.inboxNames.contains(name))
            try #require(try await nextBackend.workflow.entityFacts(name: name) == retained)
            let nextDrainBase = try await assetRetryCollectionDrains(a, zone: zone)
            // The FIRST qualifying collection drain must issue the command via
            // SyncCoordinator and execute it through the retained native backend.
            try await assetRetryScheduledSDKFetch(a, zone: zone)
            try await nextBackend.waitForAssetRetry()
            await a.sync.waitForInputs()
            try #require(try await assetRetryCollectionDrains(a, zone: zone) == nextDrainBase + 1)
            try #require(nextBackend.assetRetryStarted == 1 && nextBackend.assetRetryCompleted == 1)
            let delivered = try await assetRetryObservation(name: name, noteID: seed.id)
            try #require(delivered.retryCount == nil && delivered.ownedCount == 0)
            try #require(delivered.changedByIncoming && delivered.walReceipts > reset.walReceipts)
            let merged = try await CoreNoteService().readNote(id: seed.id)
            try #require(merged.fields == [localField, remoteField] && merged.guid == original.guid
                && merged.id == original.id && merged.notetypeId == original.notetypeId && merged.tags == original.tags)
            let reads = await wire.assetReadObservations
            try #require(reads.count == 6 && Set(reads.map { $0.original.requestID }).count == 6)
            let successfulRead = try #require(reads.last)
            try #require(!successfulRead.failedDelivery && successfulRead.retainedLease)
            try #require(successfulRead.record.envelope == remoteSaved.envelope
                && successfulRead.record.systemFields == remoteSaved.systemFields)
            try #require(await wire.receipts.count == receiptsBeforeRetries)

            // Only the ordinary policy/builder and Service Saved callback settle
            // the merged local edit. An accepted read alone is not an ACK.
            try await a.exchange(phase: .aMergedSave)
            let mergedReceipts = Array(await wire.receipts.dropFirst(receiptsBeforeRetries))
            let savedReceipt = try #require(mergedReceipts.last { $0.zone == zone && $0.record.name == name })
            let saved = try assetRetrySavedBytes(savedReceipt)
            let savedPayload = try #require(try saved.asset("pa"))
            try #require(savedPayload.range(of: Data(localField.utf8)) != nil
                && savedPayload.range(of: Data(remoteField.utf8)) != nil)
            let request = try JSONDecoder().decode(ServiceJSON.self, from: savedReceipt.original.originalRequestJSON)
            let save = try #require(request["operation"]?["Modify"]?["changes"]?.items?.compactMap { $0["Save"] }
                .first { $0["record"]?["name"]?.text == name })
            try #require(try save["if_unchanged"]?.bytes() == remoteSaved.systemFields)
            let ack = try #require(try await nextBackend.workflow.entityFacts(name: name))
            try #require(ack.mark == nil && ack.inFlight.isEmpty && ack.localRev == ack.ackedRev)
            try #require(ack.payloadDigest == ack.ackedPayloadDigest && ack.ackedPayloadDigest != baseline.ackedPayloadDigest)
            try #require(ack.ackedSystemFields == saved.systemFields && saved.systemFields != remoteSaved.systemFields)
            let sendables = try await nextBackend.workflow.trackingSendables()
            let settled = try await nextBackend.workflow.durableSnapshot()
            try #require(sendables[name] == nil && !settled.inboxNames.contains(name))
            try #require(await a.owner.close())
            try await a.reopen()
            try #require(try await CoreNoteService().readNote(id: seed.id) == merged)
            try #require(try await a.backend.workflow.entityFacts(name: name) == ack)
            try #require(try await CoreNoteService().readNote(id: unrelated.id) == unrelatedBefore)
            let mediaAfter = try await serviceMediaProof(generation: a.owner.generation)
            try #require(mediaAfter.root == mediaBefore.root && mediaAfter.names == mediaBefore.names && mediaAfter.bytes == mediaBefore.bytes)
            try #require(await wire.assetReadObservations.count == 6)
            try #require(await a.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
        } catch {
            print(serviceTransportSummary(process))
            if let active { await active.cleanupAfterServiceFailure() }
            else {
                let start = serviceAppUptimeMilliseconds()
                print("Service app cleanup.process.cancel.begin at_ms=\(start) elapsed_ms=0")
                let exit = await process.cancel()
                print("Service app cleanup.process.cancel.end at_ms=\(serviceAppUptimeMilliseconds()) elapsed_ms=\(serviceAppUptimeMilliseconds() - start) joined=\(exit.joined)")
                #expect(exit.joined, "Failing asset fixture must join its owned Service process")
            }
            throw error
        }
    }
}

private nonisolated struct AssetRetryDrainTrace: Sendable, CustomStringConvertible {
    let collectionDrains: Int
    let reads: Int
    let commands: Int
    let retryCount: UInt32?
    var description: String { "drains=\(collectionDrains) reads=\(reads) commands=\(commands) count=\(String(describing: retryCount))" }
}

private func assetRetryEdit(_ harness: ServiceAppHarness, note: NoteRecord, fields: [String]) async throws {
    let mutation = try await CoreNoteService().updateNote(.init(id: note.id, fields: fields, tags: note.tags))
    try #require(mutation.written && mutation.id == note.id)
    try await CoreCollectionService().flushForBackground()
    try await harness.backend.workflow.awaitTracking(generation: mutation.summary.generation, seq: mutation.summary.seq)
    await harness.sync.waitForInputs()
}

private func assetRetryScheduledSDKFetch(_ harness: ServiceAppHarness, zone: String) async throws {
    let engine = try #require(await harness.factory.engines.last)
    let drains = try await assetRetryCollectionDrains(harness, zone: zone)
    let manualGroups = await engine.manualFetchGroups
    // Enter the SDK's admitted scheduled network path, without a manual call.
    try await engine.scheduledFetchChanges(options: .init(zones: [zone], priorityZones: [zone], groupID: "synthetic-asset-\(UUID().uuidString)"))
    try await engine.waitForEventBookkeeping()
    await harness.sync.waitForInputs()
    try #require(await engine.manualFetchGroups == manualGroups)
    try #require(try await assetRetryCollectionDrains(harness, zone: zone) == drains + 1)
}

private func assetRetryCollectionDrains(_ harness: ServiceAppHarness, zone: String) async throws -> Int {
    try await harness.backend.workflow.fetchDrains().filter { $0.zone == zone }.count
}

private nonisolated func assetRetryObservation(name: String, noteID: Int64) async throws -> MeguriCoreBindings.CloudAssetReadFixtureObservation {
    try await MaintenanceLane.shared.call(BridgeCall("testCloudAssetReadFixtureObservation")) {
        try MeguriCoreBindings.testCloudAssetReadFixtureObservation(name: name, noteId: noteID)
    }
}

private nonisolated func assetRetryNotePrefix(_ id: Int64) -> [UInt8] {
    let bits = UInt64(bitPattern: id)
    // K5.1 canonical prefix: format 1, Note, eight-byte canonical key.
    return [8, 1, 16, 1, 26, 8] + (0..<8).reversed().map { UInt8(truncatingIfNeeded: bits >> ($0 * 8)) }
}

private nonisolated func assetRetrySavedBytes(_ receipt: ServiceSavedReceipt) throws -> ServiceRecordBytes {
    let mutations = try serviceRequired(try receipt.original.ok()["Modified"]?.items)
    let raw = try serviceRequired(mutations.first { $0["name"]?.text == receipt.record.name }?["result"]?["Ok"]?["Saved"])
    let saved = try ServiceRecordBytes(raw)
    guard saved.envelope == receipt.record.envelope, saved.systemFields == receipt.record.systemFields else {
        throw ServiceWorkflowFailure.wire
    }
    return saved
}
#endif
