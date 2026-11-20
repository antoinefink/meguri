@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// `sync/meguri-provider` MP6.3 and MP6.9: what the coordinator does with the
/// core's row, with an answer, with changed facts and with a settled acceptance.
@MainActor @Suite struct MeguriSetupDecisionTests {
    private let rows: [MeguriSetupRow] = [.download, .downloadOverEmptyDevice, .uploadToEmptyServer, .ask, .askUploadOnly,
                                          .askDownloadOnly]
    private let allChoices: [MeguriSetupChoice] = [.noQuestion, .keepDevice, .keepServer]

    @Test func onlyRowsOneToThreeRunWithoutTheSheet() {
        #expect(SyncCoordinator.meguriSetupStep(.download) == .run(.noQuestion))
        #expect(SyncCoordinator.meguriSetupStep(.downloadOverEmptyDevice) == .run(.noQuestion))
        #expect(SyncCoordinator.meguriSetupStep(.uploadToEmptyServer) == .run(.noQuestion))
        #expect(SyncCoordinator.meguriSetupStep(.ask) == .ask)
        #expect(SyncCoordinator.meguriSetupStep(.askUploadOnly) == .ask)
        // Media files on a device with no collection are that device's content: row 1 is not taken.
        #expect(SyncCoordinator.meguriSetupStep(.askDownloadOnly) == .ask)
    }

    @Test func theSheetOffersBothCollectionsOrTheUploadAlone() {
        #expect(SyncCoordinator.meguriSetupChoices(.ask) == [.keepDevice, .keepServer])
        #expect(SyncCoordinator.meguriSetupChoices(.askUploadOnly) == [.keepDevice])
        #expect(SyncCoordinator.meguriSetupChoices(.askDownloadOnly) == [.keepServer])
        for row in [MeguriSetupRow.download, .downloadOverEmptyDevice, .uploadToEmptyServer] {
            #expect(SyncCoordinator.meguriSetupChoices(row).isEmpty)
        }
    }

