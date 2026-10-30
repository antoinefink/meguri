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
