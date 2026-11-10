#if os(macOS) && DEBUG
@testable import AppModel
@testable import CoreBridge
import Foundation
import Observation
import Synchronization
import Testing

// Real CoreHost, coordinator, adapter and Service. Generated local data only;
// transport execution uses the Service's actual original replies, never verdicts.
extension ReviewCollectionStartupTests {
    @Test func selectedPublicationRebindWaitsForOriginalStatusCoreAcknowledgement() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-p4-publication-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let bytes = try await selectedBootstrapSource(root: root).bytes
        let service = root.appending(path: "service")
        try FileManager.default.createDirectory(at: service, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: service, pageSize: 32)
        let gate = SyncOwnerGate()
        let statusHeld = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        defer { statusHeld.continuation.finish() }
        var active: ServiceAppHarness?, publication: Task<Void, Never>?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let harness = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire)
            active = harness
            defer { harness.defaults.removePersistentDomain(forName: harness.suite) }
            try FileManager.default.createDirectory(at: harness.root, withIntermediateDirectories: false)
            try bytes.write(to: harness.root.appending(path: "collection.anki2"))
            let replies = SelectedBootstrapReplies()
            harness.observeOwn = { replies.record($0) }
            replies.observeFailure(harness.sync)
            await harness.owner.start(); await harness.sync.prepareConnectionAuthority()
            replies.observeOriginalBackend(harness.sync)
            try await harness.owner.prepareCloudConnectionPreview()
            await harness.sync.waitForConnectionPreview(); await harness.sync.drainOperations()
            guard case .ready(_, let summary) = harness.sync.preview else { throw ServiceWorkflowFailure.wire }
            try #require(summary.decision == .confirmUpload)
            let backend = try harness.backend
            var held: CloudPolicyCommand?, acknowledged = false, localStepsAfterHeld = 0
            backend.beforeExecution = { command in
                if case .statusChanged = command.action, held == nil {
                    held = command
                    statusHeld.continuation.yield(true)
                    await gate.hold()
                }
            }
            harness.sync.observeCausalBoundary = { boundary, command in
                switch boundary {
                case .commandAcknowledgementJoined:
                    if let held, let command, command.raw === held.raw { acknowledged = true }
                case .ownCoreStarted:
                    if held != nil {
                        localStepsAfterHeld += 1
                        #expect(acknowledged, "Original StatusChanged ACK must precede publication's next local mutation")
                    }
                default: break
                }
            }
            publication = Task { await harness.sync.confirmPublication() }
            try #require(try await serviceAppSignal(statusHeld.stream, stage: "P4 original publication status held"))
            #expect(!acknowledged && localStepsAfterHeld == 0)
            let before = try await SelectedCloudControlInspection().current()
            #expect(before.publicationComplete == nil && before.publicationDoneActive == nil)
            gate.release()
            await publication?.value
            try await replies.waitForOriginalTerminal()
            await harness.sync.waitForInputs(); await harness.sync.drainOperations()
            #expect(acknowledged && localStepsAfterHeld > 0 && !harness.sync.operationFailed)
            let published = try await SelectedCloudControlInspection().current()
            #expect(published.status == .proven && published.provenance == .runtimeAcceptedPublication)
            #expect(published.bindingMatchesEpoch == .proven && published.requiredScanComplete)
            let complete = try #require(published.publicationComplete)
            let done = try #require(published.publicationDoneActive)
            try replies.requireOriginalCommit(complete, before: done)
            backend.beforeExecution = nil; harness.sync.observeCausalBoundary = nil
            try #require(await harness.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
        } catch {
            gate.release(); await publication?.value
            if let active { await active.cleanupAfterServiceFailure() }
            else {
                let exit = await process.cancel()
                #expect(exit.joined)
            }
            throw error
        }
    }

    @Test func selectedPublicationAndExistingPeerAdoptionUseOriginalControlsAndOwnedReplacement() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-bootstrap-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let originalBytes = try await selectedBootstrapSource(root: root).bytes
        // Independent original-role capabilities, retained before either Host.
        let sourceRole = try await OriginalCloudRoleInspection.inspect(main: originalBytes, wal: nil)
        let peerRole = try await OriginalCloudRoleInspection.inspect(main: originalBytes, wal: nil)
        let serviceDirectory = root.appending(path: "service")
        try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, pageSize: 32)
        var active: ServiceAppHarness?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let a = try ServiceAppHarness(root: root.appending(path: "A"), wire: wire)
            let b = try ServiceAppHarness(root: root.appending(path: "B"), wire: wire)
            defer { a.defaults.removePersistentDomain(forName: a.suite); b.defaults.removePersistentDomain(forName: b.suite) }
            for directory in [a.root, b.root] {
                try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
                try originalBytes.write(to: directory.appending(path: "collection.anki2"))
            }
            let replies = SelectedBootstrapReplies()
            a.observeOwn = { replies.record($0) }
            replies.observeFailure(a.sync)
            await a.factory.inspectCreation { _ in
                let control = try await SelectedCloudControlInspection().current()
                try #require(control.requiredScanComplete)
                let scan = try await CoreCloudWorkflowService().observations()
                let kinds = Set(scan.compactMap { value -> UInt8? in
                    if case .reconcileKindCompleted(let kind, _) = value { return kind }; return nil
                })
                try #require(kinds == Set(UInt8(1)...UInt8(9)))
            }
            active = a
            await a.owner.start()
            await a.sync.prepareConnectionAuthority()
            replies.observeOriginalBackend(a.sync)
            try await a.owner.prepareCloudConnectionPreview()
            await a.sync.waitForConnectionPreview()
            guard case .ready(let offered, let summary) = a.sync.preview else { throw ServiceWorkflowFailure.wire }
            try #require(summary.decision == .confirmUpload && summary.manifestCounts == nil)
            await a.sync.confirmPublication()
            try await replies.waitForOriginalTerminal()
            await a.sync.waitForConnectionPreview()
            await a.sync.waitForInputs()
            try #require(!a.sync.operationFailed && a.sync.preview == nil)
            let published = try await SelectedCloudControlInspection().current()
            try #require(published.status == .proven && published.provenance == .runtimeAcceptedPublication)
            try #require(published.bindingMatchesEpoch == .proven && published.requiredScanComplete)
            let complete = try #require(published.publicationComplete)
            let done = try #require(published.publicationDoneActive)
            try replies.requireOriginalCommit(complete, before: done)
            let sourceReadback = try await sourceRole.validateOwned()
            try #require(sourceReadback.generation == a.owner.generation && !sourceReadback.qChanged)
            // The consumed preview cannot produce a second publication.
            let calls = replies.count
            let sourceConnection = try #require(try a.backend.connection)
            await #expect(throws: (any Error).self) { try await sourceConnection.confirmPublication(offered) }
            #expect(replies.count == calls)
            try #require(await a.owner.close())
            await #expect(throws: (any Error).self) { try await SelectedCloudControlInspection().current() }

            active = b
            let probe = InitialCloudEvents()
            try await ChangeFanout.shared.subscribe(probe)
            defer { ChangeFanout.shared.unsubscribe(probe); probe.finish() }
            await b.factory.inspectCreation { _ in
                let control = try await SelectedCloudControlInspection().current()
                try #require(control.requiredScanComplete && control.provenance == .retainedAdoption)
                try #require(control.activation == .proven && control.ownedBackup == .proven)
            }
            await b.owner.start()
            await b.sync.prepareConnectionAuthority()
            let old = b.owner.generation
            try await b.owner.prepareCloudConnectionPreview()
            await b.sync.waitForConnectionPreview()
            guard case .ready(let preview, let receiver) = b.sync.preview else { throw ServiceWorkflowFailure.wire }
            try #require(receiver.localCollectionPresent && !receiver.initialAdoptionRequired)
            try #require(receiver.decision == .chooseCollection || receiver.decision == .confirmAdoption)
            await b.sync.confirmAdoption()
            await b.sync.waitForConnectionPreview()
            await b.sync.waitForReplacement()
            await b.owner.waitForCloudActivation()
            await b.sync.prepareConnectionAuthority()
            await b.sync.waitForInputs()
            if b.sync.operationFailed || b.owner.generation == old {
                print("selected-adoption-progress \(DebugCloudPreviewProbe.ownWork.adoptionProgressTag) stage=\(DebugCloudPreviewProbe.ownWork.lastEnteredWorkStageTag ?? "none")")
            }
            try #require(b.owner.state == .ready && b.owner.generation > old && !b.sync.operationFailed)
            let adopted = try await SelectedCloudControlInspection().current()
            try #require(adopted.generation == b.owner.generation && adopted.status == .proven)
            try #require(adopted.provenance == .retainedAdoption && adopted.activation == .proven && adopted.ownedBackup == .proven)
            try #require(adopted.bindingMatchesEpoch == .proven && adopted.requiredScanComplete)
            #expect(adopted.activeSHA256 == published.activeSHA256 && adopted.epochSHA256 == published.epochSHA256)
            #expect(adopted.publicationComplete == nil && adopted.publicationDoneActive == nil)
            #expect(probe.events.contains { event in
                if case .collectionReplaced = event.kind { return event.generation == adopted.generation }; return false
            })
            let peerReadback = try await peerRole.validateOwned()
            #expect(peerReadback.generation == adopted.generation && !peerReadback.qChanged)
            #expect(peerReadback.noteDigest == sourceReadback.noteDigest)
            try #require(b.backends.count == 2 && b.backends[0].closed)
            await #expect(throws: (any Error).self) { try await sourceConnection.confirmAdoption(preview) }
            try #require(await b.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        } catch { await active?.cleanup(); _ = await process.cancel(); throw error }
    }

    @Test func selectedBoundHostFixedEditUsesNormalUpdaterAndCommittedRevisionReadback() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-bound-edit-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let input = try await selectedBootstrapSource(root: root)
        let original = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
        let serviceDirectory = root.appending(path: "service")
        try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, pageSize: 32)
        var active: ServiceAppHarness?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let a = try ServiceAppHarness(root: root.appending(path: "A"), wire: wire, clock: .system)
            active = a
            defer { a.defaults.removePersistentDomain(forName: a.suite) }
            try FileManager.default.createDirectory(at: a.root, withIntermediateDirectories: false)
            try input.bytes.write(to: a.root.appending(path: "collection.anki2"))
            try await selectedBootstrapPublish(a)
            let before = try await original.validateOwned() // Binds the actual current Host.
            try #require(before.generation == a.owner.generation && !before.qChanged)
            let probe = InitialCloudEvents()
            try await ChangeFanout.shared.subscribe(probe)
            defer { ChangeFanout.shared.unsubscribe(probe); probe.finish() }
            let rowBefore = try await CoreNoteService().readNote(id: input.noteID)
            let trackedBefore = try #require(try await CoreSmokeInspection().cloud(noteID: input.noteID).entities.first { $0.label == "n1" })
            let edit = try await original.fixedEdit() // InteractiveLane -> actual registered job -> notes::update_in.
            try #require(edit.generation == before.generation && edit.seq > before.seq)
            let committed = try await probe.next(after: before.seq, stage: "selected bound edit committed Changes") { event in
                event.generation == edit.generation && event.seq == edit.seq
                    && { if case .changes = event.kind { return true }; return false }()
            }
            guard case .changes(let changes) = committed.kind else { throw ServiceWorkflowFailure.wire }
            #expect(changes.source == .userUndoable && changes.notes.count == 1 && changes.cards.isEmpty)
            #expect(changes.notes.first?.id == input.noteID && changes.notes.first?.change == .updated)
            try #require(edit.source == .userUndoable && edit.updatedNotes == 1 && edit.undoName != nil)
            let revision = try #require(edit.localRevision)
            try #require(revision > 0 && revision > trackedBefore.revision)
            try await CoreCollectionService().flushForBackground()
            await a.sync.drainOperations()
            let owned = try await original.validateOwned()
            let cloud = try await CoreSmokeInspection().cloud(noteID: input.noteID)
            let tracked = try #require(cloud.entities.first { $0.label == "n1" })
            #expect(owned.generation == edit.generation && owned.seq >= edit.seq && owned.qChanged)
            #expect(owned.noteDigest == edit.noteDigest && owned.noteDigest != original.facts.noteDigest)
            #expect(cloud.generation == edit.generation && cloud.seq >= edit.seq)
            #expect(tracked.revision == revision && tracked.digest == edit.noteDigest)
            let rowAfter = try await CoreNoteService().readNote(id: input.noteID)
            #expect(rowAfter == NoteRecord(id: rowBefore.id, notetypeId: rowBefore.notetypeId,
                fields: ["Generated-sync-now-n1-Q", rowBefore.fields[1]], tags: rowBefore.tags, guid: rowBefore.guid))
            try #require(await a.owner.close())
            try await a.reopen()
            let reopened = try await original.validateOwned()
            #expect(reopened.generation == a.owner.generation && reopened.generation > edit.generation)
            #expect(reopened.qChanged && reopened.noteDigest == edit.noteDigest)
            #expect(try await CoreNoteService().readNote(id: input.noteID) == rowAfter)
            let durable = try #require(try await CoreSmokeInspection().cloud(noteID: input.noteID).entities.first { $0.label == "n1" })
            #expect(durable.revision == revision && durable.digest == edit.noteDigest)
            #expect(try Data(contentsOf: root.appending(path: "source/collection.anki2")) == input.bytes)
            try #require(await a.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        } catch { await active?.cleanup(); _ = await process.cancel(); throw error }
    }

    @Test func selectedPublicationUsesActualServiceMatchingConflictsAndRejectsInvalidConflictCompletion() async throws {
        for matching in [true, false] {
            let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-conflict-\(UUID().uuidString)")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
            defer { try? FileManager.default.removeItem(at: root) }
            let input = try await selectedBootstrapSource(root: root)
            let serviceDirectory = root.appending(path: "service")
            try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false)
            let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, pageSize: 32)
            var active: ServiceAppHarness?
            do {
                let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
                let a = try ServiceAppHarness(root: root.appending(path: "A"), wire: wire)
                active = a
                defer { a.defaults.removePersistentDomain(forName: a.suite) }
                try FileManager.default.createDirectory(at: a.root, withIntermediateDirectories: false)
                try input.bytes.write(to: a.root.appending(path: "collection.anki2"))
                a.ownTransport = SelectedConflictTransport(original: ServiceOwnTransport(wire: wire), matching: matching)
                let replies = SelectedBootstrapReplies(terminalKind: matching ? .doneActive : .completeEpoch)
                a.observeOwn = { replies.record($0) }
                replies.observeFailure(a.sync)
                if matching { print("selected-start-before failed=\(a.sync.operationFailed) setup=\(a.sync.setupState) diagnostic=\(DebugCloudDiagnostics.latestValue ?? "none")") }
                await a.owner.start()
                await a.sync.prepareConnectionAuthority()
                replies.observeOriginalBackend(a.sync)
                if matching { print("selected-start-after failed=\(a.sync.operationFailed) setup=\(a.sync.setupState) diagnostic=\(DebugCloudDiagnostics.latestValue ?? "none")") }
                try await a.owner.prepareCloudConnectionPreview()
                await a.sync.waitForConnectionPreview()
                guard case .ready(_, let summary) = a.sync.preview else { throw ServiceWorkflowFailure.wire }
                try #require(summary.decision == .confirmUpload)
                await a.sync.confirmPublication()
                try await replies.waitForOriginalTerminal()
                await a.sync.waitForConnectionPreview()
                await a.sync.waitForInputs()
                let control = try await SelectedCloudControlInspection().current()
                if matching {
                    print("selected-publication-conflict operationFailed=\(a.sync.operationFailed) status=\(control.status) controlProven=\(control.status == .proven) completePresent=\(control.publicationComplete != nil) donePresent=\(control.publicationDoneActive != nil)")
                    print("selected-publication-diagnostic \(DebugCloudDiagnostics.latestValue ?? "none")")
                    print("selected-publication-work tag=\(DebugCloudPreviewProbe.ownWork.latestTag) stage=\(DebugCloudPreviewProbe.ownWork.lastEnteredWorkStageTag ?? "none")")
                    try #require(!a.sync.operationFailed && control.status == .proven)
                    try #require(control.provenance == .runtimeAcceptedPublication)
                    let complete = try #require(control.publicationComplete)
                    let done = try #require(control.publicationDoneActive)
                    #expect(complete.origin == .conflict && done.origin == .conflict)
                    try replies.requireOriginalCommit(complete, before: done)
                } else {
                    #expect(control.status != .proven && control.publicationComplete == nil && control.publicationDoneActive == nil)
                    try replies.requireConflictWithoutCompletion()
                }
                try #require(await a.owner.close())
                let exit = await process.close()
                try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            } catch { await active?.cleanup(); _ = await process.cancel(); throw error }
        }
    }

    @Test func selectedAdoptionRefusalFailedPageAndLateCancellationJoinWithoutReplacement() async throws {
        for mode in [SelectedAdoptionPageDelivery.Mode.refused, .failed, .lateCancelled] {
            let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-adoption-failure-\(UUID().uuidString)")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
            defer { try? FileManager.default.removeItem(at: root) }
            let input = try await selectedBootstrapSource(root: root)
            let peerRole = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
            let serviceDirectory = root.appending(path: "service")
            try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false)
            let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, pageSize: 32)
            let delivery = SelectedAdoptionPageDelivery(mode: mode)
            var active: ServiceAppHarness?
            var confirmation: Task<Void, Never>?
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
                active = b
                b.ownTransport = SelectedAdoptionTransport(original: ServiceOwnTransport(wire: wire), delivery: delivery)
                let probe = InitialCloudEvents()
                try await ChangeFanout.shared.subscribe(probe)
                defer { ChangeFanout.shared.unsubscribe(probe); probe.finish() }
                await b.owner.start()
                await b.sync.prepareConnectionAuthority()
                let originalBackend = try b.backend
                let old = b.owner.generation
                let baseline = try await peerRole.validateOwned()
                try await b.owner.prepareCloudConnectionPreview()
                await b.sync.waitForConnectionPreview()
                guard case .ready(_, let summary) = b.sync.preview else { throw ServiceWorkflowFailure.wire }
                try #require(summary.decision == .chooseCollection || summary.decision == .confirmAdoption)
                let before = probe.events.last?.seq ?? 0
                if mode == .refused { b.sync.setDeviceSettings(paused: true, automatic: false, media: true) }
                confirmation = Task { await b.sync.confirmAdoption() }
                if mode == .lateCancelled {
                    // The real Service frame has replied. Cancellation releases
                    // only its held native delivery and joins the original owner.
                    try #require(try await serviceAppSignal(delivery.reached.stream, stage: "selected adoption original page reply"))
                    try #require(await b.sync.cancelAndDrain())
                }
                await confirmation?.value
                await b.sync.waitForConnectionPreview()
                await b.sync.waitForReplacement()
                await b.owner.waitForCloudActivation()
                await b.sync.prepareConnectionAuthority()
                await b.sync.waitForInputs()
                // These are the same terminal barriers used by the native
                // adapter. No replacement stream is awaited after this point.
                #expect(b.owner.state == .ready && b.owner.generation == old)
                #expect(!probe.events.contains { event in
                    guard event.seq > before, case .collectionReplaced = event.kind else { return false }
                    return event.generation > old
                })
                let retained = try await peerRole.validateOwned()
                #expect(!retained.qChanged && retained.noteDigest == baseline.noteDigest)
                #expect(b.backends.count == 1 && b.backends[0] === originalBackend)
                switch mode {
                case .refused: #expect(b.sync.preview != nil && !originalBackend.closed)
                case .failed: #expect(b.sync.operationFailed && !originalBackend.closed && delivery.drained)
                case .lateCancelled: #expect(originalBackend.closed && delivery.drained)
                }
                try #require(await b.owner.close())
                #expect(originalBackend.closed)
                let exit = await process.close()
                try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            } catch {
                delivery.release()
                await active?.cleanup()
                await confirmation?.value
                _ = await process.cancel()
                throw error
            }
        }
    }
}

