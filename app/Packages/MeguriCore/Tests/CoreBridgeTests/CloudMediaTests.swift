import Foundation
import Instrumentation
import Synchronization
import Testing
@testable import CoreBridge

struct CloudMediaTests {
    @Test @MainActor func everyMediaSelectorUsesMaintenanceLane() async throws {
        #expect(CoreCloudMediaService.selectors.contains(.cloudMediaStartNextWork))
        #expect(CoreCloudMediaService.selectors.contains(.cloudMediaNextWorkResult))
        #expect(!CoreCloudMediaService.selectors.contains(.cloudMediaNextWork))
        for selector in CoreCloudMediaService.selectors {
            let ran = try await CoreCloudMediaService.call(selector, probe)
            #expect(ran.queueLabel == Lane.maintenance.queueLabel && !ran.isMainThread)
        }
    }

    @Test func sharedStagingCachesSuccessButPropagatesAndDoesNotCacheInitializationFailure() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let blocked = root.appendingPathComponent("blocked")
        try Data([1]).write(to: blocked)
        let cache = Mutex<CloudMediaStaging?>(nil)
        var attempts = 0
        #expect(throws: (any Error).self) {
            try CloudMediaStaging.initializedDefault(in: cache) {
                attempts += 1
                return try CloudMediaStaging(directory: blocked.appendingPathComponent("runs"))
            }
        }
        #expect(attempts == 1)
        #expect(cache.withLock { $0 == nil })
        try FileManager.default.removeItem(at: blocked)
        let first = try CloudMediaStaging.initializedDefault(in: cache) {
            attempts += 1
            return try CloudMediaStaging(directory: blocked.appendingPathComponent("runs"))
        }
        let second = try CloudMediaStaging.initializedDefault(in: cache) {
            attempts += 1
            throw CloudMediaFailure.noSpace
        }
        #expect(first === second)
        #expect(attempts == 2)
        let shared = try CloudMediaStaging.sharedDefault()
        #expect(shared === (try CloudMediaStaging.sharedDefault()))
    }

    @Test func publicStagingAllocatorsShareByteAndCountCapsAcrossDistinctDirectories() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("sdk-source")
        try Data([7]).write(to: source)
        let writer = try FileHandle(forWritingTo: source)
        try writer.truncate(atOffset: CloudMediaStaging.assetLimit)
        try writer.close()
        let tiny = root.appendingPathComponent("sdk-tiny")
        try Data([9]).write(to: tiny)
        let appRoot = root.appendingPathComponent("app-root/cloud-native-staging", isDirectory: true)
        let defaultRoot = root.appendingPathComponent("default/MeguriCloudPayloads", isDirectory: true)
        let app = try CloudMediaStaging(directory: appRoot)
        let engine = try CloudMediaStaging(directory: defaultRoot)
        #expect(app !== engine)
        var adoption: CloudMediaStagedAsset? = try app.copyPayload(source, cancelled: { false })
        var event: CloudMediaStagedAsset? = try engine.copy(source, cancelled: { false }, retainedAssetLimit: 256)
        let adoptionURL = try #require(adoption?.url)
        let eventURL = try #require(event?.url)
        #expect(adoptionURL.deletingLastPathComponent().deletingLastPathComponent() == appRoot)
        #expect(eventURL.deletingLastPathComponent().deletingLastPathComponent() == defaultRoot)
        do {
            let unexpected = try engine.copy(source, cancelled: { false }, retainedAssetLimit: 256)
            withExtendedLifetime(unexpected) {}
            Issue.record("aggregate byte limit was not enforced")
        } catch CloudMediaFailure.stagingLimit {}
        do {
            let unexpected = try app.copyPayload(tiny, cancelled: { false })
            withExtendedLifetime(unexpected) {}
            Issue.record("aggregate count limit was not enforced")
        } catch CloudMediaFailure.stagingLimit {}
        adoption = nil
        #expect(!FileManager.default.fileExists(atPath: adoptionURL.path))
        #expect(FileManager.default.fileExists(atPath: eventURL.path))
        var retry: CloudMediaStagedAsset? = try engine.copy(source, cancelled: { false }, retainedAssetLimit: 256)
        let retryURL = try #require(retry?.url)
        event = nil
        retry = nil
        #expect(!FileManager.default.fileExists(atPath: eventURL.path))
        #expect(!FileManager.default.fileExists(atPath: retryURL.path))
        var afterRelease: CloudMediaStagedAsset? = try app.copyPayload(tiny, cancelled: { false })
        let finalURL = try #require(afterRelease?.url)
        afterRelease = nil
        #expect(!FileManager.default.fileExists(atPath: finalURL.path))
        #expect(try FileManager.default.contentsOfDirectory(atPath: appRoot.path).isEmpty)
        #expect(try FileManager.default.contentsOfDirectory(atPath: defaultRoot.path).isEmpty)
        #expect(FileManager.default.fileExists(atPath: source.path))
        #expect(FileManager.default.fileExists(atPath: tiny.path))
    }

    @Test func stagingOwnsNonemptyBytesAndRemovesOnlyItsRun() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("sdk-temporary")
        let bytes = Data((0..<131_073).map { UInt8($0 % 251) })
        try bytes.write(to: source)
        let staging = try CloudMediaStaging(directory: root.appendingPathComponent("runs"), available: { UInt64.max })
        var asset: CloudMediaStagedAsset? = try staging.copy(source, cancelled: { false })
        let path = try #require(asset?.url)
        try FileManager.default.removeItem(at: source)
        #expect(try Data(contentsOf: path) == bytes)
        asset = nil
        #expect(!FileManager.default.fileExists(atPath: path.path))
        #expect(FileManager.default.fileExists(atPath: root.path))
    }

    @Test func failedStagingRemovesPartialFilesAndEnforcesSpaceSizeAndCancellation() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("source")
        try Data([1, 2, 3]).write(to: source)
        let stagingRoot = root.appendingPathComponent("runs")
        let full = try CloudMediaStaging(directory: stagingRoot, available: { 512 * 1_024 * 1_024 })
        #expect(throws: CloudMediaFailure.self) { try full.copy(source, cancelled: { false }) }
        let staging = try CloudMediaStaging(directory: stagingRoot, available: { UInt64.max })
        #expect(throws: CancellationError.self) { try staging.copy(source, cancelled: { true }) }
        let handle = try FileHandle(forWritingTo: source)
        try handle.truncate(atOffset: 100 * 1_024 * 1_024 + 1)
        try handle.close()
        #expect(throws: CloudMediaFailure.self) { try staging.copy(source, cancelled: { false }) }
        #expect(try FileManager.default.contentsOfDirectory(atPath: stagingRoot.path).isEmpty)
    }

    enum InvalidSource: CaseIterable, Sendable { case empty, directory, symlink, oversized, growing, shortRead }

    @Test(arguments: InvalidSource.allCases)
    func copyKeepsMediaValidationErrorsWhilePayloadCopyTypesSourceFailures(_ fault: InvalidSource) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let stagingRoot = root.appendingPathComponent("runs")
        let validSource = root.appendingPathComponent("valid")
        let staging = try CloudMediaStaging(directory: stagingRoot, available: { UInt64.max }, beforeSourceRead: { source, boundary in
            guard source != validSource, case .open = boundary else { return }
            if fault == .shortRead || fault == .growing {
                let writer = try FileHandle(forWritingTo: source)
                try writer.truncate(atOffset: fault == .shortRead ? 1 : 4)
                try writer.close()
            }
        })
        for payload in [false, true] {
            let source = root.appendingPathComponent(payload ? "payload" : "media")
            switch fault {
            case .empty: try Data().write(to: source)
            case .directory: try FileManager.default.createDirectory(at: source, withIntermediateDirectories: false)
            case .symlink:
                let target = root.appendingPathComponent(payload ? "payload-target" : "media-target")
                try Data([1]).write(to: target)
                try FileManager.default.createSymbolicLink(at: source, withDestinationURL: target)
            case .oversized:
                try Data([1]).write(to: source)
                let writer = try FileHandle(forWritingTo: source)
                try writer.truncate(atOffset: CloudMediaStaging.assetLimit + 1)
                try writer.close()
            case .growing, .shortRead: try Data([1, 2, 3]).write(to: source)
            }
            do {
                let unexpected: CloudMediaStagedAsset
                if payload { unexpected = try staging.copyPayload(source, cancelled: { false }) }
                else { unexpected = try staging.copy(source, cancelled: { false }) }
                withExtendedLifetime(unexpected) {}
                Issue.record("invalid source was staged")
            } catch {
                if payload {
                    let failure = try #require(error as? CloudMediaSourceFailure)
                    switch (fault, failure) {
                    case (.empty, .invalidShape), (.directory, .invalidShape), (.symlink, .invalidShape),
                         (.oversized, .invalidSize), (.growing, .invalidSize), (.shortRead, .shortRead): break
                    default: Issue.record("payload source classification changed")
                    }
                } else {
                    let failure = try #require(error as? CloudMediaFailure)
                    switch (fault, failure) {
                    case (.empty, .invalidAsset), (.directory, .invalidAsset), (.symlink, .invalidAsset),
                         (.shortRead, .invalidAsset), (.oversized, .assetTooLarge), (.growing, .assetTooLarge): break
                    default: Issue.record("media validation compatibility changed")
                    }
                }
            }
            #expect(try FileManager.default.contentsOfDirectory(atPath: stagingRoot.path).isEmpty)
        }
        // Failed copies must return both count reservations to existing callers.
        try Data([1]).write(to: validSource)
        let retries = try (0..<2).map { _ in try staging.copy(validSource, cancelled: { false }) }
        withExtendedLifetime(retries) {}
    }

    @Test(arguments: [false, true])
    func copyPreservesRawSourceIOErrorsWhilePayloadCopyWrapsThem(duringRead: Bool) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("source")
        try Data([1, 2, 3]).write(to: source)
        let stagingRoot = root.appendingPathComponent("runs")
        let original = NSError(domain: NSCocoaErrorDomain, code: NSFileReadNoPermissionError)
        let staging = try CloudMediaStaging(directory: stagingRoot, available: { UInt64.max }, beforeSourceRead: { _, boundary in
            switch boundary {
            case .open where !duringRead: throw original
            case .read where duringRead: throw original
            default: break
            }
        })
        do {
            _ = try staging.copy(source, cancelled: { false })
            Issue.record("source I/O failure was lost")
        } catch { #expect((error as NSError) === original) }
        do {
            _ = try staging.copyPayload(source, cancelled: { false })
            Issue.record("payload source I/O failure was lost")
        } catch {
            guard case CloudMediaSourceFailure.unavailable(let underlying) = error else {
                Issue.record("payload source I/O failure was not typed"); return
            }
            #expect((underlying as NSError) === original)
        }
        #expect(try FileManager.default.contentsOfDirectory(atPath: stagingRoot.path).isEmpty)
    }
}