    @Test func eachRowAndAnswerHasOneDirectionOrNone() {
        // true: the prepared preview is confirmed. false: the download replaces it.
        let expected: [String: Bool] = [
            "download/noQuestion": true, "uploadToEmptyServer/noQuestion": true,
            "ask/keepDevice": true, "askUploadOnly/keepDevice": true,
            "downloadOverEmptyDevice/noQuestion": false, "ask/keepServer": false,
            "askDownloadOnly/keepServer": true,
        ]
        for row in rows {
            for choice in allChoices {
                #expect(SyncCoordinator.meguriSetupConfirmsPreview(row, choice) == expected["\(row)/\(choice)"],
                        "\(row)/\(choice)")
            }
        }
        // A row that asks is never run with no question, and a row's own step is always runnable.
        for row in rows {
            switch SyncCoordinator.meguriSetupStep(row) {
            case .ask: #expect(SyncCoordinator.meguriSetupConfirmsPreview(row, .noQuestion) == nil)
            case .run(let choice): #expect(SyncCoordinator.meguriSetupConfirmsPreview(row, choice) != nil)
            }
            for choice in SyncCoordinator.meguriSetupChoices(row) {
                #expect(SyncCoordinator.meguriSetupConfirmsPreview(row, choice) != nil)
            }
        }
    }

    @Test func theDoneLineIsOnlyForARowThatAskedNothing() {
        #expect(SyncCoordinator.meguriSetupDone(.download) == .downloaded(backup: false))
        #expect(SyncCoordinator.meguriSetupDone(.downloadOverEmptyDevice) == .downloaded(backup: true))
        #expect(SyncCoordinator.meguriSetupDone(.uploadToEmptyServer) == .uploaded)
        #expect(SyncCoordinator.meguriSetupDone(.ask) == nil)
        #expect(SyncCoordinator.meguriSetupDone(.askUploadOnly) == nil)
        #expect(SyncCoordinator.meguriSetupDone(.askDownloadOnly) == nil)
    }

    @Test func changedFactsReloadBothSidesAndOtherRefusalsDoNot() {
        #expect(SyncCoordinator.meguriSetupChangeKey(.refused(.setupLocalChanged, accounting: nil))
            == "sync.meguri.setup.error.localChanged")
        #expect(SyncCoordinator.meguriSetupChangeKey(.refused(.setupRemoteChanged, accounting: nil))
            == "sync.meguri.setup.error.remoteChanged")
        for failure: MeguriBootstrapFailure in [.setupChoice, .busy, .storage, .cancelled, .connectionChanged] {
            #expect(SyncCoordinator.meguriSetupChangeKey(.refused(failure, accounting: nil)) == nil)
        }
        #expect(SyncCoordinator.meguriSetupChangeKey(.settled(.neither(Self.operation(.overtaken, .failed)))) == nil)
        // A collection that is closed or damaged is not a changed fact: nothing is read again.
        #expect(SyncCoordinator.meguriSetupChangeKey(.refused(.setupLocalUnusable, accounting: nil)) == nil)
        #expect(MeguriSyncStatusPresentation.failureKey(.setupLocalChanged) == "sync.meguri.setup.error.localChanged")
        #expect(MeguriSyncStatusPresentation.failureKey(.setupRemoteChanged) == "sync.meguri.setup.error.remoteChanged")
    }

    static func operation(_ acceptance: ManualTransferAcceptance, _ phase: ManualTransferPhase,
                          boundary: ManualTransferBoundary? = nil) -> MeguriBootstrapOperation {
        .init(detached: .init(direction: .export, phase: phase, acceptance: acceptance, boundary: boundary))
    }

    @Test func neitherIsSettledAsOvertakenAndOnlyAnUnreadableServerStaysUnknown() {
        // MP6.9: neither is one of the three recorded outcomes. It leads to the sheet.
        guard case .overtaken = MeguriSetupSequence.settled(.settled(.neither(Self.operation(.overtaken, .failed)))) else {
            Issue.record("neither must be settled as overtaken"); return
        }
        guard case .unknown = MeguriSetupSequence.settled(.refused(.unavailable, accounting: nil)) else {
            Issue.record("an unreadable server must stay unknown"); return
        }
        guard case .notAccepted = MeguriSetupSequence.settled(.settled(.notAccepted(Self.operation(.notSent, .failed)))) else {
            Issue.record("not accepted"); return
        }
        guard case .accepted(_, hintUnchanged: false) = MeguriSetupSequence.settled(
            .settled(.accepted(Self.operation(.exactOk, .needsDecision), localUnchanged: false))) else {
            Issue.record("accepted, with a device that changed"); return
        }
    }

    @Test func anOvertakenUploadShowsTheChooseRowAndItsCancelLeavesTheServiceOff() {
        // T19.5 "full sync required": needs you, with the action that opens the sheet.
        let row = SyncKinds.meguriRow(.init(connection: .setupRequired, stage: .failed,
                                            failureKey: MeguriSetupSequence.overtakenKey))
        #expect(row.kind == .needsYou)
        #expect(row.textKey == "sync.needsYou.meguri.chooseCollection")
        #expect(row.action == .chooseCollection)
        // A status action's sheet keeps the service on Cancel (V27.4), except
        // after an overtaken upload, whose setup never finished (MP6.3).
        #expect(MeguriSetupSequence.keepsService(requested: true, stage: .failed, failureKey: nil))
        #expect(MeguriSetupSequence.keepsService(requested: true, stage: nil, failureKey: MeguriSetupSequence.overtakenKey))
        #expect(!MeguriSetupSequence.keepsService(requested: true, stage: .failed, failureKey: MeguriSetupSequence.overtakenKey))
        #expect(!MeguriSetupSequence.keepsService(requested: false, stage: .required, failureKey: nil))
    }

    @Test func aClosedOrDamagedCollectionIsItsOwnNeedsYouRowWithAnAction() {
        #expect(MeguriSyncStatusPresentation.failureKey(.setupLocalUnusable) == MeguriSetupSequence.localUnusableKey)
        let row = SyncKinds.meguriRow(.init(connection: .setupRequired, stage: .failed,
                                            failureKey: MeguriSetupSequence.localUnusableKey))
        #expect(row.kind == .needsYou)
        #expect(row.textKey == "sync.attention.collectionCorrupt")
        #expect(row.action == .copyDebugInfo)
        // It is not the "this device changed" sentence, and not the generic failure with Try Again.
        #expect(MeguriSyncStatusPresentation.failureKey(.setupLocalChanged) != MeguriSetupSequence.localUnusableKey)
    }

    @Test func anAcceptedUploadStaysFinishableAfterItsRecoveryPreviewWasPrepared() {
        // The core refused the activation with no question and left the recovery
        // preview in place: Finish Setup must still be offered.
        #expect(MeguriSyncStatusPresentation.canRecover(Self.operation(.exactOk, .preview, boundary: .localChanged).receipt))
        #expect(MeguriSyncStatusPresentation.canRecover(
            Self.operation(.exactOk, .needsDecision, boundary: .activationInterrupted).receipt))
        // A first preview sent nothing, and an overtaken upload is never finished.
        #expect(!MeguriSyncStatusPresentation.canRecover(Self.operation(.notSent, .preview).receipt))
        #expect(!MeguriSyncStatusPresentation.canRecover(Self.operation(.overtaken, .failed, boundary: .remoteChanged).receipt))
        #expect(!MeguriSyncStatusPresentation.canDiscard(Self.operation(.overtaken, .failed, boundary: .remoteChanged).receipt))
    }

    @Test func tryAgainOnAnUnknownRoundDoesNothingWithoutThatState() {
        let sync = SyncCoordinator(directory: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString),
            configuration: nil, defaults: nil)
        sync.settleMeguriRound()
        sync.settleMeguriSetup()
        sync.startMeguriSetup()
        sync.answerMeguriSetup(.keepDevice)
        #expect(sync.meguriRoutine == nil)
        #expect(sync.meguriStage == nil)
        #expect(sync.meguriSetupDecision == nil)
        #expect(!sync.meguriBusy)
        #expect(!sync.meguriCanSettle)
    }
}