@MainActor func selectedBootstrapPublish(_ a: ServiceAppHarness) async throws {
    let replies = SelectedBootstrapReplies()
    a.observeOwn = { replies.record($0) }
    replies.observeFailure(a.sync)
    await a.owner.start()
    await a.sync.prepareConnectionAuthority()
    replies.observeOriginalBackend(a.sync)
    try await a.owner.prepareCloudConnectionPreview()
    await a.sync.waitForConnectionPreview()
    guard case .ready(_, let summary) = a.sync.preview else { throw ServiceWorkflowFailure.wire }
    try #require(summary.decision == .confirmUpload)
    await a.sync.confirmPublication()
    try await replies.waitForOriginalTerminal()
    await a.sync.waitForConnectionPreview()
    await a.sync.waitForInputs()
    let control = try await SelectedCloudControlInspection().current()
    try #require(!a.sync.operationFailed && control.status == .proven && control.requiredScanComplete)
    let complete = try #require(control.publicationComplete)
    let done = try #require(control.publicationDoneActive)
    try replies.requireOriginalCommit(complete, before: done)
}

private nonisolated final class SelectedAdoptionPageDelivery: Sendable {
    enum Mode: Sendable, Equatable { case refused, failed, lateCancelled }
    let mode: Mode
    let reached = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    private let terminal = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    private let state = Mutex(false)
    var drained: Bool { state.withLock { $0 } }
    init(mode: Mode) { self.mode = mode }
    func release() { terminal.continuation.yield(true); terminal.continuation.finish() }
    func afterOriginalReply() async throws {
        state.withLock { $0 = true }
        reached.continuation.yield(true)
        if mode == .lateCancelled {
            for await _ in terminal.stream { break }
            throw CancellationError()
        }
        throw CloudOwnTransportFailure.unexpectedReply
    }
}
private nonisolated struct SelectedAdoptionTransport: CloudOwnTransport {
    let original: ServiceOwnTransport
    let delivery: SelectedAdoptionPageDelivery
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        let actual = try original.prepare(command)
        if case .adoptionPage = command { return SelectedAdoptionOperation(actual: actual, delivery: delivery) }
        return actual
    }
}
private nonisolated final class SelectedAdoptionOperation: CloudPreparedOwnOperation, Sendable {
    let actual: any CloudPreparedOwnOperation
    let delivery: SelectedAdoptionPageDelivery
    init(actual: any CloudPreparedOwnOperation, delivery: SelectedAdoptionPageDelivery) { self.actual = actual; self.delivery = delivery }
    func enqueue() -> Bool { actual.enqueue() }
    func cancel() { actual.cancel(); delivery.release() }
    func reply() async throws -> CloudOwnTransportReply {
        _ = try await actual.reply()
        try await delivery.afterOriginalReply()
        throw CloudOwnTransportFailure.unexpectedReply
    }
}

