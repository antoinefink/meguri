import Foundation
import Testing
@testable import AppModel
@testable import CoreBridge

#if os(macOS)
#if DEBUG
extension ReviewCollectionStartupTests {
    @Test func unavailableIdentityResultDuringOwnedShutdownStaysSticky() async throws {
        try await withIssuedStartupAdmissionCommands(test: "N1") { media, _ in
            let backend = SyncOwnerFake(), gate = SyncOwnerGate()
            defer { gate.release(); backend.executeCommand = nil; backend.onQuiesce = nil }
            backend.executeCommand = { _ in
                await gate.hold()
                // MP3.2: cancellation belongs to the closing command owner.
                #expect(Task.isCancelled && SyncCommandTaskContext.current?.shutdownCancelled == true)
                return SyncCommandResult(outputs: [],
                    error: BridgeError.unavailable(reason: "cloud runtime account is unverified"), deferred: false)
            }
            backend.onQuiesce = { gate.release() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform()
            await sync.completeOpenedHost(generation: 301)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [media]
            sync.receive(.refresh(.cloud))
            await gate.waitUntilHeld()
            let close = Task { await sync.cancelAndDrain() }
            // MP3.2: join the held command through its owner before close returns.
            #expect(await close.value == true)
            // Sticky rule at SyncCoordinator.swift:2137-2147.
            #expect(sync.operationFailed)
            #expect(sync.failureRevision == 1)
            // MP3.2: the retired backend closes exactly once.
            #expect(backend.closed && backend.closeCount == 1)
        }
    }

    @Test func unavailableIdentityResultWithoutCancellationStaysSticky() async throws {
        try await withIssuedStartupAdmissionCommands(test: "N2") { media, _ in
            let backend = SyncOwnerFake(), gate = SyncOwnerGate()
            var cancelled = false
            defer { gate.release(); backend.executeCommand = nil; backend.onQuiesce = nil }
            backend.executeCommand = { _ in
                await gate.hold()
                cancelled = Task.isCancelled
                return SyncCommandResult(outputs: [],
                    error: BridgeError.unavailable(reason: "cloud runtime account is unverified"), deferred: false)
            }
            backend.onQuiesce = { gate.release() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform()
            await sync.completeOpenedHost(generation: 302)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [media]
            sync.receive(.refresh(.cloud))
            await gate.waitUntilHeld()
            gate.release()
            await backend.waitForExecution()
            await sync.waitForInputs()
            // Sticky rule at SyncCoordinator.swift:2137-2147.
            #expect(!cancelled)
            #expect(sync.operationFailed)
            #expect(sync.failureRevision == 1)
            // MP3.2: close joins the command's owner.
            #expect(await sync.cancelAndDrain())
            // Sticky rule at SyncCoordinator.swift:2137-2147.
            #expect(sync.operationFailed)
            #expect(sync.failureRevision == 1)
            // MP3.2: the retired backend closes exactly once.
            #expect(backend.closed && backend.closeCount == 1)
        }
    }

    @Test func unavailableIdentityResultUnderBackgroundExpiryStaysSticky() async throws {
        try await withIssuedStartupAdmissionCommands(test: "N3") { media, _ in
            let backend = SyncOwnerFake(), gate = SyncOwnerGate()
            defer { gate.release(); backend.executeCommand = nil; backend.onQuiesce = nil }
            backend.executeCommand = { _ in
                await gate.hold()
                // MP3.2: expiry cancellation keeps its original owner.
                #expect(Task.isCancelled)
                #expect(SyncCommandTaskContext.current?.shutdownCancellationRequested == false)
                return SyncCommandResult(outputs: [],
                    error: BridgeError.unavailable(reason: "cloud runtime account is unverified"), deferred: false)
            }
            backend.onQuiesce = { gate.release() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform()
            await sync.completeOpenedHost(generation: 303)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [media]
            sync.receive(.refresh(.cloud))
            await gate.waitUntilHeld()
            sync.backgroundExpired()
            gate.release()
            await backend.waitForExecution()
            // MP3.2: close joins the command's owner after expiry.
            #expect(await sync.cancelAndDrain())
            // Sticky rule at SyncCoordinator.swift:2137-2147.
            #expect(sync.operationFailed)
            #expect(sync.failureRevision == 1)
            // MP3.2: the retired backend closes exactly once.
            #expect(backend.closed && backend.closeCount == 1)
        }
    }
}

// Retain the real host and pass only observed issued handles to SyncOwnerFake.
@MainActor private func withIssuedStartupAdmissionCommands(
    test: String,
    _ body: @MainActor (CloudPolicyCommand, CloudPolicyCommand) async throws -> Void
) async throws {
    try await withIssuedStartupAdmissionHarness(test: test) { harness in
        let backend = try harness.backend
        let observed = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        defer { backend.onCommands = nil; observed.continuation.finish() }
        var media: CloudPolicyCommand?, update: CloudPolicyCommand?
        backend.onCommands = { commands in
            for command in commands {
                switch command.action {
                case .resumeMedia where media == nil: media = command
                case .updatePending where update == nil: update = command
                default: break
                }
            }
            if media != nil && update != nil {
                observed.continuation.yield(true)
                observed.continuation.finish()
            }
        }
        try await harness.exchange(phase: .aBaseline)
        // MP3.2: retain actual issued commands before substituting execution.
        try #require(try await serviceAppSignal(observed.stream, stage: "issued startup admission fixture commands"))
        backend.onCommands = nil
        try await body(try #require(media), try #require(update))
    }
}

@MainActor private func withIssuedStartupAdmissionHarness(
    test: String,
    _ body: @MainActor (ServiceAppHarness) async throws -> Void
) async throws {
    let fixture = try InitialCloudFixture.load()
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-issued-startup-admission-\(test)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    defer {
        do { try FileManager.default.removeItem(at: root) }
        catch { Issue.record(error, "Remove issued startup admission fixture directory") }
    }
    let serviceDirectory = root.appending(path: "service")
    try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, transport: .binaryAssetsV1)
    var active: ServiceAppHarness?
    do {
        let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
        let harness = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire)
        active = harness
        defer { harness.defaults.removePersistentDomain(forName: harness.suite) }
        try await wire.seed(fixture)
        try await harness.adopt()
        try await body(harness)
        // MP3.2: join the collection owner before stopping its Service.
        try #require(await harness.owner.close())
        let exit = await process.close()
        // MP3.2: join the owned Service with its strict shutdown evidence.
        try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
    } catch {
        if let active { await active.cleanupAfterServiceFailure() }
        else {
            let exit = await process.cancel()
            // MP3.2: failure cleanup still joins the owned Service.
            #expect(exit.joined, "Issued startup admission fixture must join its owned Service process")
        }
        throw error
    }
}
#endif
#endif
