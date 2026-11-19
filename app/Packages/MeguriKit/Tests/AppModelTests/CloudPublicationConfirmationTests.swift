import Foundation
import Synchronization
import Testing
@testable import AppModel
@testable import CoreBridge

extension ReviewCollectionStartupTests {
    @Test func waitingPublicationStopReportsBusyAndExplicitCancelRetainsEnabledSetting() async throws {
        let harness = try PublicationPresentationHarness()
        defer { harness.removeStorage() }
        var original: Task<Void, Never>?
        do {
            try await harness.prepare()
            original = Task { await harness.sync.confirmPublication() }
            try await harness.transport.zones.waitForAdmission()
            let before = try #require(try await harness.connection().recoveryStatus())
            #expect(!before.adoptionRecoveredCancelOnly && SyncStatusPresentation.showsRecoveryResume(before))
            #expect(!before.adoptionAfterReview && !before.adoptionTransportWait)
            #expect(SyncStatusPresentation.cancellationMessageKey(before) == "sync.cancelOp.publish.message")
            // Projection coverage only: reuse the real issued handle without
            // executing it. The Rust crash regression proves the recovered fact.
            for (recovered, transportWait, afterReview) in [
                (false, false, false), (false, true, false), (false, false, true),
                (true, false, false), (true, true, true)
            ] {
                let projected = CoreBridge.CloudRecoveryStatus(.init(
                    operation: before.operation.raw, step: .ready,
                    switchSent: false, switchConflict: false, retryAfterMs: nil,
                    adoptionTransportWait: transportWait, adoptionCancellable: true,
                    adoptionAfterReview: afterReview,
                    adoptionRecoveredCancelOnly: recovered,
                    publicationConflictCancellable: false, publicationCancellable: false))
                #expect(projected.adoptionRecoveredCancelOnly == recovered)
                #expect(projected.adoptionTransportWait == transportWait)
                #expect(projected.adoptionAfterReview == afterReview)
                #expect(SyncStatusPresentation.showsRecoveryResume(projected) == (!recovered && !afterReview && transportWait))
                #expect(SyncStatusPresentation.recoveryStatusKey(projected)
                    == (recovered ? "sync.attention.adoptUnproven"
                        : afterReview ? "sync.attention.adoptAfterReview" : "sync.publish.preparing"))
                if recovered {
                    #expect(SyncText.value(SyncStatusPresentation.recoveryStatusKey(projected))
                        == "Meguri couldn’t confirm which collection file is current, so syncing stopped.")
                } else if afterReview {
                    #expect(SyncText.value(SyncStatusPresentation.recoveryStatusKey(projected))
                        == "Replacing this collection with the iCloud collection continues when you finish reviewing.")
                }
                #expect(SyncStatusPresentation.cancellationMessageKey(projected)
                    == (recovered ? "sync.cancelOp.adoptUnproven.message" : "sync.cancelOp.adopt.message"))
            }
            #expect(before.step == .bound && before.publicationCancellable)
            let settings = harness.sync.settings
            await harness.sync.stopUsingICloud()
            #expect(harness.sync.connectionFailure == .busy && harness.sync.settings == settings)
            #expect(!harness.transport.zones.cancelled && harness.transport.zones.enqueues == 1)
            harness.transport.zones.finish(.publication(.failed(retryAfterMilliseconds: nil)))
            await original?.value
            let waiting = try #require(try await harness.connection().recoveryStatus())
            #expect(waiting.step == .bound && waiting.publicationCancellable && !waiting.publicationConflictCancellable)
            #expect(!waiting.adoptionRecoveredCancelOnly && SyncStatusPresentation.showsRecoveryResume(waiting))
            #expect(!waiting.adoptionAfterReview && !waiting.adoptionTransportWait)
            await harness.sync.stopUsingICloud()
            #expect(harness.sync.connectionFailure == .busy && harness.sync.settings == settings)
            #expect(try await harness.connection().recoveryStatus()?.step == .bound)
            await harness.sync.cancelConnectionOperation()
            #expect(harness.sync.connectionFailure == nil && harness.sync.recovery == nil)
            #expect(try await harness.connection().recoveryStatus() == nil)
            #expect(harness.sync.settings == settings && !harness.sync.connectionOperationActive)
            #expect(await harness.owner.close())
        } catch { harness.release(); await original?.value; _ = await harness.owner.close(); throw error }
    }

    @Test(arguments: [false, true])
    func publicationPresentationReleasesAtDurableAcceptanceWhileOriginalTransportStaysOwned(backgroundThrows: Bool) async throws {
        let harness = try PublicationPresentationHarness()
        defer { harness.removeStorage() }
        let presentation = CloudPublicationConfirmation()
        var returned = false, confirmations = 0
        harness.sync.observePublicationConfirmation = { confirmations += 1 }
        var original: Task<Void, Never>?
        do {
            try await harness.prepare()
            harness.owner.navigation.present(.cloudSync(.replaceCloud))
            #expect(harness.owner.navigation.sheet == .cloudSync(.replaceCloud))
            original = Task { await presentation.confirm(using: harness.sync); returned = true }
            try await harness.transport.zones.waitForAdmission()
            #expect(presentation.accepted && !presentation.confirming && presentation.submitted)
            #expect(presentation.backgroundErrorKey == nil)
            #expect(!returned && harness.sync.connectionOperationActive && harness.sync.settings.enabled)
            #expect(try await harness.connection().recoveryStatus() != nil)
            await presentation.confirm(using: harness.sync)
            await harness.sync.confirmPublication() // consumed preview cannot start another Core call
            #expect(confirmations == 1 && harness.transport.zones.enqueues == 1)
            #expect(presentation.close())
            harness.owner.navigation.dismissSheet()
            #expect(harness.owner.navigation.sheet == nil && !harness.transport.zones.cancelled)
            harness.transport.zones.finish(.publication(.zonesCreated))
            try await harness.transport.save.waitForAdmission()
            #expect(!returned && harness.sync.connectionOperationActive)
            #expect(harness.transport.save.enqueues == 1 && !harness.transport.save.cancelled)
            // A different presentation cannot inherit the accepted attempt.
            let nextPresentation = CloudPublicationConfirmation()
            #expect(!nextPresentation.submitted && !nextPresentation.confirming)
            if backgroundThrows { harness.transport.save.fail() }
            else { harness.transport.save.finish(.publication(.failed(retryAfterMilliseconds: nil))) }
            await original?.value
            #expect(returned && !harness.sync.connectionOperationActive)
            #expect(presentation.accepted && harness.sync.settings.enabled)
            #expect(try await harness.connection().recoveryStatus() != nil)
            if backgroundThrows { #expect(harness.sync.operationFailed) }
            #expect(presentation.backgroundErrorKey == nil) // Close invalidated this presentation's attempt.
            #expect(confirmations == 1 && !nextPresentation.submitted)
            #expect(await harness.owner.close())
        } catch { harness.release(); await original?.value; _ = await harness.owner.close(); throw error }
    }

    @Test(arguments: [false, true])
    func publicationRefusalIsAttemptSpecificAndHistoricalFailureDoesNotBecomeCurrentFailure(backgroundThrows: Bool) async throws {
        let harness = try PublicationPresentationHarness()
        defer { harness.removeStorage() }
        let refused = CloudPublicationConfirmation()
        var original: Task<Void, Never>?
        do {
            try await harness.prepare()
            guard case .ready(let handle, _) = harness.sync.preview else { throw CloudOwnTransportFailure.unexpectedReply }
            try await harness.connection().cancelPreview(handle)
            await refused.confirm(using: harness.sync)
            #expect(!refused.confirming && !refused.submitted && refused.refused)
            #expect(refused.failure == .consumed && harness.sync.operationFailed)
            #expect(!harness.sync.settings.enabled && !harness.sync.connectionOperationActive)
            #expect(try await harness.connection().recoveryStatus() == nil)
            harness.sync.beginConnectionPreview()
            await harness.sync.waitForConnectionPreview()
            #expect(harness.sync.operationFailed && harness.sync.connectionFailure == nil)
            refused.clearRefusal()
            #expect(!refused.refused && refused.failure == nil)
            #expect(refused.backgroundErrorKey == nil)
            original = Task { await refused.confirm(using: harness.sync) }
            try await harness.transport.zones.waitForAdmission()
            #expect(refused.accepted && !refused.refused && refused.failure == nil)
            #expect(harness.sync.operationFailed) // history is preserved
            #expect(refused.backgroundErrorKey == nil) // History is not a failure of this accepted attempt.
            harness.owner.navigation.present(.cloudSync(.replaceCloud))
            harness.transport.zones.finish(.publication(.zonesCreated))
            try await harness.transport.save.waitForAdmission()
            #expect(refused.backgroundErrorKey == nil && harness.sync.connectionOperationActive)
            if backgroundThrows { harness.transport.save.fail() }
            else { harness.transport.save.finish(.publication(.failed(retryAfterMilliseconds: nil))) }
            harness.release()
            await original?.value
            #expect(refused.backgroundErrorKey == (backgroundThrows ? "sync.recovery.error.other" : nil))
            #expect(refused.accepted && !refused.refused && refused.failure == nil)
            #expect(harness.sync.operationFailed && harness.sync.settings.enabled && !harness.sync.connectionOperationActive)
            #expect(try await harness.connection().recoveryStatus() != nil)
            #expect(!harness.transport.save.cancelled && harness.transport.save.enqueues == 1)
            let nextPresentation = CloudPublicationConfirmation()
            #expect(nextPresentation.backgroundErrorKey == nil && !nextPresentation.submitted)
            #expect(refused.close())
            harness.owner.navigation.dismissSheet()
            #expect(harness.owner.navigation.sheet == nil)
            #expect(await harness.owner.close())
        } catch { harness.release(); await original?.value; _ = await harness.owner.close(); throw error }
    }

    @Test(arguments: [false, true])
    func publicationBeforeCoreReturnIsSingleFlightAndStalePresentationCannotReceiveResult(changeGeneration: Bool) async throws {
        let harness = try PublicationPresentationHarness()
        defer { harness.removeStorage() }
        let presentation = CloudPublicationConfirmation()
        let entered = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let release = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        var confirmations = 0
        harness.sync.observePublicationConfirmation = {
            confirmations += 1; entered.continuation.yield(true)
            for await _ in release.stream { break }
        }
        var original: Task<Void, Never>?
        do {
            try await harness.prepare()
            original = Task { await presentation.confirm(using: harness.sync) }
            try #require(try await publicationSignal(entered.stream))
            #expect(presentation.confirming && presentation.submitted && !presentation.close())
            #expect(!harness.sync.settings.enabled && confirmations == 1)
            await presentation.confirm(using: harness.sync)
            await harness.sync.confirmPublication()
            #expect(confirmations == 1)
            if changeGeneration {
                harness.sync.collectionReplaced(harness.sync.generation + 1)
                await harness.sync.waitForInputs()
            }
            if !changeGeneration { presentation.invalidate() }
            release.continuation.yield(()); release.continuation.finish()
            harness.release()
            await original?.value
            #expect(!presentation.accepted && !presentation.refused)
            if changeGeneration {
                #expect(presentation.confirming) // no stale-generation result was delivered
                presentation.invalidate() // mirrors the view's generation observer
            }
            #expect(!presentation.confirming)
            #expect(confirmations == 1)
            if changeGeneration { await harness.sync.waitForReplacement() }
            #expect(await harness.owner.close())
        } catch {
            presentation.invalidate(); release.continuation.yield(()); release.continuation.finish()
            harness.release(); await original?.value; _ = await harness.owner.close(); throw error
        }
    }

    @Test func retainedPublicationStartupDiscoversBeforeProofAndContinuesOriginalOrderedWorkOnce() async throws {
        let harness = try PublicationPresentationHarness(realAccountProof: true)
        var releaseCommand: CheckedContinuation<Void, Never>?
        var commandReturned = false, heldStatus = false
        let hold = PublicationStatusHold()
        var heldCommand: CloudPolicyCommand?
        var stage = "retain-publication"
        var waiting = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let entered = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        defer { harness.removeStorage() }
        do {
            let operationID = try await harness.retainPublicationAndClose()
            let proof = harness.accounts.holdNextRead()
            harness.beforeExecution = { command in
                guard case .statusChanged = command.action, hold.armed, !heldStatus else { return }
                heldStatus = true
                heldCommand = command
                await withCheckedContinuation {
                    releaseCommand = $0
                    entered.continuation.yield(true)
                }
            }
            harness.afterExecution = { command, result in
                guard heldCommand?.raw === command.raw else { return }
                #expect(result.error == nil && !result.deferred)
                commandReturned = true
            }
            harness.freshCoordinator()
            harness.sync.observeAccountPolicyDrain = { stage in
                if case .waiting = stage { waiting.continuation.yield(true) }
            }
            stage = "startup-owner-open"
            await harness.owner.start()
            stage = "startup-account-proof-admission"
            try await proof.waitForAdmission(stage: "startup-account-proof-admission")
            // The real resolver is admitted first; hold a later actual status.
            hold.armed = true
            let settings = harness.sync.settings
            // P25.2: the update hold, as a stored pause beside the retained op. It
            // changes the status, and nothing of the op may start while it is set.
            harness.sync.setDeviceSettings(paused: true, automatic: settings.automatic)
            try #require(try await publicationSignal(entered.stream, stage: "startup-issued-status-entry"))
            #expect(harness.sync.recovery?.step == .bound)
            #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
            #expect(!harness.sync.connectionOperationActive)
            proof.finish()
            stage = "startup-policy-drain"
            try #require(try await publicationSignal(waiting.stream, stage: "startup-policy-drain"))
            #expect(heldStatus && !commandReturned)
            #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
            releaseCommand?.resume(); releaseCommand = nil
            stage = "startup-update-hold"
            await harness.sync.prepareConnectionAuthority()
            #expect(harness.sync.updateHold && harness.sync.recovery?.step == .bound)
            #expect(harness.sync.serviceChangeRefusedForOperation, "V27.5: Off is refused while the op exists")
            #expect(!(await harness.sync.turnOffICloud()) && harness.sync.providerState?.selection == .iCloud)
            #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
            #expect(!harness.sync.connectionOperationActive)
            // The person's Resume is a clearing act: it writes `paused: false` first.
            await harness.sync.resumeRecoveryOperation()
            #expect(!harness.sync.updateHold)
            stage = "startup-account-proof-completion"
            await harness.sync.prepareConnectionAuthority()
            harness.expectSuccessfulAccountCompletion(stage: "startup-account-proof-completion")
            stage = "startup-create-zones-admission"
            try await harness.transport.zones.waitForAdmission(stage: "startup-create-zones-admission")
            #expect(commandReturned && harness.sync.connectionOperationActive)
            #expect(harness.observations.operationIDs == Set([operationID]))
            #expect(harness.transport.zones.enqueues == 1)
            // Observe the resolved repeat read behind the original own task before joining it.
            stage = "startup-repeat-account-proof"
            waiting = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            await harness.sync.accountChanged()
            try #require(try await publicationSignal(waiting.stream, stage: "startup-repeat-account-proof"))
            #expect(harness.transport.zones.enqueues == 1)
            #expect(harness.sync.connectionOperationActive && harness.transport.save.enqueues == 0)
            #expect(harness.observations.operationIDs == Set([operationID]))
            #expect(harness.accountCompletion == nil)
            harness.transport.zones.finish(.publication(.zonesCreated))
            stage = "startup-save-control-admission"
            try await harness.transport.save.waitForAdmission(stage: "startup-save-control-admission")
            #expect(harness.transport.save.enqueues == 1)
            #expect(harness.observations.operationIDs == Set([operationID]))
            #expect(harness.observations.kinds.contains { if case .saveMediaRoot = $0 { true } else { false } })
            stage = "startup-durable-bound-status"
            #expect(try await harness.connection().recoveryStatus()?.step == .bound)
            harness.release()
            stage = "startup-own-work-drain"
            await harness.sync.waitForConnectionPreview()
            stage = "startup-repeat-account-proof-completion"
            await harness.sync.prepareConnectionAuthority()
            harness.expectSuccessfulAccountCompletion(stage: "startup-repeat-account-proof-completion")
            stage = "startup-owner-close"
            #expect(await harness.owner.close())
        } catch {
            Issue.record("Startup check stopped during \(stage)")
            releaseCommand?.resume(); harness.accounts.release(); harness.release()
            _ = await harness.owner.close()
            throw error
        }
    }

    @Test(arguments: RetainedPublicationProofBlock.allCases)
    func retainedPublicationStartupRejectsUnsuccessfulOrObsoleteProof(block: RetainedPublicationProofBlock) async throws {
        let harness = try PublicationPresentationHarness(realAccountProof: true)
        defer {
            harness.sync.observeAccountPolicyDrain = nil
            harness.removeStorage()
        }
        do {
            let operationID = try await harness.retainPublicationAndClose(disabled: block == .disabled)
            let accountReads = harness.accounts.reads
            let proof = block == .disabled ? nil : harness.accounts.holdNextRead()
            harness.freshCoordinator()
            await harness.owner.start()
            if let proof {
                try await proof.waitForAdmission()
            } else {
                // Disabled provider authority forbids automatic account reads.
                // Join the real startup inputs and their original command ACKs.
                await harness.sync.prepareConnectionAuthority()
                await harness.sync.drainOperations()
                #expect(harness.sync.providerState?.settings?.enabled == false)
                #expect(harness.sync.providerState?.permitsICloudStartup == false)
                #expect(harness.accountReadCount == 0 && harness.accounts.reads == accountReads)
                #expect(harness.accountCompletion == nil && harness.observations.operationIDs.isEmpty)
            }
            #expect(harness.sync.recovery?.step == .bound)
            #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
            var successorProof: PublicationAccountProof?
            switch block {
            case .paused:
                harness.sync.setDeviceSettings(paused: true, automatic: false)
            case .stale:
                successorProof = harness.accounts.holdNextRead()
                await harness.sync.accountChanged()
            case .replaced:
                successorProof = harness.accounts.holdNextRead()
                harness.sync.collectionReplaced(harness.sync.generation + 1)
                await harness.sync.waitForInputs()
            case .failed, .disabled: break
            }
            if let proof {
                if block == .failed { proof.fail() } else { proof.finish() }
            }
            if block == .replaced { await harness.sync.waitForReplacement() }
            if let successorProof {
                try await successorProof.waitForAdmission()
                // Even an available reply for the original attachment grants
                // nothing to its successor or to a newer account-read generation.
                #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
                successorProof.fail()
            }
            await harness.sync.prepareConnectionAuthority()
            #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
            #expect(!harness.sync.connectionOperationActive)
            #expect(harness.sync.recovery?.step == .bound)
            #expect(try await harness.connection().recoveryStatus()?.step == .bound)
            if block == .paused {
                // The successful proof while paused left this same durable
                // operation pending. Unpause must resolve a new native proof.
                let freshProof = harness.accounts.holdNextRead()
                harness.sync.setDeviceSettings(paused: false, automatic: false)
                try await freshProof.waitForAdmission(stage: "unpause-account-proof-admission")
                #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
                #expect(!harness.sync.connectionOperationActive)
                #expect(try await harness.connection().recoveryStatus()?.step == .bound)
                freshProof.finish()
                await harness.sync.prepareConnectionAuthority()
                harness.expectSuccessfulAccountCompletion(stage: "unpause-account-proof-completion")
                try await harness.transport.zones.waitForAdmission(stage: "unpause-create-zones-admission")
                #expect(harness.transport.zones.enqueues == 1 && harness.transport.save.enqueues == 0)
                #expect(harness.observations.operationIDs == Set([operationID]))
                // Observe the repeat proof waiting for the original own task before joining it.
                let waiting = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                harness.sync.observeAccountPolicyDrain = { stage in
                    if case .waiting = stage { waiting.continuation.yield(true) }
                }
                await harness.sync.accountChanged()
                try #require(try await publicationSignal(waiting.stream, stage: "unpause-repeat-account-proof"))
                #expect(harness.transport.zones.enqueues == 1)
                #expect(harness.sync.connectionOperationActive && harness.transport.save.enqueues == 0)
                #expect(harness.observations.operationIDs == Set([operationID]))
                #expect(harness.accountCompletion == nil)
                harness.transport.zones.finish(.publication(.zonesCreated))
                try await harness.transport.save.waitForAdmission(stage: "unpause-save-control-admission")
                #expect(harness.transport.save.enqueues == 1)
                #expect(harness.observations.operationIDs == Set([operationID]))
                #expect(harness.observations.kinds.contains { if case .saveMediaRoot = $0 { true } else { false } })
                harness.release()
                await harness.sync.waitForConnectionPreview()
                await harness.sync.prepareConnectionAuthority()
                harness.expectSuccessfulAccountCompletion(stage: "unpause-repeat-account-proof-completion")
            }
            #expect(await harness.owner.close())
        } catch {
            harness.accounts.release(); harness.release(); _ = await harness.owner.close()
            throw error
        }
    }

    @Test func retainedPublicationStartupNilRecoveryStaysIdleAcrossFreshAttachment() async throws {
        let harness = try PublicationPresentationHarness(realAccountProof: true)
        defer { harness.removeStorage() }
        await harness.owner.start()
        await harness.sync.prepareConnectionAuthority()
        await harness.sync.drainOperations()
        #expect(harness.sync.providerState?.selection == .localOnly)
        #expect(harness.sync.providerState?.settings?.enabled == false && !harness.sync.settings.enabled)
        #expect(harness.sync.providerState?.permitsICloudStartup == false)
        #expect(harness.accountReadCount == 0 && harness.accounts.reads == 0)
        #expect(harness.accountCompletion == nil && harness.observations.operationIDs.isEmpty)
        #expect(harness.sync.recovery == nil)
        #expect(!harness.sync.connectionOperationActive)
        #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
        #expect(await harness.owner.close())
        let accountReads = harness.accounts.reads
        harness.freshCoordinator()
        await harness.owner.start()
        await harness.sync.prepareConnectionAuthority()
        await harness.sync.drainOperations()
        #expect(harness.sync.recovery == nil)
        #expect(harness.sync.providerState?.selection == .localOnly)
        #expect(harness.sync.providerState?.settings?.enabled == false && !harness.sync.settings.enabled)
        #expect(harness.sync.providerState?.permitsICloudStartup == false)
        #expect(harness.accountReadCount == 0 && harness.accounts.reads == accountReads)
        #expect(harness.accountCompletion == nil && harness.observations.operationIDs.isEmpty)
        #expect(harness.sync.recovery == nil && !harness.sync.connectionOperationActive)
        #expect(harness.transport.zones.enqueues == 0 && harness.transport.save.enqueues == 0)
        #expect(await harness.owner.close())
    }
}

