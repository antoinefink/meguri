@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// The order of the bridge calls of Meguri Sync setup (`sync/meguri-provider`
/// MP6.3, MP6.9), with a fake bridge that has no core behind it.
@MainActor @Suite struct MeguriSetupSequenceTests {
    /// Records each call and answers from a script. A call with no scripted
    /// answer is refused as busy.
    final class FakeBridge: MeguriSetupBridge, @unchecked Sendable {
        var calls: [String] = []
        var answers: [String: MeguriBootstrapOutcome] = [:]
        private func answer(_ call: String) -> MeguriBootstrapCommandResult {
            calls.append(call)
            return .init(outcome: answers[call] ?? .refused(.busy, accounting: nil), jobID: nil, mediaProgress: nil,
                         progressFailure: nil, retryOperation: nil)
        }
        func confirmSetup(_ decision: MeguriSetupDecision, preview: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                          progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult {
            answer("confirmSetup")
        }
        func prepareSetupDownload(_ decision: MeguriSetupDecision, preview: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                                  progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult {
            answer("prepareSetupDownload")
        }
        func confirmSetupDownload(_ decision: MeguriSetupDecision, download: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                                  progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult {
            answer("confirmSetupDownload")
        }
        func settleSetup() async throws -> MeguriBootstrapCommandResult { answer("settleSetup") }
        func prepareRecovery(_ operation: MeguriBootstrapOperation,
                             progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult {
            answer("prepareRecovery")
        }
        func confirmSetupActivation(_ recovery: MeguriBootstrapPreview,
                                    progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult {
            answer("confirmSetupActivation")
        }
        func confirm(_ preview: MeguriBootstrapPreview,
                     progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult {
            answer("confirm")
        }
        func discard(_ preview: MeguriBootstrapPreview) async throws -> MeguriBootstrapCommandResult { answer("discard") }
    }

    private static func receipt(_ direction: ManualTransferDirection, _ phase: ManualTransferPhase,
                                _ acceptance: ManualTransferAcceptance) -> MeguriBootstrapReceipt {
        .init(direction: direction, phase: phase, acceptance: acceptance)
    }
    private static func preview(_ direction: ManualTransferDirection = .export) -> MeguriBootstrapPreview {
        .init(detached: receipt(direction, .preview, .notSent))
    }
    private static func operation(_ phase: ManualTransferPhase, _ acceptance: ManualTransferAcceptance) -> MeguriBootstrapOperation {
        .init(detached: receipt(.export, phase, acceptance))
    }
    private static func decision(_ row: MeguriSetupRow) -> MeguriSetupDecision {
        .init(detached: .init(row: row, device: nil, server: nil, unsent: false), account: "synthetic-user")
    }
    private static func isRefused(_ result: MeguriBootstrapCommandResult?) -> Bool {
        if case .refused? = result?.outcome { return true }
        return false
    }

    @Test func eachRowAndAnswerMakesOnlyItsOwnCalls() async throws {
        let done = MeguriBootstrapOutcome.operation(Self.operation(.mediaIncomplete, .exactOk))
        let cases: [(MeguriSetupRow, MeguriSetupChoice, [String])] = [
            (.download, .noQuestion, ["confirmSetup"]),
            (.uploadToEmptyServer, .noQuestion, ["confirmSetup"]),
            (.ask, .keepDevice, ["confirmSetup"]),
            (.askUploadOnly, .keepDevice, ["confirmSetup"]),
            (.askDownloadOnly, .keepServer, ["confirmSetup"]),
            (.downloadOverEmptyDevice, .noQuestion, ["prepareSetupDownload", "confirmSetupDownload"]),
            (.ask, .keepServer, ["prepareSetupDownload", "confirmSetupDownload"]),
        ]
        for (row, choice, expected) in cases {
            let bridge = FakeBridge()
            bridge.answers = ["confirmSetup": done, "prepareSetupDownload": .preview(Self.preview(.import)),
                              "confirmSetupDownload": done]
            let result = try await MeguriSetupSequence.transfer(bridge, decision: Self.decision(row), preview: Self.preview(),
                                                                choice: choice, progress: { _ in })
            #expect(bridge.calls == expected, "\(row)/\(choice)")
            #expect(result != nil && !Self.isRefused(result), "\(row)/\(choice)")
        }
        // A choice that the row does not offer makes no call at all.
        let refusedChoices: [(MeguriSetupRow, MeguriSetupChoice)] = [
            (.ask, .noQuestion), (.askUploadOnly, .keepServer), (.askUploadOnly, .noQuestion),
            (.askDownloadOnly, .keepDevice), (.askDownloadOnly, .noQuestion), (.download, .keepDevice),
            (.downloadOverEmptyDevice, .keepDevice), (.uploadToEmptyServer, .keepServer),
        ]
        for (row, choice) in refusedChoices {
            let bridge = FakeBridge()
            let result = try await MeguriSetupSequence.transfer(bridge, decision: Self.decision(row), preview: Self.preview(),
                                                                choice: choice, progress: { _ in })
            #expect(result == nil, "\(row)/\(choice)")
            #expect(bridge.calls.isEmpty, "\(row)/\(choice)")
        }
    }

    @Test func aDownloadThatWasNotPreparedIsNeverConfirmed() async throws {
        for failure: MeguriBootstrapFailure in [.setupLocalChanged, .setupRemoteChanged, .setupChoice, .busy] {
            let bridge = FakeBridge()
            bridge.answers = ["prepareSetupDownload": .refused(failure, accounting: nil)]
            let result = try await MeguriSetupSequence.transfer(bridge, decision: Self.decision(.ask), preview: Self.preview(),
                                                                choice: .keepServer, progress: { _ in })
            #expect(bridge.calls == ["prepareSetupDownload"])
            #expect(Self.isRefused(result))
        }
    }

    // S1: after "accepted", this device's collection is replaced with no
    // question only through the core's own test that it is still what was sent.
    @Test func aCancelBetweenPreparationAndConfirmationDiscardsTheDownload() async throws {
        let bridge = FakeBridge()
        bridge.answers = ["prepareSetupDownload": .preview(Self.preview(.import))]
        await #expect(throws: CancellationError.self) {
            _ = try await MeguriSetupSequence.transfer(bridge, decision: Self.decision(.ask), preview: Self.preview(),
                                                       choice: .keepServer, progress: { _ in }, proceed: { false })
        }
        #expect(bridge.calls == ["prepareSetupDownload", "discard"])
    }

    @Test func aSettledAcceptanceFinishesOnlyThroughTheCoresUnchangedTest() async throws {
        let bridge = FakeBridge()
        let accepted = Self.operation(.needsDecision, .exactOk)
        bridge.answers = ["settleSetup": .settled(.accepted(accepted, localUnchanged: true)),
                          "prepareRecovery": .preview(Self.preview()),
                          "confirmSetupActivation": .operation(Self.operation(.mediaIncomplete, .exactOk)),
                          "confirm": .operation(Self.operation(.mediaIncomplete, .exactOk))]
        let settled = MeguriSetupSequence.settled(try await bridge.settleSetup().outcome)
        guard case .accepted(let operation, hintUnchanged: true) = settled else { Issue.record("accepted"); return }
        let finished = try await MeguriSetupSequence.finish(bridge, operation: operation, asked: false, progress: { _ in },
                                                           current: { true })
        #expect(bridge.calls == ["settleSetup", "prepareRecovery", "confirmSetupActivation"])
        #expect(finished.confirmation != nil && !Self.isRefused(finished.confirmation))
    }

    @Test func aDeviceThatChangedIsRefusedAndOnlyTheUsersFinishSetupConfirms() async throws {
        let bridge = FakeBridge()
        let accepted = Self.operation(.needsDecision, .exactOk)
        bridge.answers = ["prepareRecovery": .preview(Self.preview()),
                          "confirmSetupActivation": .refused(.setupLocalChanged, accounting: nil),
                          "confirm": .operation(Self.operation(.mediaIncomplete, .exactOk))]
        // The continuation of a settled acceptance: refused, and the plain confirmation is not tried.
        let automatic = try await MeguriSetupSequence.finish(bridge, operation: accepted, asked: false, progress: { _ in },
                                                            current: { true })
        #expect(bridge.calls == ["prepareRecovery", "confirmSetupActivation"])
        #expect(Self.isRefused(automatic.confirmation))
        // The refusal is not a reason to read both sides again: setup stays at Finish Setup.
        #expect(MeguriSetupSequence.reloadNotice(after: automatic.confirmation?.outcome, reloads: 0, choiceAllowed: false) == nil)
        // Finish Setup, which the user chose: the retained confirmation.
        bridge.calls = []
        let asked = try await MeguriSetupSequence.finish(bridge, operation: accepted, asked: true, progress: { _ in },
                                                        current: { true })
        #expect(bridge.calls == ["prepareRecovery", "confirm"])
        #expect(asked.confirmation != nil && !Self.isRefused(asked.confirmation))
    }

    @Test func aRecoveryThatWasNotPreparedOrASupersededTaskConfirmsNothing() async throws {
        for asked in [false, true] {
            let bridge = FakeBridge()
            bridge.answers = ["prepareRecovery": .refused(.busy, accounting: nil)]
            let finished = try await MeguriSetupSequence.finish(bridge, operation: Self.operation(.needsDecision, .exactOk),
                                                               asked: asked, progress: { _ in }, current: { true })
            #expect(bridge.calls == ["prepareRecovery"])
            #expect(finished.confirmation == nil && Self.isRefused(finished.preparation))

            let superseded = FakeBridge()
            superseded.answers = ["prepareRecovery": .preview(Self.preview())]
            await #expect(throws: CancellationError.self) {
                _ = try await MeguriSetupSequence.finish(superseded, operation: Self.operation(.needsDecision, .exactOk),
                                                         asked: asked, progress: { _ in }, current: { false })
            }
            #expect(superseded.calls == ["prepareRecovery"])
        }
    }

    // S2: neither is settled; the next step is setup's own preparation, never a transfer.
    @Test func neitherMakesNoFurtherCallAndLeadsToTheSheet() async throws {
        let bridge = FakeBridge()
        bridge.answers = ["settleSetup": .settled(.neither(Self.operation(.failed, .overtaken)))]
        let result = try await bridge.settleSetup()
        guard case .overtaken = MeguriSetupSequence.settled(result.outcome) else { Issue.record("overtaken"); return }
        #expect(bridge.calls == ["settleSetup"])
        // The state it leaves is the "choose" row, and its sheet's Cancel leaves the service Off.
        let row = SyncKinds.meguriRow(.init(connection: .setupRequired, stage: .failed,
                                            failureKey: MeguriSetupSequence.overtakenKey))
        #expect(row.action == .chooseCollection)
        #expect(!MeguriSetupSequence.keepsService(requested: true, stage: .failed, failureKey: MeguriSetupSequence.overtakenKey))
        // The core then gives the sheet: both collections, each answer one direction.
        #expect(SyncCoordinator.meguriSetupStep(.ask) == .ask)
        #expect(SyncCoordinator.meguriSetupChoices(.ask) == [.keepDevice, .keepServer])
    }

    @Test func changedFactsReloadBothSidesOnceAndOnlyWhileAChoiceIsAllowed() {
        let local = MeguriBootstrapOutcome.refused(.setupLocalChanged, accounting: nil)
        let remote = MeguriBootstrapOutcome.refused(.setupRemoteChanged, accounting: nil)
        #expect(MeguriSetupSequence.reloadNotice(after: local, reloads: 1, choiceAllowed: true)
            == "sync.meguri.setup.error.localChanged")
        #expect(MeguriSetupSequence.reloadNotice(after: remote, reloads: 1, choiceAllowed: true)
            == "sync.meguri.setup.error.remoteChanged")
        // One reload only, and never while an operation still holds the choice.
        #expect(MeguriSetupSequence.reloadNotice(after: local, reloads: 0, choiceAllowed: true) == nil)
        #expect(MeguriSetupSequence.reloadNotice(after: local, reloads: 1, choiceAllowed: false) == nil)
        #expect(MeguriSetupSequence.reloadNotice(after: nil, reloads: 1, choiceAllowed: true) == nil)
        for failure: MeguriBootstrapFailure in [.setupChoice, .setupLocalUnusable, .busy, .storage] {
            #expect(MeguriSetupSequence.reloadNotice(after: .refused(failure, accounting: nil), reloads: 1,
                                                     choiceAllowed: true) == nil)
        }
    }

    @Test func cancelDiscardsThePreviewAndReportsARefusedDiscard() async throws {
        let bridge = FakeBridge()
        bridge.answers = ["discard": .operation(Self.operation(.cancelled, .notSent))]
        let cancelled = try await MeguriSetupSequence.cancel(bridge, preview: Self.preview())
        #expect(bridge.calls == ["discard"])
        #expect(cancelled.discarded)
        let refusing = FakeBridge()
        let kept = try await MeguriSetupSequence.cancel(refusing, preview: Self.preview())
        #expect(refusing.calls == ["discard"])
        #expect(!kept.discarded && Self.isRefused(kept.result))
    }

    @Test func aDetachedHandleHasNoAuthorityOnTheRealBridge() {
        // The handles above carry a receipt only; the opaque core handle is absent.
        #expect(Self.preview().receipt.acceptance == .notSent)
        #expect(Self.decision(.ask).account == "synthetic-user")
    }
}