extension CloudMediaTests {
    @Test func finalBatchAdmissionDeliversPreparedValueOnlyFromRetainedCallback() async throws {
        let admission = CloudMediaBatchAdmission { queue in queue.enqueue() }
        #expect(try await admission.deliver(73) == 73)
        let denied = CloudMediaBatchAdmission { _ in false }
        #expect(try await denied.deliver(73) == nil)
        let stale = CloudMediaBatchAdmission { _ in throw CloudMediaFailure.closed }
        await #expect(throws: CloudMediaFailure.self) { try await stale.deliver(73) }
    }
}


extension CloudMediaTests {
    @Test func acceptedDeleteAbsenceIsDiagnosticAndDoesNotStopTheMediaRound() throws {
        let missing = CloudRecordFailure(name: "blob", code: .unknownItem, zone: "meguri-media", nativeCode: 11)
        let receipt = CloudMediaSendReceipt(failed: [], failedDeletes: [missing])
        #expect(receipt.blocking == nil)
        #expect(receipt.diagnostic?.code == 11)
        // The same result for a SAVE is an error, never deletion evidence.
        #expect(CloudMediaSendReceipt(failed: [missing], failedDeletes: []).blocking?.code == 11)
        let denied = CloudRecordFailure(name: "other", code: .other, zone: "meguri-media", nativeCode: 12)
        for saveFailure in [false, true] {
            let mixed = CloudMediaSendReceipt(failed: saveFailure ? [denied] : [],
                failedDeletes: saveFailure ? [missing] : [missing, denied])
            #expect(mixed.blocking?.code == 12)
            #expect(mixed.diagnostic?.code == 11 || mixed.diagnostic?.code == 12)
        }
    }