@MainActor private final class PublicationPresentationHarness {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    var transport = PublicationPresentationTransport()
    let accounts = PublicationAccountFactory()
    var observations = PublicationOwnObservations()
    let suite = "synthetic-publication-startup-\(UUID().uuidString)"
    let defaults: UserDefaults
    var beforeExecution: (@MainActor (CloudPolicyCommand) async -> Void)?
    var afterExecution: (@MainActor (CloudPolicyCommand, SyncCommandResult) -> Void)?
    private var backend: PublicationStartupBackend?
    private var providerDirectory: URL { root.appending(path: "synthetic-installation") }
    lazy var sync = makeSync()

    private func makeSync() -> SyncCoordinator {
        SyncCoordinator(bootstrap: {
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-publication-presentation", environment: .development,
                status: .couldNotDetermine, identity: nil)
        }, makeBackend: { [self] persisted in
            let observations = self.observations
            let native = try await NativeCollectionCloudBackend.make(workflow: CoreCloudWorkflowService(), factory: accounts,
                ownTransport: transport, mediaTransport: PublicationUnusedMediaTransport(), persisted: persisted,
                observeOwn: { observations.record($0) })
            let backend = PublicationStartupBackend(native: native)
            backend.beforeExecution = beforeExecution
            backend.afterExecution = afterExecution
            self.backend = backend
            return backend
        }, defaults: defaults, clock: .system, directory: nil, observeSignals: false,
           providerDirectory: providerDirectory)
    }
    lazy var owner: ReviewCollection = makeOwner()
    private func makeOwner() -> ReviewCollection {
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        return ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: providerDirectory)
    }
    init(realAccountProof _: Bool = false) throws {
        defaults = try #require(UserDefaults(suiteName: suite))
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.copyItem(at: source, to: root.appending(path: "collection.anki2"))
    }
    func prepare() async throws {
        await owner.start()
        try #require(owner.state == .ready)
        try await owner.prepareCloudConnectionPreview()
        await sync.waitForConnectionPreview()
        guard case .ready(_, let summary) = sync.preview else { throw CloudOwnTransportFailure.unexpectedReply }
        try #require(summary.decision == .confirmUpload)
    }
    func retainPublicationAndClose(disabled: Bool = false) async throws -> Data {
        try await prepare()
        let original = Task { await sync.confirmPublication() }
        do {
            try await transport.zones.waitForAdmission(stage: "retain-publication-create-zones-admission")
            let operationID = try #require(observations.operationIDs.first)
            // A terminal failure keeps the accepted durable row at bound.
            transport.zones.finish(.publication(.failed(retryAfterMilliseconds: nil)))
            await original.value
            try #require(try await connection().recoveryStatus()?.step == .bound)
            if disabled {
                // Persist through the original live provider, preserving the
                // retained operation rather than changing legacy defaults.
                let provider = try #require(backend?.providerService)
                let settings = try #require(try await provider.refresh().settings)
                let saved = try await provider.saveSettings(.init(enabled: false, paused: settings.paused,
                    automatic: settings.automatic, media: settings.media))
                try #require(saved.settings?.enabled == false && !saved.permitsICloudStartup)
                let refreshed = try await provider.refresh()
                try #require(refreshed.selection == .iCloud && refreshed.settings?.enabled == false)
                try #require(try await connection().recoveryStatus()?.step == .bound)
                try #require(observations.operationIDs == Set([operationID]))
            }
            try #require(await owner.close())
            return operationID
        } catch { release(); await original.value; throw error }
    }
    func freshCoordinator() {
        transport = PublicationPresentationTransport()
        observations = PublicationOwnObservations()
        sync = makeSync(); owner = makeOwner()
    }
    func connection() throws -> CoreCloudConnectionService { try #require(backend?.connection) }
    var accountReadCount: Int? { backend?.accountReadCount }
    var accountCompletion: PublicationAccountCompletion? { backend?.accountCompletion }
    func expectSuccessfulAccountCompletion(stage: String) {
        let completion = accountCompletion
        let recoveryStep = sync.recovery.map { String(describing: $0.step) } ?? "none"
        #expect(completion?.accountVerified == true && completion?.errorPresent == false,
            "\(stage) completionObserved=\(completion != nil) accountVerified=\(completion?.accountVerified ?? false) errorPresent=\(completion?.errorPresent ?? false) runtimeTrackingFailed=\(completion?.runtimeTrackingFailed ?? false) connectionOperationActive=\(sync.connectionOperationActive) operationFailed=\(sync.operationFailed) recoveryStep=\(recoveryStep) ownObservationCount=\(observations.kinds.count)")
    }
    func release() {
        transport.zones.finish(.publication(.failed(retryAfterMilliseconds: nil)))
        transport.save.finish(.publication(.failed(retryAfterMilliseconds: nil)))
    }
    func removeStorage() {
        defaults.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: root)
    }
}