private nonisolated struct SelectedConflictTransport: CloudOwnTransport {
    let original: ServiceOwnTransport
    let matching: Bool
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        let actual = try original.prepare(command)
        guard case .publication(.save(let target, let bytes, let fields)) = command,
              target.name == "active" || (target.name == "epoch" && fields != nil) else { return actual }
        let competing = CloudOwnTransportCommand.publication(.save(target,
            encrypted: matching ? bytes : Data([0]), systemFields: fields))
        return SelectedConflictOperation(first: try original.prepare(competing), actual: actual)
    }
}
private nonisolated final class SelectedConflictOperation: CloudPreparedOwnOperation, Sendable {
    let first: any CloudPreparedOwnOperation
    let actual: any CloudPreparedOwnOperation
    init(first: any CloudPreparedOwnOperation, actual: any CloudPreparedOwnOperation) { self.first = first; self.actual = actual }
    func enqueue() -> Bool { first.enqueue() && actual.enqueue() }
    func cancel() { first.cancel(); actual.cancel() }
    func reply() async throws -> CloudOwnTransportReply {
        // Drain both accepted frames even if the first delivery fails.
        var firstFailure: (any Error)?
        do { _ = try await first.reply() } catch { firstFailure = error }
        let reply = try await actual.reply()
        if let firstFailure { throw firstFailure }
        return reply
    }
}