    @Test func deletionOnlyThrottleAndMixedReceiptsRetainOriginalMaximumDelay() throws {
        let missing = CloudRecordFailure(name: "absent", code: .unknownItem, zone: "meguri-media", nativeCode: 11)
        let deletion = CloudRecordFailure(name: "delete", code: .requestRateLimited, zone: "meguri-media",
            nativeCode: 7, retryAfterMilliseconds: 120_000)
        let save = CloudRecordFailure(name: "save", code: .serviceUnavailable, zone: "meguri-media",
            nativeCode: 6, retryAfterMilliseconds: 30_000)
        let only = try #require(CloudMediaSendReceipt(failed: [], failedDeletes: [missing, deletion]).blocking)
        #expect(only.code == 7 && only.retryAfterMilliseconds == 120_000)
        let mixed = try #require(CloudMediaSendReceipt(failed: [save], failedDeletes: [missing, deletion]).blocking)
        #expect(mixed.code == 6 && mixed.retryAfterMilliseconds == 120_000)
        // Metadata inputs retain their original per-record attribution.
        #expect(deletion.name == "delete" && deletion.zone == "meguri-media" && deletion.nativeCode == 7)
        #expect(save.retryAfterMilliseconds == 30_000)
    }
}

extension EventTests {
    @Test func originalFakeSDKCannotTurnAnInvalidDeleteAbsenceIntoAcceptedBookkeeping() async throws {
        try await withCloudKitPendingFixture { f in
            let engine = try await f.engine()
            let batch = try #require(await engine.requestRetainedBatch(zone: f.zone))
            let before = try await f.core.durableSnapshot()
            let name = try #require(batch.saves.first?.name)
            // This name was handed as a save. Only original Core validation
            // can decide whether a delete receipt belongs to a retained Work.
            await engine.emitRetained(.sent(batchID: batch.id, saved: [], failed: [], deleted: [],
                failedDeletes: [.init(name: name, code: .unknownItem, zone: "meguri-media", nativeCode: 11)]),
                label: "invalidDeleteAbsence")
            #expect(await f.adapter.lastFailure != nil)
            let after = try await f.core.durableSnapshot()
            #expect(after.entities == before.entities)
        }
    }

