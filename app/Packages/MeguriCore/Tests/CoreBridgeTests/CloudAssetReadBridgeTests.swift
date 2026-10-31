import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

struct CloudAssetReadBridgeTests {
    enum HandoffOutcome: CaseIterable, Sendable { case success, failure, cancellation }

    @Test(arguments: HandoffOutcome.allCases)
    func ownedResultRetainsFileThroughAwaitedHandoff(outcome: HandoffOutcome) async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        let bytes = Data(repeating: 0x37, count: 131_073)
        try bytes.write(to: source)
        let copies = directory.appendingPathComponent("owned")
        let staging = try CloudMediaStaging(directory: copies, available: { UInt64.max })
        let gate = CloudCallbackGate()
        let task = Task<Void, any Error> {
            do {
                // No outer variable or SDK source owns the copied file. The
                // temporary result must survive raw conversion and suspension.
                try await Self.ownedResult(staging: staging, source: source).withRaw { value in
                    await gate.enter()
                    try await MaintenanceLane.shared.call("test.cloudAssetReadOwnedHandoff") {
                        let raw = try #require(value)
                        #expect(raw.name == "projected-record" && raw.zone == "projected-zone")
                        #expect(raw.recordType == "MGEntity")
                        #expect(raw.envelope == Data([1, 3, 5]) && raw.systemFields == Data([2, 4]))
                        let path = try #require(raw.assetPath)
                        #expect(try Data(contentsOf: URL(fileURLWithPath: path)) == bytes)
                        if case .failure = outcome { throw BridgeError.invalidArgument("asset handoff failed") }
                        if case .cancellation = outcome { #expect(Task.isCancelled) }
                    }
                    if case .cancellation = outcome { try Task.checkCancellation() }
                }
                _ = await gate.operationSucceeded()
            } catch {
                await gate.abort(error)
                throw error
            }
        }
        do {
            try await gate.waitForArrival()
            #expect(!FileManager.default.fileExists(atPath: source.path))
            let runs = try FileManager.default.contentsOfDirectory(at: copies, includingPropertiesForKeys: nil)
            let copied = try #require(runs.first).appendingPathComponent("asset")
            #expect(runs.count == 1)
            #expect(try Data(contentsOf: copied) == bytes)
            if case .cancellation = outcome { task.cancel() }
            await gate.open()
            let result = await task.result
            switch (outcome, result) {
            case (.success, .success): break
            case (.failure, .failure(let error)):
                #expect((error as? BridgeError) == .invalidArgument("asset handoff failed"))
            case (.cancellation, .failure(let error)):
                #expect(error is CancellationError)
            default: Issue.record("unexpected handoff outcome: \(result)")
            }
            #expect(!FileManager.default.fileExists(atPath: copied.path))
            #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
        } catch {
            task.cancel()
            await gate.open()
            _ = await task.result
            throw error
        }
    }

    private static func ownedResult(staging: CloudMediaStaging, source: URL) throws -> CloudAssetReadResult {
        let asset = try staging.copyPayload(source, cancelled: { false })
        try FileManager.default.removeItem(at: source)
        let record = CoreBridge.CloudRecord(name: "projected-record", envelope: Data([1, 3, 5]),
                                           systemFields: Data([2, 4]), zone: "projected-zone", assetURL: asset.url)
        return try CloudAssetReadResult(record: record, asset: asset)
    }

    @Test func resultRequiresItsActualMatchingLease() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        try Data([7]).write(to: source)
        let staging = try CloudMediaStaging(directory: directory.appendingPathComponent("owned"), available: { UInt64.max })
        let asset = try staging.copyPayload(source, cancelled: { false })
        let matching = CoreBridge.CloudRecord(name: "projection", envelope: Data(), systemFields: nil, assetURL: asset.url)
        #expect(throws: CloudAssetReadResultFailure.self) { try CloudAssetReadResult(record: matching) }
        let other = CoreBridge.CloudRecord(name: "projection", envelope: Data(), systemFields: nil, assetURL: source)
        #expect(throws: CloudAssetReadResultFailure.self) { try CloudAssetReadResult(record: other, asset: asset) }
        let inline = CoreBridge.CloudRecord(name: "projection", envelope: Data(), systemFields: nil)
        #expect(throws: CloudAssetReadResultFailure.self) { try CloudAssetReadResult(record: inline, asset: asset) }
    }

    @Test func inlineAndFailedReadsNeedNoFile() async throws {
        let inline = CoreBridge.CloudRecord(name: "inline", envelope: Data([8]), systemFields: nil, zone: "projected-zone")
        try await CloudAssetReadResult(record: inline).withRaw { raw in
            await Task.yield()
            #expect(raw?.name == inline.name && raw?.envelope == inline.envelope)
            #expect(raw?.zone == inline.zone && raw?.assetPath == nil)
        }
        await CloudAssetReadResult.failed.withRaw { raw in
            await Task.yield()
            #expect(raw == nil)
        }
    }

    private final class PreparedQueue: CoreBridge.CloudAssetReadQueue {
        let accepted: Bool
        let calls = Mutex(0)
        init(accepted: Bool) { self.accepted = accepted }
        func enqueue() -> Bool { calls.withLock { $0 += 1 }; return accepted }
    }

    @Test(arguments: [false, true])
    func foreignQueueRetainsAndSynchronouslyForwardsPreparedOperation(accepted: Bool) {
        var prepared: PreparedQueue? = PreparedQueue(accepted: accepted)
        weak let original = prepared
        var foreign: (any MeguriCoreBindings.CloudAssetReadQueue)? = ForeignAssetReadQueue(prepared!)
        prepared = nil
        #expect(original != nil)
        #expect(foreign?.enqueue() == accepted)
        #expect(original?.calls.withLock { $0 } == 1)
        // Admission is the original operation's fact; this adapter adds no
        // cancellation or terminal cleanup for either callback result.
        #expect(original?.accepted == accepted)
        foreign = nil
        #expect(original == nil)
    }

    @Test func rawKindAndIntakeAcceptanceAreProjectedWithoutInference() {
        #expect(CoreBridge.CloudAssetReadKind(MeguriCoreBindings.CloudAssetReadKind.retry) == .retry)
        #expect(CoreBridge.CloudAssetReadKind(MeguriCoreBindings.CloudAssetReadKind.saveConflictFirst) == .saveConflictFirst)
        for accepted in [false, true] {
            let raw = MeguriCoreBindings.CloudAssetReadCompletion(intakeAccepted: accepted)
            #expect(CoreBridge.CloudAssetReadCompletion(raw).intakeAccepted == accepted)
        }
    }
}

extension EventTests {
    @Test func actualHostCanBeginAndFinishAnEmptyFirstAssetReadPass() async throws {
        try await withCloudKitFixture { fixture in
            let engine = try await fixture.engine()
            let pass = try await fixture.core.beginFirstAssetReadPass(sdk: engine.instanceID)
            #expect(try await pass.next() == nil)
            try await pass.finish()
        }
    }
}
