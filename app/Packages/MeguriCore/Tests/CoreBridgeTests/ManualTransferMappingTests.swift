import Foundation
import MeguriCoreBindings
import Testing
@testable import CoreBridge

@Suite struct ManualTransferMappingTests {
    @Test func redirectReasonsAreAllowlistedAndDestinationsAreSafe() {
        let reasons: [(String, ManualTransferRedirectReason)] = [
            ("not_meta", .notMeta), ("not_308", .notPermanent), ("missing_location", .missingLocation),
            ("empty", .empty), ("unparseable", .unparseable), ("unsupported_scheme", .unsupportedScheme),
            ("missing_host", .missingHost), ("credentials", .credentials), ("query_or_fragment", .queryOrFragment),
            ("https_lost", .lostHttps), ("host", .host), ("repeated", .repeated), ("not_resendable", .notResendable)
        ]
        for (id, reason) in reasons {
            let failure = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .server,
                boundary: nil, generation: nil, candidateActive: false, nextEligibleMs: nil, serverKind: "redirect_refused",
                redirect: .init(destination: "https://USER:SECRET@example.test/prefix/?token=SECRET#SECRET", reason: id)))
            #expect(failure.redirect?.reason == reason)
            #expect(failure.redirect?.destination == "https://example.test/prefix/")
        }
        for target in [nil, "broken SECRET", "/relative?SECRET", "//example.test/SECRET", "ftp://example.test/SECRET", "file:///SECRET", "https://"] as [String?] {
            let failure = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .server,
                boundary: nil, generation: nil, candidateActive: false, nextEligibleMs: nil, serverKind: "redirect_refused",
                redirect: .init(destination: target, reason: "host")))
            #expect(failure.redirect?.reason == .host)
            #expect(failure.redirect?.destination == nil)
        }
        for reason in ["HOST", " host", "host\n", "unknown_SECRET", "server_message: SECRET"] {
            let failure = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .server,
                boundary: nil, generation: nil, candidateActive: false, nextEligibleMs: nil, serverKind: "redirect_refused",
                redirect: .init(destination: "https://example.test/", reason: reason)))
            #expect(failure.redirect == nil)
        }
        for kind in [nil, "server_message", "unknown_SECRET", "redirect_refused: SECRET"] as [String?] {
            let failure = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .server,
                boundary: nil, generation: nil, candidateActive: false, nextEligibleMs: nil, serverKind: kind,
                redirect: .init(destination: "https://example.test/", reason: "host")))
            #expect(failure.redirect == nil)
        }
    }

    @Test func activatedPartialAndUnknownRemainDistinct() {
        let partial = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .mediaIncomplete,
            boundary: .mediaProviderUnavailable, generation: 42, candidateActive: true, nextEligibleMs: nil, serverKind: nil, redirect: nil))
        #expect(partial.candidateActive && partial.generation == 42 && partial.issue == .mediaIncomplete)
        let unknown = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .needsDecision,
            boundary: .remoteCompletionUnknown, generation: nil, candidateActive: false, nextEligibleMs: nil, serverKind: nil, redirect: nil))
        #expect(unknown.issue == .needsDecision && unknown.boundary == .remoteCompletionUnknown)
        #expect(unknown.generation == nil && unknown.nextEligibleMilliseconds == nil)
        let cooldown = CoreBridge.ManualTransferFailure(MeguriCoreBindings.TransferFailure(issue: .cooldown,
            boundary: nil, generation: nil, candidateActive: false, nextEligibleMs: 12345, serverKind: nil, redirect: nil))
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
            servedCounts: nil, preparedMs: 1_900_000_010_000, generation: nil, collectionComplete: false, mediaSelected: false, revision: 0, media: nil,
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
            mediaSelected: false, revision: 0, media: nil, copies: [], nextEligibleMs: nil))
        #expect(record.canInspectRecovery == (acceptance != .possiblyAccepted))
    }
}

extension ManualTransferMappingTests {
    @Test func typedMediaSnapshotKeepsExclusionsOutsideTransferredCounts() {
        let snapshot = ManualMediaSnapshot(.init(phase: .downloading,
            counters: .init(uploadedFiles: 2, uploadedDeletions: 1, downloadedFiles: 7, downloadedDeletions: 3,
                checked: 18, bytesDownloaded: 900, bytesUploaded: 200, resynced: true,
                oversized: ["large.bin"], empty: ["empty.bin"], awaiting: 4),
            listed: 10, downloadFiles: 11, downloadReceived: 7, publishedFiles: 7,
            filesLeft: 4, estimateSeconds: 1.5))
        #expect(snapshot.phase == .downloading && snapshot.estimateSeconds == 1.5)
        #expect(snapshot.counters.downloadedFiles == 7 && snapshot.counters.uploadedFiles == 2)
        #expect(snapshot.counters.bytesDownloaded == 900 && snapshot.counters.bytesUploaded == 200)
        #expect(snapshot.counters.oversized == ["large.bin"] && snapshot.counters.empty == ["empty.bin"])
        #expect(snapshot.filesLeft == 4 && snapshot.counters.awaiting == 4)
    }
}

extension ManualTransferMappingTests {
    @Test func completionKeepsHistoricalEffectUncertaintyAndExclusionKnowledgeSeparate() {
        for verified in [false, true] {
            let outcome = ManualMediaOutcome(.init(stage: .complete,
                counters: .init(uploadedFiles: 0, uploadedDeletions: 0, downloadedFiles: 0, downloadedDeletions: 0,
                    checked: 9, bytesDownloaded: 0, bytesUploaded: 7, resynced: false,
                    oversized: [], empty: [], awaiting: 0),
                remoteOutcomeUnknown: true, exclusionsVerified: verified, failureKind: nil))
            #expect(outcome.stage == .complete && outcome.remoteOutcomeUnknown)
            #expect(outcome.exclusionsVerified == verified)
            #expect(outcome.counters.uploadedFiles == 0 && outcome.counters.bytesUploaded == 7)
        }
    }
}


extension ManualTransferMappingTests {
    @Test func supersededPartialMapsWithoutResumeOrWholeSuccess() {
        let record = ManualTransferRecord(.init(id: "superseded-partial", configuredEndpoint: "https://example.test/",
            targetEndpoint: "https://example.test/", direction: .export, phase: .superseded, acceptance: .exactOk,
            boundary: .interrupted, local: nil, remoteHasNoCards: nil, remote: nil, servedCounts: nil,
            preparedMs: 1, generation: 2, collectionComplete: true, mediaSelected: true, revision: 7,
            media: .init(stage: .interrupted,
                counters: .init(uploadedFiles: 2, uploadedDeletions: 1, downloadedFiles: 3, downloadedDeletions: 1,
                    checked: 9, bytesDownloaded: 100, bytesUploaded: 50, resynced: false, oversized: [], empty: [], awaiting: 4),
                remoteOutcomeUnknown: true, exclusionsVerified: false, failureKind: "interrupted"),
            copies: [], nextEligibleMs: nil))
        #expect(record.phase == .superseded && record.collectionComplete && record.mediaSelected)
        #expect(!record.canResumeMedia && !record.canInspectRecovery)
        #expect(record.media?.stage == .interrupted && record.media?.remoteOutcomeUnknown == true)
        #expect(record.media?.counters.uploadedFiles == 2 && record.nextEligibleMilliseconds == nil)
    }
}