private nonisolated final class PublicationPresentationTransport: CloudOwnTransport, Sendable {
    let zones = PublicationPresentationGate(), save = PublicationPresentationGate()
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        switch command {
        case .read: PublicationImmediateReply(value: .read(.zoneMissing))
        case .publication(.createZones): zones
        case .publication(.save): save
        default: throw CloudOwnTransportFailure.unexpectedReply
        }
    }
}
private nonisolated final class PublicationImmediateReply: CloudPreparedOwnOperation, Sendable {
    let value: CloudOwnTransportReply
    init(value: CloudOwnTransportReply) { self.value = value }
    func enqueue() -> Bool { true }
    func cancel() {}
    func reply() async throws -> CloudOwnTransportReply { value }
}
private nonisolated struct PublicationTestFailure: Error, CustomStringConvertible {
    let stage: String
    init(stage: String = "transport-failure") { self.stage = stage }
    var description: String { "Publication test failed at stage: \(stage)" }
}
private nonisolated final class PublicationPresentationGate: CloudPreparedOwnOperation, Sendable {
    private let state = Mutex((enqueues: 0, cancelled: false))
    private let admission = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    private let terminal = AsyncStream<Result<CloudOwnTransportReply, PublicationTestFailure>>.makeStream(bufferingPolicy: .bufferingNewest(1))
    var enqueues: Int { state.withLock { $0.enqueues } }
    var cancelled: Bool { state.withLock { $0.cancelled } }
    func enqueue() -> Bool { state.withLock { $0.enqueues += 1 }; admission.continuation.yield(true); return true }
    func cancel() { state.withLock { $0.cancelled = true } }
    func finish(_ value: CloudOwnTransportReply) { terminal.continuation.yield(.success(value)); terminal.continuation.finish() }
    func fail() { terminal.continuation.yield(.failure(PublicationTestFailure())); terminal.continuation.finish() }
    func waitForAdmission(stage: String = "transport-admission") async throws {
        try #require(try await publicationSignal(admission.stream, stage: stage))
    }
    func reply() async throws -> CloudOwnTransportReply {
        for await result in terminal.stream { return try result.get() }
        throw CloudOwnTransportFailure.missingReply
    }
}
private nonisolated func publicationSignal(_ stream: AsyncStream<Bool>, stage: String = "publication-signal") async throws -> Bool {
    try await withThrowingTaskGroup(of: Bool.self) { group in
        group.addTask { for await value in stream { return value }; throw CloudOwnTransportFailure.missingReply }
        group.addTask { try await Task.sleep(for: .seconds(10)); throw PublicationTestFailure(stage: stage) }
        defer { group.cancelAll() }
        let value: Bool? = try await group.next()
        return try #require(value as Bool?)
    }
}

