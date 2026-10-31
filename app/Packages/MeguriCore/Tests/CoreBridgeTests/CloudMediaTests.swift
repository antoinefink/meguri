import Foundation
import Instrumentation
import Testing
@testable import CoreBridge

struct CloudMediaTests {
    @Test @MainActor func everyMediaSelectorUsesMaintenanceLane() async throws {
        for selector in CoreCloudMediaService.selectors {
            let ran = try await CoreCloudMediaService.call(selector, probe)
            #expect(ran.queueLabel == Lane.maintenance.queueLabel && !ran.isMainThread)
        }
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
