#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

struct NativeCloudPayloadAssetTests {
    enum SourceFault: CaseIterable, Sendable { case metadata, open, read, shortRead, empty, directory, oversized }

    @Test(arguments: SourceFault.allCases)
    func sourceFailuresKeepEnvelopesEarlierLeasesAndFollowingRecords(_ fault: SourceFault) throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        let sources = (0..<3).map { directory.appendingPathComponent("sdk-\($0)") }
        let bytes = (0..<3).map { Data(repeating: UInt8($0 + 1), count: 131_073) }
        var native: [CKRecord] = []
        for index in sources.indices {
            try bytes[index].write(to: sources[index])
            let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-\(index)", zoneID: zone))
            record.encryptedValues["e"] = Data([UInt8(index + 1)]) as CKRecordValue
            record["pa"] = CKAsset(fileURL: sources[index])
            native.append(record)
        }
        let inline = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "following-inline", zoneID: zone))
        inline.encryptedValues["e"] = Data([4]) as CKRecordValue
        native.append(inline)
        let reads = Mutex(0)
        let injected = Mutex(false)
        let stagingDirectory = directory.appendingPathComponent("native")
        let staging = try CloudMediaStaging(directory: stagingDirectory, available: { UInt64.max }, beforeSourceRead: { source, boundary in
            guard source == sources[1] else { return }
            switch (fault, boundary) {
            case (.open, .open):
                // Shape validation has already succeeded; the real open fails.
                try FileManager.default.removeItem(at: source)
                injected.withLock { $0 = true }
            case (.read, .read(let input)):
                if reads.withLock({ $0 += 1; return $0 == 2 }) {
                    // One real chunk was written before the real read fails.
                    try input.close()
                    injected.withLock { $0 = true }
                }
            case (.shortRead, .open):
                let writer = try FileHandle(forWritingTo: source)
                try writer.truncate(atOffset: 1)
                try writer.close()
                injected.withLock { $0 = true }
            default: break
            }
        })
        switch fault {
        case .metadata: try FileManager.default.removeItem(at: sources[1])
        case .empty: try Data().write(to: sources[1])
        case .directory:
            try FileManager.default.removeItem(at: sources[1])
            try FileManager.default.createDirectory(at: sources[1], withIntermediateDirectories: false)
        case .oversized:
            let writer = try FileHandle(forWritingTo: sources[1])
            try writer.truncate(atOffset: CloudMediaStaging.assetLimit + 1)
            try writer.close()
        default: break
        }
        let deletion = CKRecord.ID(recordName: "deleted", zoneID: zone)
        var delivery: NativeCloudFetchedPayloads? = try .init(records: native, deleted: [deletion], staging: staging)
        #expect([SourceFault.metadata, .empty, .directory, .oversized].contains(fault) || injected.withLock { $0 })
        #expect(delivery?.events.count == 1)
        let event = try #require(delivery?.events.first)
        guard case .fetched(let deliveredZone, let records, let deleted) = event else { Issue.record("fetched event missing"); return }
        try #require(records.count == native.count)
        #expect(deliveredZone == zone.zoneName && deleted == [deletion.recordName])
        for (record, original) in zip(records, native) {
            #expect(record.name == original.recordID.recordName)
            #expect(record.zone == zone.zoneName && record.recordType == "MGEntity")
            #expect(record.envelope == (original.encryptedValues["e"] as? Data))
            let fields = try #require(record.systemFields)
            let restored = try NativeCloudKitSystemFields.restore(fields, target: .init(zone: zone.zoneName, name: record.name, recordType: "MGEntity"))
            #expect(restored.recordID == original.recordID)
        }
        #expect(records[1].assetURL == nil && records[3].assetURL == nil)
        let copied = records.compactMap(\.assetURL)
        #expect(copied.count == 2)
        for index in [0, 2] {
            let copy = try #require(records[index].assetURL)
            try FileManager.default.removeItem(at: sources[index])
            #expect(try Data(contentsOf: copy) == bytes[index])
        }
        #expect(try FileManager.default.contentsOfDirectory(atPath: stagingDirectory.path).count == 2)
        withExtendedLifetime(delivery) {}
        delivery = nil
        #expect(copied.allSatisfy { !FileManager.default.fileExists(atPath: $0.path) })
        #expect(try FileManager.default.contentsOfDirectory(atPath: stagingDirectory.path).isEmpty)
        // Both default count reservations must be available after cleanup.
        try bytes[0].write(to: sources[0])
        let retries = try (0..<2).map { _ in try staging.copy(sources[0], cancelled: { false }) }
        withExtendedLifetime(retries) {}
    }

    enum IntakeFault: CaseIterable, Sendable { case destinationOpen, noSpace, cancellation }

    @Test(arguments: IntakeFault.allCases)
    func genuineIntakeFailuresPropagateAndReleaseAllFilesAndReservations(_ fault: IntakeFault) throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk")
        try Data(repeating: 7, count: 131_073).write(to: source)
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        let native = (0..<2).map { index in
            let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-\(index)", zoneID: zone))
            record.encryptedValues["e"] = Data([UInt8(index + 1)]) as CKRecordValue
            record["pa"] = CKAsset(fileURL: source)
            return record
        }
        let stagingDirectory = directory.appendingPathComponent("native")
        let opens = Mutex(0)
        let enabled = Mutex(true)
        let outOfSpace = Mutex(false)
        let staging = try CloudMediaStaging(directory: stagingDirectory, available: {
            outOfSpace.withLock { $0 } ? 0 : UInt64.max
        }, beforeSourceRead: { _, boundary in
            guard enabled.withLock({ $0 }), case .open = boundary,
                  opens.withLock({ $0 += 1; return $0 == 2 }) else { return }
            switch fault {
            case .destinationOpen:
                // Turn only the new empty target into a directory. The source
                // opens normally; the real destination FileHandle open fails.
                let runs = try FileManager.default.contentsOfDirectory(at: stagingDirectory, includingPropertiesForKeys: nil)
                let target = try #require(runs.map { $0.appendingPathComponent("asset") }.first {
                    try $0.resourceValues(forKeys: [.fileSizeKey]).fileSize == 0
                })
                try FileManager.default.removeItem(at: target)
                try FileManager.default.createDirectory(at: target, withIntermediateDirectories: false)
            case .noSpace: outOfSpace.withLock { $0 = true }
            case .cancellation: throw CancellationError()
            }
        })
        do {
            _ = try NativeCloudFetchedPayloads(records: native, deleted: [], staging: staging)
            Issue.record("expected destination error")
        } catch {
            switch fault {
            case .destinationOpen:
                #expect(!(error is CloudMediaSourceFailure))
                #expect((error as NSError).domain == NSCocoaErrorDomain)
            case .noSpace:
                guard case CloudMediaFailure.noSpace = error else { Issue.record("noSpace changed: \(error)"); return }
            case .cancellation: #expect(error is CancellationError)
            }
        }
        #expect(opens.withLock { $0 } == 2)
        #expect(try FileManager.default.contentsOfDirectory(atPath: stagingDirectory.path).isEmpty)
        enabled.withLock { $0 = false }; outOfSpace.withLock { $0 = false }
        let retries = try (0..<2).map { _ in try staging.copy(source, cancelled: { false }) }
        #expect(try retries.allSatisfy { try Data(contentsOf: $0.url) == Data(repeating: 7, count: 131_073) })
        withExtendedLifetime(retries) {}
    }

    @Test func blobSourceReadFailureStillFailsInsteadOfDeliveringAnAbsentAsset() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-blob")
        try Data([1, 2, 3]).write(to: source)
        let stagingDirectory = directory.appendingPathComponent("native")
        let staging = try CloudMediaStaging(directory: stagingDirectory, available: { UInt64.max }, beforeSourceRead: { _, boundary in
            if case .read(let input) = boundary { try input.close() }
        })
        let request = CloudMediaRequest.download(name: "baaaaaaaaaaaaaaaaaaaaaaaaaa")
        let record = CKRecord(recordType: "MGBlob", recordID: try NativeCloudMediaRecords.id(request))
        record.encryptedValues["e"] = Data([1]) as CKRecordValue
        record["a"] = CKAsset(fileURL: source)
        do {
            _ = try NativeCloudMediaRecords.read(record, request: request, staging: staging, cancelled: { false })
            Issue.record("MGBlob source failure became a successful read")
        } catch {
            #expect((error as NSError).domain == NSCocoaErrorDomain)
            #expect(!(error is CloudMediaSourceFailure))
            guard case .failed(let code) = NativeCloudMediaRecords.failure(error).result else { Issue.record("MGBlob failure changed"); return }
            #expect(code == 0)
        }
        #expect(try FileManager.default.contentsOfDirectory(atPath: stagingDirectory.path).isEmpty)
    }

    @Test func fetchedAssetsExceedingDefaultByteBudgetKeepMetadataAndAvailableLeases() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        let stagingDirectory = directory.appendingPathComponent("native")
        let staging = try CloudMediaStaging(directory: stagingDirectory, available: { UInt64.max })
        #expect(CloudMediaStaging.assetLimit == 100 * 1_024 * 1_024)
        #expect(CloudMediaStaging.stagedLimit == 256 * 1_024 * 1_024)
        var sources: [URL] = []
        var native: [CKRecord] = []
        for index in 0..<3 {
            let source = directory.appendingPathComponent("sdk-pa-\(index)")
            // Sparse files bound allocation while exercising full-size copies.
            // Each endpoint has a file-specific byte.
            #expect(FileManager.default.createFile(atPath: source.path, contents: nil))
            let file = try FileHandle(forWritingTo: source)
            let marker = Data([UInt8(index + 1)])
            try file.write(contentsOf: marker)
            try file.truncate(atOffset: CloudMediaStaging.assetLimit)
            try file.seek(toOffset: CloudMediaStaging.assetLimit - 1)
            try file.write(contentsOf: marker)
            try file.close()
            try NativeCloudRecordConversion.validatePayloadFile(source)
            let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-\(index)", zoneID: zone))
            record.encryptedValues["e"] = marker as CKRecordValue
            record["pa"] = CKAsset(fileURL: source)
            sources.append(source); native.append(record)
        }
        let inline = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "inline-after-assets", zoneID: zone))
        inline.encryptedValues["e"] = Data([4]) as CKRecordValue
        native.append(inline)
        let deletion = CKRecord.ID(recordName: "deleted", zoneID: zone)
        var delivery: NativeCloudFetchedPayloads? = try .init(records: native, deleted: [deletion], staging: staging)
        let event = try #require(delivery?.events.first)
        #expect(delivery?.events.count == 1)
        guard case .fetched(let deliveredZone, let records, let deleted) = event else { Issue.record("fetched event missing"); return }
        try #require(records.count == native.count)
        #expect(deliveredZone == zone.zoneName && deleted == [deletion.recordName])
        for (record, original) in zip(records, native) {
            #expect(record.name == original.recordID.recordName)
            #expect(record.zone == zone.zoneName && record.recordType == "MGEntity")
            let envelope = try #require(original.encryptedValues["e"] as? Data)
            #expect(record.envelope == envelope)
            let fields = try #require(record.systemFields)
            let restored = try NativeCloudKitSystemFields.restore(fields, target: .init(zone: zone.zoneName, name: record.name, recordType: "MGEntity"))
            #expect(restored.recordID == original.recordID)
        }
        let copied = records.compactMap(\.assetURL)
        #expect(copied.count == 2)
        #expect(records[0].assetURL != nil && records[1].assetURL != nil)
        #expect(records[2].assetURL == nil && records[3].assetURL == nil)
        #expect(try FileManager.default.contentsOfDirectory(atPath: stagingDirectory.path).count == 2)
        var retainedBytes: UInt64 = 0
        for (index, copy) in copied.enumerated() {
            #expect(copy != sources[index])
            try FileManager.default.removeItem(at: sources[index])
            let size = try #require(copy.resourceValues(forKeys: [.fileSizeKey]).fileSize)
            #expect(UInt64(size) == CloudMediaStaging.assetLimit)
            retainedBytes += UInt64(size)
            let file = try FileHandle(forReadingFrom: copy)
            #expect(try file.read(upToCount: 1) == Data([UInt8(index + 1)]))
            try file.seek(toOffset: CloudMediaStaging.assetLimit - 1)
            #expect(try file.read(upToCount: 1) == Data([UInt8(index + 1)]))
            try file.close()
        }
        #expect(retainedBytes == 200 * 1_024 * 1_024 && retainedBytes <= CloudMediaStaging.stagedLimit)
        // No hidden third copy or released reservation can bypass the shared
        // production budget while the event still owns the first two files.
        do {
            let unexpected = try staging.copy(sources[2], cancelled: { false }, retainedAssetLimit: Int.max)
            withExtendedLifetime(unexpected) {}
            Issue.record("shared staging budget admitted a third 100 MiB file")
        } catch CloudMediaFailure.stagingLimit {}
        withExtendedLifetime(delivery) {}
        delivery = nil
        #expect(copied.allSatisfy { !FileManager.default.fileExists(atPath: $0.path) })
        #expect(try FileManager.default.contentsOfDirectory(atPath: stagingDirectory.path).isEmpty)
        // The original lease releases both the files and byte reservations.
        let retry = try staging.copy(sources[2], cancelled: { false })
        #expect(FileManager.default.fileExists(atPath: retry.url.path))
        withExtendedLifetime(retry) {}
    }

    @Test func fetchedEventRetainsCopiedPayloadAfterSDKSourceDeletionUntilDeliveryReleasesIt() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        let bytes = Data(repeating: 0x71, count: 800_000)
        try bytes.write(to: source)
        let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-note", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
        record.encryptedValues["e"] = Data([1]) as CKRecordValue
        record["pa"] = CKAsset(fileURL: source)
        let staging = try CloudMediaStaging(directory: directory.appendingPathComponent("native"), available: { 1_000_000_000 })
        var delivery: NativeCloudFetchedPayloads? = try .init(records: [record], deleted: [], staging: staging)
        let event = try #require(delivery?.events.first)
        guard case .fetched(let zone, let records, let deleted) = event else { Issue.record("fetched event missing"); return }
        let copied = try #require(records.first?.assetURL)
        #expect(zone == "c-synthetic" && deleted.isEmpty)
        #expect(copied != source)
        try FileManager.default.removeItem(at: source)
        #expect(try Data(contentsOf: copied) == bytes)
        // Metadata conversion of a Saved never adopts the download URL.
        #expect(try NativeCloudRecordConversion.decode(record).assetURL == nil)
        withExtendedLifetime(delivery) {}
        delivery = nil
        #expect(!FileManager.default.fileExists(atPath: copied.path))
    }

    @Test func unreadableControlKeepsRoutingAndOriginalSystemFields() throws {
        for (type, zone, name) in [("MGActive", "meguri-root", "active"), ("MGEpoch", "c-synthetic", "epoch")] {
            let record = CKRecord(recordType: type, recordID: .init(recordName: name, zoneID: NativeCloudOwnRecords.zone(zone)))
            let expected = try NativeCloudKitSystemFields.encode(record)
            let value = try NativeCloudRecordConversion.decode(record)
            #expect(value.name == name && value.zone == zone && value.recordType == type)
            #expect(value.envelope.isEmpty && value.systemFields == expected)
            // Malformed encrypted fields remain a native shape failure.
            record.encryptedValues["e"] = "synthetic-wrong-type" as CKRecordValue
            #expect(throws: NativeCloudKitFailure.self) { try NativeCloudRecordConversion.decode(record) }
        }
        let entity = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-note", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudRecordConversion.decode(entity) }
        let wrongRoute = CKRecord(recordType: "MGActive", recordID: .init(recordName: "epoch", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudRecordConversion.decode(wrongRoute) }
    }

    @Test func entityPayloadFieldDoesNotRelaxControlOrOtherAssetFields() throws {
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        for type in ["MGEntity", "MGEpoch"] {
            for field in ["pa", "ma", "a"] {
                let record = CKRecord(recordType: type, recordID: .init(recordName: type == "MGEpoch" ? "epoch" : "ordinary-note", zoneID: zone))
                record.encryptedValues["e"] = Data([1]) as CKRecordValue
                record[field] = CKAsset(fileURL: URL(fileURLWithPath: "/tmp/synthetic-unavailable-payload"))
                if type == "MGEntity" && field == "pa" {
                    #expect(try NativeCloudRecordConversion.decode(record).assetURL == nil)
                } else {
                    #expect(throws: NativeCloudKitFailure.self) { try NativeCloudRecordConversion.decode(record) }
                }
            }
        }
    }
}
#endif
