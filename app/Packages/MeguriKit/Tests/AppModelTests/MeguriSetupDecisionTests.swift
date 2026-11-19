@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// `sync/meguri-provider` MP6.3 and MP6.9: what the coordinator does with the
/// core's row, with an answer, with changed facts and with a settled acceptance.
@MainActor @Suite struct MeguriSetupDecisionTests {
    private let rows: [MeguriSetupRow] = [.download, .downloadOverEmptyDevice, .uploadToEmptyServer, .ask, .askUploadOnly]
    private let allChoices: [MeguriSetupChoice] = [.noQuestion, .keepDevice, .keepServer]

    @Test func onlyRowsOneToThreeRunWithoutTheSheet() {
        #expect(SyncCoordinator.meguriSetupStep(.download) == .run(.noQuestion))
        #expect(SyncCoordinator.meguriSetupStep(.downloadOverEmptyDevice) == .run(.noQuestion))
        #expect(SyncCoordinator.meguriSetupStep(.uploadToEmptyServer) == .run(.noQuestion))
        #expect(SyncCoordinator.meguriSetupStep(.ask) == .ask)
        #expect(SyncCoordinator.meguriSetupStep(.askUploadOnly) == .ask)
    }

    @Test func theSheetOffersBothCollectionsOrTheUploadAlone() {
        #expect(SyncCoordinator.meguriSetupChoices(.ask) == [.keepDevice, .keepServer])
        #expect(SyncCoordinator.meguriSetupChoices(.askUploadOnly) == [.keepDevice])
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
    }

    @Test func changedFactsReloadBothSidesAndOtherRefusalsDoNot() {
        #expect(SyncCoordinator.meguriSetupChangeKey(.refused(.setupLocalChanged, accounting: nil))
            == "sync.meguri.setup.error.localChanged")
        #expect(SyncCoordinator.meguriSetupChangeKey(.refused(.setupRemoteChanged, accounting: nil))
            == "sync.meguri.setup.error.remoteChanged")
        for failure: MeguriBootstrapFailure in [.setupChoice, .busy, .storage, .cancelled, .connectionChanged] {
            #expect(SyncCoordinator.meguriSetupChangeKey(.refused(failure, accounting: nil)) == nil)
        }
        #expect(SyncCoordinator.meguriSetupChangeKey(.settled(.neither)) == nil)
        #expect(MeguriSyncStatusPresentation.failureKey(.setupLocalChanged) == "sync.meguri.setup.error.localChanged")
        #expect(MeguriSyncStatusPresentation.failureKey(.setupRemoteChanged) == "sync.meguri.setup.error.remoteChanged")
    }

    @Test func neitherAndUnreadableLeaveTheAcceptanceUnknown() {
        guard case .unknown = SyncCoordinator.meguriSettled(.settled(.neither)) else {
            Issue.record("neither must stay unknown"); return
        }
        guard case .unknown = SyncCoordinator.meguriSettled(.refused(.unavailable, accounting: nil)) else {
            Issue.record("an unreadable server must stay unknown"); return
        }
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
