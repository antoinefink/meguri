import Darwin
import Dispatch
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

private struct ExportPublicationHook: CollectionExportPublication {
    let action: @Sendable (URL, URL, Bool) throws -> Void
    func publish(_ staged: URL, to destination: URL, replacing: Bool) throws {
        try action(staged, destination, replacing)
    }
}
private final class StagedExportWitness: Sendable {
    let url = Mutex<URL?>(nil)
    let calls = Mutex(0)
}
private final class RedirectedExportAccess: CollectionExportDestinationAccess {
    let actual: URL
    let active = Mutex(false)
    let failCompletion: Bool
    init(actual: URL, failCompletion: Bool) { self.actual = actual; self.failCompletion = failCompletion }
    func write(_ url: URL, accessor: @escaping @Sendable (URL) -> Void) throws {
        active.withLock { $0 = true }
        defer { active.withLock { $0 = false } }
        accessor(actual)
        if failCompletion { throw CocoaError(.fileWriteUnknown) }
    }
}
@Suite struct StagedCollectionExportTests {
    private func temporaryRoot() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        return root
    }
    @Test(arguments: [false, true])
    func actualCoordinatedFoundationStagingAndPublication(replacing: Bool) async throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let destination = root.appendingPathComponent("user.colpkg")
        if replacing { try "original".write(to: destination, atomically: false, encoding: .utf8) }
        let witness = StagedExportWitness()
        let publisher = StagedCollectionExportDestination()
        try await CoordinatedExportDestination().withDestination(destination) { actual in
            try await publisher.save(to: actual, operation: CollectionExportOperation()) { staged in
                #expect(!Thread.isMainThread)
                #expect(staged != actual && staged.deletingLastPathComponent() != root)
                let files = FileManager()
                let stageVolume = try files.attributesOfFileSystem(forPath: staged.deletingLastPathComponent().path)
                let destinationVolume = try files.attributesOfFileSystem(forPath: root.path)
                let stageDevice = try #require(stageVolume[.systemNumber] as? NSNumber)
                let destinationDevice = try #require(destinationVolume[.systemNumber] as? NSNumber)
                #expect(stageDevice == destinationDevice)
                if replacing { #expect(try String(contentsOf: actual, encoding: .utf8) == "original") }
                else { #expect(!files.fileExists(atPath: actual.path)) }
                witness.url.withLock { $0 = staged }
                // Tiny fixture only. Production bytes are copied/verified/synced in Rust.
                try "completed snapshot".write(to: staged, atomically: false, encoding: .utf8)
            }
        }
        #expect(try String(contentsOf: destination, encoding: .utf8) == "completed snapshot")
        let staged = try #require(witness.url.withLock { $0 })
        #expect(!FileManager.default.fileExists(atPath: staged.path))
        #expect(!FileManager.default.fileExists(atPath: staged.deletingLastPathComponent().path))
    }
    @Test(arguments: [false, true])
    func coordinatedURLAndAccessLastThroughActualPublication(failCompletion: Bool) async throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let selected = root.appendingPathComponent("selected.colpkg")
        try "selected original".write(to: selected, atomically: false, encoding: .utf8)
        let actual = root.appendingPathComponent("coordinated.colpkg")
        let access = RedirectedExportAccess(actual: actual, failCompletion: failCompletion)
        let publisher = StagedCollectionExportDestination(publication: ExportPublicationHook { staged, final, replace in
            #expect(access.active.withLock { $0 } && final == actual && !replace)
            try FoundationCollectionExportPublication().publish(staged, to: final, replacing: replace)
            #expect(access.active.withLock { $0 })
        })
        do {
            try await CoordinatedExportDestination(access: access).withDestination(selected) { coordinated in
                try await publisher.save(to: coordinated, operation: CollectionExportOperation()) { staged in
                    #expect(access.active.withLock { $0 })
                    try "snapshot".write(to: staged, atomically: false, encoding: .utf8)
                }
            }
            #expect(!failCompletion)
        } catch { #expect(failCompletion && error is ExportDestinationCompletionUncertain) }
        #expect(!access.active.withLock { $0 })
        #expect(try String(contentsOf: actual, encoding: .utf8) == "snapshot")
        #expect(try String(contentsOf: selected, encoding: .utf8) == "selected original")
    }
    @Test func actualExclusiveNewMoveRefusesConcurrentItemAndCleansStage() async throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let destination = root.appendingPathComponent("user.colpkg")
        let witness = StagedExportWitness()
        do {
            try await StagedCollectionExportDestination().save(to: destination, operation: CollectionExportOperation()) { staged in
                witness.url.withLock { $0 = staged }
                try "snapshot".write(to: staged, atomically: false, encoding: .utf8)
                try "concurrent user item".write(to: destination, atomically: false, encoding: .utf8)
            }
            Issue.record("Expected exclusive publication refusal")
        } catch {
            let failure = error as NSError
            #expect(failure.domain == NSPOSIXErrorDomain && failure.code == Int(EEXIST))
            #expect(!(error is ExportDestinationCompletionUncertain))
        }
        #expect(try String(contentsOf: destination, encoding: .utf8) == "concurrent user item")
        let staged = try #require(witness.url.withLock { $0 })
        #expect(!FileManager.default.fileExists(atPath: staged.deletingLastPathComponent().path))
    }
    @Test(arguments: [false, true])
    func prepublicationFailureOrCancellationPreservesOriginalAndCleansStage(cancel: Bool) async throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let destination = root.appendingPathComponent("user.colpkg")
        try "original".write(to: destination, atomically: false, encoding: .utf8)
        let operation = CollectionExportOperation()
        let witness = StagedExportWitness()
        do {
            try await StagedCollectionExportDestination().save(to: destination, operation: operation) { staged in
                witness.url.withLock { $0 = staged }
                try "partial or completed stage".write(to: staged, atomically: false, encoding: .utf8)
                if cancel { await operation.cancel() }
                else { throw BridgeError.unavailable(reason: "stage verification failed") }
            }
            Issue.record("Expected prepublication failure")
        } catch {
            #expect(!(error is ExportDestinationCompletionUncertain))
            if cancel { #expect(error as? BridgeError == .cancelled) }
        }
        #expect(try String(contentsOf: destination, encoding: .utf8) == "original")
        let staged = try #require(witness.url.withLock { $0 })
        #expect(!FileManager.default.fileExists(atPath: staged.deletingLastPathComponent().path))
    }
    @Test(arguments: [false, true])
    func successfulNativePublicationWinsLateCancel(replacing: Bool) async throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let destination = root.appendingPathComponent("user.colpkg")
        if replacing { try "original".write(to: destination, atomically: false, encoding: .utf8) }
        let operation = CollectionExportOperation()
        let publisher = StagedCollectionExportDestination(publication: ExportPublicationHook { staged, final, replace in
            #expect(!Thread.isMainThread && replace == replacing)
            try FoundationCollectionExportPublication().publish(staged, to: final, replacing: replace)
            let cancelled = DispatchSemaphore(value: 0)
            Task { await operation.cancel(); cancelled.signal() }
            cancelled.wait()
        })
        try await publisher.save(to: destination, operation: operation) { staged in
            try "snapshot".write(to: staged, atomically: false, encoding: .utf8)
        }
        #expect(throws: BridgeError.cancelled) { try operation.check() }
        #expect(try String(contentsOf: destination, encoding: .utf8) == "snapshot")
    }
    @Test(arguments: [false, true])
    func completionFailureAfterActualNativePublicationRetainsRecovery(replacing: Bool) async throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let destination = root.appendingPathComponent("user.colpkg")
        if replacing { try "original".write(to: destination, atomically: false, encoding: .utf8) }
        let witness = StagedExportWitness()
        let publisher = StagedCollectionExportDestination(publication: ExportPublicationHook { staged, final, replace in
            #expect(!Thread.isMainThread && replace == replacing)
            witness.calls.withLock { $0 += 1 }
            let recoveryFile = staged.deletingLastPathComponent().appendingPathComponent("recovery-item")
            try "retained recovery".write(to: recoveryFile, atomically: false, encoding: .utf8)
            witness.url.withLock { $0 = recoveryFile }
            try FoundationCollectionExportPublication().publish(staged, to: final, replacing: replace)
            throw CocoaError(.fileWriteUnknown) // Completion failure after the actual native move/replace.
        })
        do {
            try await publisher.save(to: destination, operation: CollectionExportOperation()) { staged in
                try "snapshot".write(to: staged, atomically: false, encoding: .utf8)
            }
            Issue.record("Expected uncertain publication")
        } catch {
            let failure = try #require(error as? ExportDestinationCompletionUncertain)
            #expect(failure.destination == destination)
            let recoveryFile = try #require(witness.url.withLock { $0 })
            defer { try? FileManager.default.removeItem(at: recoveryFile.deletingLastPathComponent()) }
            #expect(failure.recoveryDirectory == recoveryFile.deletingLastPathComponent())
            #expect(try String(contentsOf: recoveryFile, encoding: .utf8) == "retained recovery")
        }
        #expect(witness.calls.withLock { $0 } == 1)
        #expect(try String(contentsOf: destination, encoding: .utf8) == "snapshot")
    }
    @Test func actualFoundationReplacementErrorRetainsStagedContents() async throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let destination = root.appendingPathComponent("user.colpkg")
        try "original".write(to: destination, atomically: false, encoding: .utf8)
        let publisher = StagedCollectionExportDestination(publication: ExportPublicationHook { staged, final, replace in
            #expect(replace)
            // Exercise a real Foundation error without risking the original:
            // an absent replacement must not cause staging/recovery cleanup.
            try FoundationCollectionExportPublication().publish(staged.appendingPathExtension("missing"), to: final, replacing: true)
        })
        do {
            try await publisher.save(to: destination, operation: CollectionExportOperation()) { staged in
                try "snapshot".write(to: staged, atomically: false, encoding: .utf8)
            }
            Issue.record("Expected Foundation replacement error")
        } catch {
            let failure = try #require(error as? ExportDestinationCompletionUncertain)
            let directory = try #require(failure.recoveryDirectory)
            defer { try? FileManager.default.removeItem(at: directory) }
            #expect(try String(contentsOf: directory.appendingPathComponent("Meguri-Collection.colpkg"), encoding: .utf8) == "snapshot")
        }
        #expect(try String(contentsOf: destination, encoding: .utf8) == "original")
    }
}