    @Test func deletionReceiptDelayReachesActualPolicyErrorFacts() async throws {
        try await withNativePolicyFixture { f in
            let (policy, _) = try await nativePolicy(f)
            let engine = try await f.engine()
            let failure = try #require(CloudMediaSendReceipt(failed: [], failedDeletes: [
                .init(name: "delete", code: .requestRateLimited, zone: "meguri-media", nativeCode: 7,
                      retryAfterMilliseconds: 120_000)
            ]).blocking)
            _ = try await policy.input(.operationError(instance: engine.instanceID,
                operation: .init(zone: .media, kind: .send), error: .throttled,
                code: failure.code, retryAfterMilliseconds: failure.retryAfterMilliseconds))
            let error = try #require(try await policy.status().error)
            #expect(error.code == 7 && error.retryAfterMilliseconds == 120_000)
            #expect(error.operation == .init(zone: .media, kind: .send))
        }
    }
}


extension CloudMediaTests {
    @Test func failedRecordMappingPreservesOriginalNativeMetadata() {
        let failure = CloudRecordFailure(name: "original-delete", code: .requestRateLimited,
            zone: "meguri-media", nativeCode: 7, retryAfterMilliseconds: 120_000)
        let raw = failure.raw
        #expect(raw.name == failure.name && raw.zone == failure.zone)
        #expect(raw.nativeCode == 7 && raw.retryAfterMs == 120_000)
    }
}
