import Foundation
import MeguriCoreBindings
import Testing
@testable import CoreBridge

@Suite struct ManualTransferMappingTests {
    @Test func activatedPartialAndUnknownRemainDistinct() {
        let partial = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .mediaIncomplete,
            boundary: .mediaProviderUnavailable, generation: 42, candidateActive: true, nextEligibleMs: nil, serverKind: nil))
        #expect(partial.candidateActive && partial.generation == 42 && partial.issue == .mediaIncomplete)
        let unknown = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .needsDecision,
            boundary: .remoteCompletionUnknown, generation: nil, candidateActive: false, nextEligibleMs: nil, serverKind: nil))
        #expect(unknown.issue == .needsDecision && unknown.boundary == .remoteCompletionUnknown)
        #expect(unknown.generation == nil && unknown.nextEligibleMilliseconds == nil)
        let cooldown = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .cooldown,
            boundary: nil, generation: nil, candidateActive: false, nextEligibleMs: 12345, serverKind: nil))
        #expect(cooldown.nextEligibleMilliseconds == 12345 && cooldown.issue == .cooldown)
    }
    @Test func terminalBeforeRegistrationAndUnrelatedJobCannotReplaceMatchedOutcome() async {
        let id = JobID(rawValue: 9_400_001)
        let (_, progress) = AsyncStream<CoreProgress>.makeStream()
        let completion = JobCompletion(progress: progress)
        JobRegistry.shared.finish(id, .cancelled)
        JobRegistry.shared.finish(JobID(rawValue: 9_400_002), .succeeded)
        JobRegistry.shared.register(id, completion)
        #expect(await completion.wait() == .cancelled)
        completion.resolve(.succeeded)
        #expect(await completion.wait() == .cancelled)
        let (_, unrelatedProgress) = AsyncStream<CoreProgress>.makeStream()
        let unrelated = JobCompletion(progress: unrelatedProgress)
        JobRegistry.shared.register(JobID(rawValue: 9_400_002), unrelated)
        #expect(await unrelated.wait() == .succeeded)
    }
}
extension EventTests {
    @Test func manualCancellationBeforeAttachmentDrainsActualJobWithFirstCause() async throws {
        let operation = ManualTransferOperation()
        await operation.cancel(.system)
        let job = try await Jobs.start("test.startJob") {
            try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
        }
        await operation.attach(job)
        await operation.cancel(.user)
        #expect(await job.outcome() == .cancelled)
        #expect(operation.cancellation == .system)
    }
}

extension ManualTransferMappingTests {
    @Test func remoteObservationsAndCopyTimesSurviveMapping() {
        let record = ManualTransferRecord(.init(id: "mapping-operation", configuredEndpoint: "https://example.test/",
            targetEndpoint: "https://example.test/", direction: .export, phase: .preview, acceptance: .notSent,
            boundary: nil, local: nil, remoteHasNoCards: true,
            remote: .init(modifiedMs: 1_900_000_001_234, schemaModifiedMs: 1_899_000_005_678, revision: 19, mediaRevision: 4),
            servedCounts: nil, preparedMs: 1_900_000_010_000, generation: nil, collectionComplete: false, mediaSelected: false,
            copies: [.init(name: "safety.colpkg", length: 10, sha256: Data(repeating: 0x11, count: 32), verifiedPackage: true, instantMs: nil, server: false),
                     .init(name: "server.raw", length: 20, sha256: Data(repeating: 0x22, count: 32), verifiedPackage: false, instantMs: 1_900_000_003_456, server: true)], nextEligibleMs: nil))
        #expect(record.remote?.modifiedMilliseconds == 1_900_000_001_234)
        #expect(record.remote?.schemaModifiedMilliseconds == 1_899_000_005_678)
        #expect(record.remote?.revision == 19 && record.remote?.mediaRevision == 4)
        #expect(record.copies[0].instantMilliseconds == nil)
        #expect(record.copies[1].instantMilliseconds == 1_900_000_003_456)
        #expect(record.copies[0].sha256 != record.copies[1].sha256)
        #expect(record.copies[0].verifiedPackage && !record.copies[1].verifiedPackage)
        #expect(record.servedCounts == nil && record.remoteHasNoCards == true)
    }
    @Test(arguments: [ManualTransferAcceptance.exactOk, .notSent, .possiblyAccepted])
    func persistedRecoveryPreviewOffersAuthoritativeInspection(acceptance: ManualTransferAcceptance) {
        let rawAcceptance: MeguriCoreBindings.TransferAcceptance
        switch acceptance {
        case .exactOk: rawAcceptance = .exactOk
        case .possiblyAccepted: rawAcceptance = .possiblyAccepted
        default: rawAcceptance = .notSent
        }
        let record = ManualTransferRecord(.init(id: "persisted-preview", configuredEndpoint: "https://example.test/",
            targetEndpoint: "https://example.test/", direction: acceptance == .exactOk ? .export : .import,
            phase: .preview, acceptance: rawAcceptance, boundary: .localChanged, local: nil, remoteHasNoCards: nil,
            remote: nil, servedCounts: nil, preparedMs: 1, generation: nil, collectionComplete: false,
            mediaSelected: false, copies: [], nextEligibleMs: nil))
        #expect(record.canInspectRecovery == (acceptance != .possiblyAccepted))
    }
}