@MainActor func selectedBootstrapSource(root: URL) async throws -> (bytes: Data, noteID: Int64) {
    let source = root.appending(path: "source")
    let location = CoreBridge.CollectionLocation(collectionDirectory: source, backupsDirectory: root.appending(path: "source-backups"),
                              appGroupSnapshot: root.appending(path: "widget"))
    try FileManager.default.createDirectory(at: source, withIntermediateDirectories: false)
    try FileManager.default.createDirectory(at: location.backupsDirectory, withIntermediateDirectories: false)
    let schema = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
    try FileManager.default.copyItem(at: schema, to: source.appending(path: "collection.anki2"))
    let collection = CoreCollectionService()
    let noteID: Int64
    do {
        try await collection.open(location, randomSeed: 329)
        let allExisting = try await CoreDeckService().allDeckNames().decks.map(\.id)
        let deletion = try await CoreDeckService().startDeleteDecks(ids: allExisting)
        _ = try await deletion.value()
        _ = try await CoreDeckService().renameDeck(id: 1, name: "Generated-D1")
        let deck = try await CoreDeckService().addDeck(name: "Generated-D1::Generated-D2")
        let type = try await CoreNotetypeService().addNotetype(.init(name: "QA", cloze: false, fields: ["Q", "A"],
        templates: [.init(name: "Face", front: "{{Q}}", back: "{{A}}", browserFront: "", browserBack: "")], css: nil))
        let note = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: ["Generated-Q", "Generated-A"], tags: [],
                            deckId: deck.id, guid: nil, updateAddDefaults: false))
        noteID = note.id
        try await collection.flushForBackground()
        try await collection.close()
    } catch { try? await collection.close(); throw error }
    let bytes = try Data(contentsOf: source.appending(path: "collection.anki2"))
    _ = try await OriginalCloudRoleInspection.inspect(main: bytes, wal: nil)
    return (bytes, noteID)
}