nonisolated enum RetainedPublicationProofBlock: CaseIterable, Sendable { case failed, stale, replaced, paused, disabled }

private nonisolated struct PublicationAccountCompletion {
    let accountVerified: Bool
    let errorPresent: Bool
    let runtimeTrackingFailed: Bool
}

// Native account proof and policy execution stay intact. Only delivery of an
// original command may be held, as in the initial-adoption ordered-proof tests.
@MainActor private final class PublicationStartupBackend: SyncCoordinatorBackend {
    let native: NativeCollectionCloudBackend
    var beforeExecution: (@MainActor (CloudPolicyCommand) async -> Void)?
    var afterExecution: (@MainActor (CloudPolicyCommand, SyncCommandResult) -> Void)?
    private(set) var accountCompletion: PublicationAccountCompletion?
    private(set) var accountReadCount = 0
    var connection: CoreCloudConnectionService? { native.connection }
    var own: CloudOwnExecutor? { native.own }
    var providerService: CoreProviderService? { native.providerService }
    func stopSyncing() async throws { try await native.stopSyncing() }
    func setProofAdmission(_ admission: @escaping @Sendable (CloudOriginalCommandProofYield) async throws -> Void,
                           acknowledgeOutputs: @escaping @Sendable ([CloudPolicyOutput]) async -> Void) async {
        await native.setProofAdmission(admission, acknowledgeOutputs: acknowledgeOutputs)
    }
    init(native: NativeCollectionCloudBackend) { self.native = native }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput { try await native.input(event) }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        await execute(command, corePrepared: {})
    }
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void) async -> SyncCommandResult {
        await beforeExecution?(command)
        let result = await native.execute(command, corePrepared: corePrepared)
        afterExecution?(command, result)
        return result
    }
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void,
                 acknowledgeOutputs: @escaping @MainActor @Sendable ([CloudPolicyOutput]) async -> Void,
                 performInput: @escaping @MainActor @Sendable (@escaping @Sendable () async throws -> CloudPolicyOutput) async throws -> CloudPolicyOutput) async -> SyncCommandResult {
        await beforeExecution?(command)
        let result = await native.execute(command, corePrepared: corePrepared,
            acknowledgeOutputs: acknowledgeOutputs, performInput: performInput)
        afterExecution?(command, result)
        return result
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 {
        accountReadCount += 1
        return try await native.beginAccountRead(transition: transition)
    }
    func readAccount(_ read: UInt64) async -> SyncCommandResult { await native.readAccount(read) }
    func prepareAccountRead(_ read: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult {
        accountCompletion = nil
        let complete = await native.prepareAccountRead(read)
        return { admitted in
            let result = await complete(admitted)
            self.accountCompletion = .init(accountVerified: result.accountVerified, errorPresent: result.error != nil,
                runtimeTrackingFailed: (result.error as? BridgeError) == .unavailable(reason: "cloud runtime tracking failed"))
            return result
        }
    }
    func takeAccountReadRequest() async -> Bool { await native.takeAccountReadRequest() }
    func freezeForSuspend() async throws { try await native.freezeForSuspend() }
    func resumeAfterCoreResume() async { await native.resumeAfterCoreResume() }
    func quiesce() async throws { try await native.quiesce() }
    func close() async throws { try await native.close() }
}

@MainActor private final class PublicationAccountFactory: CloudKitEngineFactory {
    private(set) var reads = 0
    private var proofs: [PublicationAccountProof] = []
    private var allProofs: [PublicationAccountProof] = []
    func holdNextRead() -> PublicationAccountProof {
        let proof = PublicationAccountProof()
        proofs.append(proof); allProofs.append(proof)
        return proof
    }
    func resolveAccount() async throws -> CloudResolvedAccount? {
        reads += 1
        if !proofs.isEmpty { return try await proofs.removeFirst().resolve() }
        return CloudResolvedAccount(status: .available, identity: Data("synthetic-publication-account".utf8))
    }
    func makeEngine(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate) async throws -> any CloudKitEngine {
        // These tests stop at the original bound step's next control write,
        // before zones can open engine admission.
        throw CloudOwnTransportFailure.unexpectedReply
    }
    func release() { for proof in allProofs { proof.fail() } }
}

private nonisolated final class PublicationAccountProof: Sendable {
    private let admission = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    private let terminal = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    func waitForAdmission(stage: String = "account-proof-admission") async throws {
        try #require(try await publicationSignal(admission.stream, stage: stage))
    }
    func finish() { terminal.continuation.yield(true); terminal.continuation.finish() }
    func fail() { terminal.continuation.yield(false); terminal.continuation.finish() }
    func resolve() async throws -> CloudResolvedAccount {
        admission.continuation.yield(true)
        guard try await publicationSignal(terminal.stream, stage: "account-proof-reply") else { throw PublicationTestFailure() }
        return .init(status: .available, identity: Data("synthetic-publication-account".utf8))
    }
}

private nonisolated final class PublicationOwnObservations: Sendable {
    private let values = Mutex<[CloudOwnMetadata]>([])
    var operationIDs: Set<Data> { values.withLock { Set($0.map(\.operationID)) } }
    var kinds: [CloudOwnKind] { values.withLock { $0.map(\.kind) } }
    func record(_ observation: CloudOwnObservation) {
        if case .enqueued(true) = observation.phase { values.withLock { $0.append(observation.metadata) } }
    }
}
private nonisolated struct PublicationUnusedMediaTransport: CloudMediaTransport {
    func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead { throw CloudOwnTransportFailure.unexpectedReply }
}

@MainActor private final class PublicationStatusHold {
    var armed = false
}
