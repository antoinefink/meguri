#if os(macOS) && DEBUG
@testable import AppModel
@testable import CoreBridge
import Foundation
import Instrumentation
import Observation
import Synchronization
import Testing

// Real CoreHost, coordinator, adapter and Service. Generated local data only;
// transport execution uses the Service's actual original replies, never verdicts.
extension ReviewCollectionStartupTests {
    @Test(arguments: [0, 1, 2, 3, 4])
    func heldOriginalActiveRefusesNativeCancellationBeforeDrainAndRevalidatesConfirmation(mode: Int) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-held-active-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let input = try await selectedBootstrapSource(root: root)
        let service = root.appending(path: "service")
        try FileManager.default.createDirectory(at: service, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: service, pageSize: 32)
        let beforeSwitch = SelectedSwitchReplyGate(), switchReply = SelectedSwitchReplyGate()
        var active: ServiceAppHarness?, publication: Task<Void, Never>?, cancellation: Task<Void, Never>?
        var ownDrain: Task<Void, Never>?, accountChange: Task<Void, Never>?, postCancelDrain: Task<Void, Never>?
        let coreCancels = Mutex(0)
        let observeCancel: @Sendable (BridgeCall) -> Void = { call in
            if call == .cloudConnectionCancelOperation { coreCancels.withLock { $0 += 1 } }
        }
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let a = try ServiceAppHarness(root: root.appending(path: "A"), wire: wire, clock: .system)
            active = a
            defer { a.defaults.removePersistentDomain(forName: a.suite) }
            try FileManager.default.createDirectory(at: a.root, withIntermediateDirectories: false)
            try input.bytes.write(to: a.root.appending(path: "collection.anki2"))
            a.ownTransport = SelectedHeldSwitchTransport(original: ServiceOwnTransport(wire: wire),
                beforeSwitch: beforeSwitch, switchReply: switchReply, loseReply: mode == 1)
            let activeCommitted = Mutex(false)
            let originalReplies = SelectedBootstrapReplies()
            a.observeOwn = { observation in
                originalReplies.record(observation)
                if case .saveActive = observation.metadata.kind, case .committed = observation.phase {
                    activeCommitted.withLock { $0 = true }
                }
            }
            await a.owner.start(); await a.sync.prepareConnectionAuthority()
            try await a.owner.prepareCloudConnectionPreview()
            await a.sync.waitForConnectionPreview()
            guard case .ready(_, let summary) = a.sync.preview else { throw ServiceWorkflowFailure.wire }
            try #require(summary.decision == .confirmUpload)
            let originalGeneration = a.owner.generation
            publication = Task { await a.sync.confirmPublication() }
            try #require(try await serviceAppSignal(beforeSwitch.reached.stream, stage: "original ReadOwnEpoch reply held"))
            let oldDialog = try #require(await a.sync.prepareConnectionCancellation())
            #expect(oldDialog.recovery.publicationCancellable)
            #expect(!oldDialog.recovery.switchSent)
            #expect(SyncStatusPresentation.cancellationMessageKey(oldDialog.recovery) == "sync.cancelOp.publish.message")
            beforeSwitch.release()
            try #require(try await serviceAppSignal(switchReply.reached.stream, stage: "original accepted Active reply held"))
            let backend = try a.backend
            let connection = try #require(backend.connection)
            let executor = try #require(backend.own)
            let before = try #require(try await connection.recoveryStatus())
            #expect(before.switchSent)
            #expect(!before.publicationCancellable)
            #expect(before.step == .complete)
            #expect(a.sync.recovery?.switchSent == false, "The actual coordinator snapshot precedes SaveActive")
            #expect(a.sync.connectionOperationActive)
            #expect(switchReply.enqueues == 1)
            #expect(switchReply.cancels == 0)
            #expect(!activeCommitted.withLock { $0 })
            // confirmPublication's first slice may finish at sendEntities. Join
            // the actual current/successor owner and its command/input ACKs.
            let originalGuard = try await backend.workflow.currentGuard()
            let originalSnapshot = try await backend.workflow.durableSnapshot()
            let originalSDK = try #require(await a.factory.engines.last)
            let sdkCancels = Mutex(0)
            await originalSDK.inspectNextCancellation { sdkCancels.withLock { $0 += 1 } }
            let drainAdmitted = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let drainFinished = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let drained = Mutex(false), releasedOriginal = Mutex(false)
            let joinedOwner = Mutex<UUID?>(nil)
            let ownerJoined = Mutex(false)
            let admittedAccount = Mutex(false), acceptedAccount = Mutex(false)
            let oldOwnerBookkeeping = Mutex(false)
            let changedRead = Mutex<UInt64?>(nil)
            let accountPending = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let accountFinished = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            defer {
                a.sync.observeAccountPolicyDrain = nil; a.sync.observeCausalBoundary = nil
                accountPending.continuation.finish(); accountFinished.continuation.finish()
                drainAdmitted.continuation.finish(); drainFinished.continuation.finish()
                backend.afterInput = nil; backend.beforeExecution = nil; backend.afterExecution = nil
                backend.beforeAdmittedAccountCompletion = nil; backend.observeCoreCall = nil
            }
            a.sync.observeCausalBoundary = { boundary, _ in
                switch boundary {
                case .ownDrainWaiting(let owner) where joinedOwner.withLock({ $0 == nil }):
                    // This event is inside drainOperations, immediately before
                    // awaiting the current SaveActive owner, not an outer Task.
                    joinedOwner.withLock { $0 = owner }
                    #expect(a.sync.connectionOperationActive)
                    #expect(!releasedOriginal.withLock { $0 })
                    #expect(!switchReply.replyReturned)
                    #expect(!activeCommitted.withLock { $0 })
                    #expect(switchReply.enqueues == 1)
                    drainAdmitted.continuation.yield(true); drainAdmitted.continuation.finish()
                case .ownDrainJoined(let owner) where joinedOwner.withLock({ $0 == owner }):
                    #expect(releasedOriginal.withLock { $0 })
                    #expect(switchReply.replyReturned)
                    #expect(activeCommitted.withLock { $0 } == (mode != 1))
                    ownerJoined.withLock { $0 = true }
                case .accountCompletion(let read, let followup, _, _, _, _, let admitted, _, _, _, _)
                    where mode >= 2 && admitted && !followup && changedRead.withLock({ $0 == nil }):
                    changedRead.withLock { $0 = read }
                    #expect(releasedOriginal.withLock { $0 })
                    #expect(activeCommitted.withLock { $0 })
                    // Admission is observed before the async completion wrapper
                    // inspects Core, and before Core accepts the new account.
                    #expect(!oldOwnerBookkeeping.withLock { $0 })
                    admittedAccount.withLock { $0 = true }
                case .accountAccepted(let read, let verified, let succeeded)
                    where changedRead.withLock({ $0 == read }):
                    #expect(admittedAccount.withLock { $0 })
                    #expect(oldOwnerBookkeeping.withLock { $0 })
                    #expect(!verified)
                    #expect(succeeded == (mode != 2))
                    acceptedAccount.withLock { $0 = true }
                    accountFinished.continuation.yield(true); accountFinished.continuation.finish()
                default: break
                }
            }
            backend.beforeAdmittedAccountCompletion = { read in
                guard mode >= 2 else { return }
                do {
                    try #require(changedRead.withLock { $0 == read })
                    try #require(admittedAccount.withLock { $0 })
                    try #require(releasedOriginal.withLock { $0 } && switchReply.replyReturned)
                    try #require(activeCommitted.withLock { $0 })
                    try #require(!oldOwnerBookkeeping.withLock { $0 })
                    let saved = try await SelectedCloudControlInspection().current()
                    let complete = try #require(saved.publicationComplete)
                    let done = try #require(saved.publicationDoneActive)
                    try originalReplies.requireOriginalCommit(complete, before: done)
                    try #require(done.origin == .saved)
                    try #require(saved.status == .proven && saved.provenance == .runtimeAcceptedPublication)
                    try #require(saved.bindingMatchesEpoch == .proven && saved.requiredScanComplete)
                    let currentGuard = try await backend.workflow.currentGuard()
                    let snapshot = try await backend.workflow.durableSnapshot()
                    let policy = try await backend.workflow.policy()
                    let status = try await policy.status()
                    try #require(currentGuard == originalGuard)
                    try #require(snapshot.binding == originalSnapshot.binding)
                    try #require(status.accountStatus == .available && status.accountMatchesBinding)
                    try #require(a.owner.generation == originalGeneration)
                    try #require(try a.backend === backend)
                    try #require(!acceptedAccount.withLock { $0 })
                    oldOwnerBookkeeping.withLock { $0 = true }
                } catch is ExpectationFailedError {
                    // #require already recorded this strict failure.
                } catch { Issue.record(error) }
            }
            ownDrain = Task {
                await a.sync.drainOperations()
                #expect(ownerJoined.withLock { $0 })
                drained.withLock { $0 = true }
                drainFinished.continuation.yield(true); drainFinished.continuation.finish()
            }
            try #require(try await serviceAppSignal(drainAdmitted.stream, stage: "drain actually awaiting current SaveActive owner"))
            #expect(!drained.withLock { $0 })
            a.sync.observeAccountPolicyDrain = { stage in
                if case .waiting = stage { accountPending.continuation.yield(true) }
            }
            if mode >= 2 {
                let status: CloudAccountStatus = mode == 2 ? .temporarilyUnavailable : mode == 3 ? .noAccount : .available
                let identity = mode == 4 ? Data("synthetic-different-account".utf8) : nil
                let reads = await a.factory.accountReadCount
                await a.factory.resolveNextAccountRead(with: .success(.init(status: status, identity: identity)))
                accountChange = Task { await a.sync.accountChanged() }
                try #require(try await serviceAppSignal(accountPending.stream, stage: "live account BeginRead resolved, completion fenced by original SaveActive"))
                #expect(await a.factory.accountReadCount == reads + 1)
                #expect(!admittedAccount.withLock { $0 })
                #expect(!acceptedAccount.withLock { $0 })
            }
            let refused = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            cancellation = Task {
                await MaintenanceCallObservation.$capture.withValue(observeCancel) {
                    do { try await executor.cancel(oldDialog.recovery.operation); Issue.record("Outstanding switch cancelled") }
                    catch { #expect(error as? BridgeError == .cloudConnection(.busy)) }
                    await a.sync.cancelConnectionOperation(confirmation: oldDialog)
                    #expect(a.sync.connectionFailure == .busy)
                    #expect(await a.sync.prepareConnectionCancellation() == nil)
                }
                refused.continuation.yield(true); refused.continuation.finish()
            }
            try #require(try await serviceAppSignal(refused.stream, stage: "Busy before original Active callback"))
            await cancellation?.value
            #expect(coreCancels.withLock { $0 } == 0)
            #expect(switchReply.cancels == 0)
            #expect(!drained.withLock { $0 })
            #expect(!ownerJoined.withLock { $0 })
            #expect(!switchReply.replyReturned)
            #expect(sdkCancels.withLock { $0 } == 0)
            #expect(a.sync.connectionOperationActive)
            #expect(switchReply.enqueues == 1)
            #expect(a.owner.generation == originalGeneration)
            #expect(try a.backend === backend)
            #expect(!activeCommitted.withLock { $0 })
            let retained = try #require(try await connection.recoveryStatus())
            #expect(retained.step == before.step)
            #expect(retained.switchSent)
            #expect(!retained.publicationCancellable)
            #expect(try await SelectedCloudControlInspection().current().publicationDoneActive == nil)
            #expect(!admittedAccount.withLock { $0 })
            releasedOriginal.withLock { $0 = true }
            switchReply.release() // Only the original matching callback may settle ownership.
            try #require(try await serviceAppSignal(drainFinished.stream, stage: "original SaveActive own drain and ACKs finished"))
            await ownDrain?.value
            await publication?.value
            await accountChange?.value
            if mode >= 2 {
                try #require(try await serviceAppSignal(accountFinished.stream, stage: "actual live account reader completion after original SaveActive drain"))
                await a.sync.prepareConnectionAuthority()
            }
            await a.sync.waitForInputs(); await a.sync.drainOperations()
            #expect(drained.withLock { $0 })
            #expect(ownerJoined.withLock { $0 })
            #expect(switchReply.replyReturned)
            #expect(!a.sync.connectionOperationActive)
            #expect(switchReply.cancels == 0)
            #expect(switchReply.enqueues == 1)
            #expect(a.owner.generation == originalGeneration)
            if mode == 0 {
                #expect(try await connection.recoveryStatus() == nil)
                let accepted = try await SelectedCloudControlInspection().current()
                #expect(accepted.status == .proven)
                #expect(accepted.publicationDoneActive != nil)
                await MaintenanceCallObservation.$capture.withValue(observeCancel) {
                    await a.sync.cancelConnectionOperation(confirmation: oldDialog)
                }
                #expect(a.sync.connectionFailure == .busy)
                #expect(coreCancels.withLock { $0 } == 0)
            } else if mode == 1 {
                let settled = try #require(try await connection.recoveryStatus())
                #expect(settled.switchSent)
                #expect(settled.publicationCancellable)
                #expect(try await SelectedCloudControlInspection().current().publicationDoneActive == nil)
                await MaintenanceCallObservation.$capture.withValue(observeCancel) {
                    await a.sync.cancelConnectionOperation(confirmation: oldDialog)
                }
                #expect(a.sync.connectionFailure == .busy)
                #expect(coreCancels.withLock { $0 } == 0)
                #expect(try await connection.recoveryStatus() != nil)
                let fresh = try #require(await a.sync.prepareConnectionCancellation())
                #expect(fresh.recovery.switchSent)
                #expect(SyncStatusPresentation.cancellationMessageKey(fresh.recovery) == "sync.cancelOp.publish.message.switchSent")
                #expect(SyncText.value(SyncStatusPresentation.cancellationMessageKey(fresh.recovery))
                    != SyncText.value(SyncStatusPresentation.cancellationMessageKey(oldDialog.recovery)))
                // Every observed raw handle stays retained through its original
                // Core calls, result enqueue and ACK. The collector labels the
                // return context, which may include buffered commands.
                let stopTrace = ServiceStopEngineTrace()
                backend.afterInput = { stopTrace.input($0, output: $1) }
                backend.beforeExecution = { stopTrace.record($0, at: .begin) }
                backend.afterExecution = { command, result in
                    stopTrace.record(command, at: .end, error: result.error != nil)
                }
                backend.observeCoreCall = { stopTrace.record($0, at: .core($1)) }
                a.sync.observeCausalBoundary = { boundary, command in
                    guard let command else { return }
                    switch boundary {
                    case .commandResultEnqueue: stopTrace.record(command, at: .resultEnqueue)
                    case .commandAcknowledgementJoined: stopTrace.record(command, at: .acknowledgementJoined)
                    default: break
                    }
                }
                stopTrace.setPhase(.cancelling)
                await MaintenanceCallObservation.$capture.withValue(observeCancel) {
                    await a.sync.cancelConnectionOperation(confirmation: fresh)
                }
                stopTrace.setPhase(.cancelReturned)
                stopTrace.setPhase(.postCancelDrain)
                backend.trace("heldActive.phase=postCancelDrain", emit: true)
                let postCancelFinished = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                postCancelDrain = Task {
                    await a.sync.waitForInputs(); await a.sync.drainOperations()
                    postCancelFinished.continuation.yield(true); postCancelFinished.continuation.finish()
                }
                try #require(try await serviceAppSignal(postCancelFinished.stream, stage: "fresh cancel original policy inputs, results and ACKs before teardown"))
                await postCancelDrain?.value
                backend.trace("heldActive.postCancelDrain.finished", emit: true)
                #expect(coreCancels.withLock { $0 } == 1)
                #expect(a.sync.connectionFailure == nil)
                #expect(a.sync.recovery == nil)
                #expect(try await connection.recoveryStatus() == nil)
                #expect(try await SelectedCloudControlInspection().current().publicationDoneActive == nil)
                stopTrace.setPhase(.closing)
                backend.trace("heldActive.phase=closing", emit: true)
                try #require(await a.owner.close())
            } else {
                // BeginRead alone does not change owner (V9.1). The real lane
                // commits this Saved result first, then admits account proof.
                #expect(activeCommitted.withLock { $0 })
                #expect(admittedAccount.withLock { $0 })
                #expect(oldOwnerBookkeeping.withLock { $0 })
                #expect(acceptedAccount.withLock { $0 })
                #expect(try await connection.recoveryStatus() == nil)
                let changed = try await SelectedCloudControlInspection().current()
                #expect(changed.status != .proven)
                #expect(changed.publicationDoneActive == nil)
                await MaintenanceCallObservation.$capture.withValue(observeCancel) {
                    await a.sync.cancelConnectionOperation(confirmation: oldDialog)
                }
                #expect(a.sync.connectionFailure == .busy)
                #expect(coreCancels.withLock { $0 } == 0)
                let policy = try await backend.workflow.policy()
                let changedStatus = try await policy.status()
                #expect(changedStatus.accountStatus == (mode == 2 ? .temporary : mode == 3 ? .noAccount : .available))
                #expect(changedStatus.syncNow != .available)
                if mode == 2 { #expect(changedStatus.state == .iCloudUnavailable(.temporary)) }
                else if mode == 3 { #expect(changedStatus.state == .iCloudUnavailable(.noAccount)) }
                else { #expect(changedStatus.state == .needsAttention) }
                await #expect(throws: (any Error).self) { try await backend.workflow.validateSDK(originalSDK.instanceID) }
                // Restore via the actual provider/read completion. The accepted
                // same-account proof does not itself reactivate isolation.
                backend.beforeAdmittedAccountCompletion = nil
                let restoredRead = Mutex<UInt64?>(nil)
                let restoredAccount = Mutex(false)
                a.sync.observeCausalBoundary = { boundary, _ in
                    switch boundary {
                    case .accountCompletion(let read, let followup, _, _, _, _, let admitted, _, _, _, _)
                        where admitted && !followup && restoredRead.withLock({ $0 == nil }):
                        restoredRead.withLock { $0 = read }
                    case .accountAccepted(let read, _, let succeeded)
                        where restoredRead.withLock({ $0 == read }):
                        // verified reflects adapter delivery replay, which an
                        // isolated binding may refuse until explicit Resume.
                        #expect(succeeded)
                        restoredAccount.withLock { $0 = succeeded }
                    default: break
                    }
                }
                let readsBeforeRestore = await a.factory.accountReadCount
                await a.factory.resolveNextAccountRead(with: .success(.init(status: .available,
                    identity: Data("synthetic-initial-account".utf8))))
                await a.sync.accountChanged(); await a.sync.prepareConnectionAuthority(); await a.sync.drainOperations()
                #expect(await a.factory.accountReadCount == readsBeforeRestore + 1)
                #expect(restoredAccount.withLock { $0 })
                let restoredStatus = try await policy.status()
                #expect(restoredStatus.accountStatus == .available)
                // Isolation needs an explicit, binding-specific Resume. A
                // temporary unavailable read only closed the account gate.
                if mode >= 3 {
                    try await a.owner.prepareCloudConnectionPreview()
                    await a.sync.waitForConnectionPreview()
                    guard case .ready(_, let resume) = a.sync.preview else { throw ServiceWorkflowFailure.wire }
                    try #require(resume.decision == .confirmResume)
                    #expect(resume.resumeBindingId == originalGuard.binding)
                    await a.sync.confirmResume()
                    await a.sync.prepareConnectionAuthority(); await a.sync.drainOperations()
                }
                let restoredGuard = try await backend.workflow.currentGuard()
                #expect(restoredGuard.generation == originalGuard.generation && restoredGuard.binding == originalGuard.binding)
                let restoredSnapshot = try await backend.workflow.durableSnapshot()
                #expect(restoredSnapshot.binding == originalSnapshot.binding)
                let restoredAuthority = try await policy.status()
                #expect(restoredAuthority.accountStatus == .available && restoredAuthority.accountMatchesBinding)
                #expect(restoredAuthority.controls.bindingUsable == true)
                let restoredSDK = try #require(await a.factory.engines.last)
                try await backend.workflow.validateSDK(restoredSDK.instanceID)
                // No Sending-ready assertion: the resumed instance must still
                // satisfy the production fresh-Root path before later sends.
                #expect(switchReply.enqueues == 1)
                #expect(switchReply.cancels == 0)
                #expect(a.owner.generation == originalGeneration)
            }
            if mode != 1 { try #require(await a.owner.close()) }
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
        } catch {
            beforeSwitch.release(); switchReply.release()
            await cancellation?.value; await publication?.value
            await ownDrain?.value; await accountChange?.value; await postCancelDrain?.value
            await active?.cleanup()
            _ = await process.cancel()
            throw error
        }
    }

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
                if mode == .refused {
                    b.sync.setDeviceSettings(paused: true, automatic: false)
                    await b.sync.waitForInputs()
                    try #require(b.sync.settings.paused)
                    guard case .ready = b.sync.preview else { throw ServiceWorkflowFailure.wire }
                }
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

private nonisolated final class SelectedSwitchReplyGate: Sendable {
    let reached = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    private let delivery = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    private let state = Mutex((enqueues: 0, cancels: 0, replyReturned: false))
    var enqueues: Int { state.withLock { $0.enqueues } }
    var cancels: Int { state.withLock { $0.cancels } }
    var replyReturned: Bool { state.withLock { $0.replyReturned } }
    func returnedOriginalReply() { state.withLock { $0.replyReturned = true } }
    func enqueued() { state.withLock { $0.enqueues += 1 } }
    func cancelled() { state.withLock { $0.cancels += 1 } }
    func hold() async throws {
        reached.continuation.yield(true)
        try #require(try await serviceAppSignal(delivery.stream, stage: "original control callback release"))
    }
    func release() { delivery.continuation.yield(true); delivery.continuation.finish() }
}
private nonisolated struct SelectedHeldSwitchTransport: CloudOwnTransport {
    let original: ServiceOwnTransport
    let beforeSwitch: SelectedSwitchReplyGate
    let switchReply: SelectedSwitchReplyGate
    let loseReply: Bool
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        let actual = try original.prepare(command)
        if case .publication(.read(let target)) = command, target.recordType == "MGEpoch" {
            return SelectedHeldSwitchOperation(actual: actual, gate: beforeSwitch, loseReply: false, active: false)
        }
        if case .publication(.save(let target, _, _)) = command, target.recordType == "MGActive" {
            return SelectedHeldSwitchOperation(actual: actual, gate: switchReply, loseReply: loseReply, active: true)
        }
        return actual
    }
}
private nonisolated struct SelectedOriginalReplyLost: Error {}
private nonisolated final class SelectedHeldSwitchOperation: CloudPreparedOwnOperation, Sendable {
    let actual: any CloudPreparedOwnOperation
    let gate: SelectedSwitchReplyGate
    let loseReply: Bool
    let active: Bool
    init(actual: any CloudPreparedOwnOperation, gate: SelectedSwitchReplyGate, loseReply: Bool, active: Bool) {
        self.actual = actual; self.gate = gate; self.loseReply = loseReply; self.active = active
    }
    func enqueue() -> Bool {
        let accepted = actual.enqueue()
        if accepted { gate.enqueued() }
        return accepted
    }
    func cancel() { gate.cancelled(); actual.cancel() }
    func reply() async throws -> CloudOwnTransportReply {
        let reply = try await actual.reply()
        if active {
            guard case .publication(.saved(_)) = reply else { throw ServiceWorkflowFailure.wire }
        } else {
            guard case .publication(.read(.found(_))) = reply else { throw ServiceWorkflowFailure.wire }
        }
        try await gate.hold()
        defer { gate.returnedOriginalReply() }
        if loseReply { throw SelectedOriginalReplyLost() }
        return reply
    }
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
                              appGroupSnapshot: root.appending(path: "widget"),
                              manualTransferDirectory: root.appending(path: "source-provider-installation"))
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