nonisolated final class SelectedBootstrapReplies: Sendable {
    private let values = Mutex<[CloudOwnObservation]>([])
    private let terminalKind: SelectedCloudPublicationEvent.Kind
    // Installed before backend creation; a fast original commit stays buffered
    // until the test waits. This observes progress and supplies no authority.
    private let terminal = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    init(terminalKind: SelectedCloudPublicationEvent.Kind = .doneActive) { self.terminalKind = terminalKind }
    deinit { terminal.continuation.finish() }
    var count: Int { values.withLock { $0.count } }
    func record(_ value: CloudOwnObservation) {
        values.withLock { $0.append(value) }
        switch value.phase {
        case .committed where value.publicationEvent?.kind == terminalKind:
            terminal.continuation.yield(true)
            terminal.continuation.finish()
        case .enqueued(false), .replied(.failed), .replied(.notStarted), .replied(.retryAfter):
            terminal.continuation.finish()
        default: break
        }
    }
    @MainActor func observeFailure(_ sync: SyncCoordinator) {
        if sync.operationFailed { terminal.continuation.finish(); return }
        // The sticky failure has no successful reset. Any original failure
        // ends this wait through serviceAppSignal's existing missing-reply path.
        withObservationTracking { _ = sync.operationFailed } onChange: { [self] in
            terminal.continuation.finish()
        }
    }
    @MainActor func observeOriginalBackend(_ sync: SyncCoordinator) {
        guard sync.connectionService != nil else { terminal.continuation.finish(); return }
        // Cancellation/drain or replacement ends the original backend lifetime.
        withObservationTracking { _ = sync.connectionService } onChange: { [self] in
            terminal.continuation.finish()
        }
    }
    func waitForOriginalTerminal() async throws {
        do {
            try #require(try await serviceAppSignal(terminal.stream, stage: "selected original publication Core result"))
        } catch {
            let workflow = CoreCloudWorkflowService()
            if let snapshot = try? await workflow.durableSnapshot() {
                let rows = snapshot.entities
                print("selected-terminal-rows total=\(rows.count) marked=\(rows.filter { $0.mark != nil }.count) ackedBehind=\(rows.filter { $0.ackedRev < $0.localRev }.count) blocked=\(rows.filter { $0.mark != nil || $0.ackedRev < $0.localRev }.count) inFlightRows=\(rows.filter { !$0.inFlight.isEmpty }.count) inbox=\(snapshot.inboxNames.count) stopped=\(snapshot.trackingStopped)")
            }
            if let connection = try? await workflow.connection(), let recovery = try? await connection.recoveryStatus() {
                print("selected-terminal-recovery step=\(recovery.step) switchConflict=\(recovery.switchConflict) retryPresent=\(recovery.retryAfterMilliseconds != nil)")
            }
            if let policy = try? await workflow.policy(), let status = try? await policy.status() {
                print("selected-terminal-policy pending=\(status.pending) held=\(status.heldBack) inFlight=\(status.inFlight) incoming=\(status.incomingWaiting) uploadFiles=\(String(describing: status.media.uploadFiles)) missing=\(String(describing: status.media.missingNames)) unknown=\(String(describing: status.media.unknownBlobs))")
            }
            if let control = try? await SelectedCloudControlInspection().current() {
                print("selected-terminal-control status=\(control.status) scan=\(control.requiredScanComplete)")
            }
            print("selected-terminal-work tag=\(DebugCloudPreviewProbe.ownWork.latestTag) stage=\(DebugCloudPreviewProbe.ownWork.lastEnteredWorkStageTag ?? "none") diagnostic=\(DebugCloudDiagnostics.latestValue ?? "none")")
            throw error
        }
    }
    func requireConflictWithoutCompletion() throws {
        try values.withLock { values in
            let conflict = try #require(values.first { value in
                if case .replied(.conflict) = value.phase { return value.publicationEvent?.kind == .completeEpoch }; return false
            })
            try #require(values.contains { value in
                if case .committed = value.phase {
                    return value.identity == conflict.identity && value.publicationEvent == conflict.publicationEvent
                }
                return false
            })
            #expect(!values.contains { $0.publicationEvent?.kind == .doneActive })
        }
    }
    func requireOriginalCommit(_ complete: SelectedCloudPublicationEvent, before done: SelectedCloudPublicationEvent) throws {
        try values.withLock { values in
            func index(_ event: SelectedCloudPublicationEvent) throws -> Int {
                let committed = values.indices.filter { index in
                    if case .committed = values[index].phase { return values[index].publicationEvent == event }; return false
                }
                let i = try #require(committed.count == 1 ? committed.first : nil)
                let original = values[i]
                let phases = values[..<i].filter { $0.identity == original.identity }
                try #require(phases.count == 3 && original.metadata.attempt == event.attempt)
                try #require(phases.allSatisfy { $0.metadata.operationID == original.metadata.operationID
                    && $0.metadata.attempt == original.metadata.attempt && $0.metadata.kind == original.metadata.kind })
                guard case .prepared = phases[0].phase, case .enqueued(true) = phases[1].phase,
                      case .replied(let reply) = phases[2].phase, phases[2].publicationEvent == event else { throw ServiceWorkflowFailure.wire }
                switch (reply, event.origin) {
                case (.saved, .saved), (.conflict, .conflict): break
                default: throw ServiceWorkflowFailure.wire
                }
                try #require(original.metadata.kind == (event.kind == .completeEpoch ? .saveCompleteEpoch : .saveActive))
                return i
            }
            let completeIndex = try index(complete), doneIndex = try index(done)
            try #require(completeIndex < doneIndex)
            try #require(values[completeIndex].metadata.operationID == values[doneIndex].metadata.operationID)
        }
    }
}
#endif
