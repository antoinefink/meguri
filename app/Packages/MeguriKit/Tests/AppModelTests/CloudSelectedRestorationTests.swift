#if os(macOS) && DEBUG
@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

// Actual Core/Service reopen coverage. These tests supply no binding, ACK or
// outcome rows. Transport replies and original issued commands remain real.
extension ReviewCollectionStartupTests {
    @Test func selectedPublishedEditAndExactIssuedExchangeSurviveFreshCoreOwner() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-restored-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let input = try await selectedBootstrapSource(root: root)
        let service = root.appending(path: "service")
        try FileManager.default.createDirectory(at: service, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: service, pageSize: 32)
        var active: ServiceAppHarness?
        var publicationOwner: ServiceAppHarness?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let first = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire, clock: .system)
            defer { first.defaults.removePersistentDomain(forName: first.suite) }
            active = first
            publicationOwner = first
            try FileManager.default.createDirectory(at: first.root, withIntermediateDirectories: false)
            try input.bytes.write(to: first.root.appending(path: "collection.anki2"))
            try await selectedBootstrapPublish(first)
            await first.sync.waitForInputs(); await first.sync.drainOperations(); await first.sync.waitForInputs()
            let inspector = SelectedCloudRestorationInspection()
            let publication = try await inspector.originalControlWitness()
            try #require(await first.owner.close())

            // The post-publish prefix reopens existing mutable data through a
            // fresh ReviewCollection, before permitting its first original edit.
            let second = try ServiceAppHarness(root: first.root, wire: wire, defaults: first.defaults, clock: .system)
            active = second
            let role = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
            var admitted = 0
            second.beforeBackend = {
                let control = try await inspector.control(original: publication)
                let owned = try await role.validateOwned()
                try #require(control.generation == owned.generation && !owned.qChanged)
                admitted += 1
            }
            second.observeOwn = { value in
                if value.metadata.kind == .saveCompleteEpoch { Issue.record("Reopen must not republish") }
            }
            try await second.reopen()
            let current = try await SelectedCloudControlInspection().current()
            #expect(current.status == .missing(.missingAfterRestart)) // Keep live semantics.
            #expect(admitted == 1)
            let backend = try second.backend
            var witness: Data?, issued: CloudPolicyCommand?, original: CloudPolicyExchangeRecord?, requests = 0
            var originalWork: OriginalCloudExchangeWork?, recordHandle: CloudPolicyCommand?
            backend.beforeExecution = { command in
                if case .runExchange = command.action {
                    requests += 1; issued = command
                    witness = try? await inspector.originalExchangeWitness(command: command)
                } else if case .recordExchange = command.action {
                    recordHandle = command
                    originalWork = try? await inspector.originalExchangeWork(command: command)
                }
            }
            backend.afterExecution = { command, result in
                if case .recordExchange(let record) = command.action, result.error == nil, !result.deferred { original = record }
            }
            let edited = try await role.fixedEdit()
            #expect(edited.generation != publication.generation)
            let editWitness = try #require(edited.originalWitness)
            try #require(edited.updatedNotes == 1 && edited.source == .userUndoable && editWitness.count == 32)
            try await second.exchange()
            await second.sync.drainOperations(); await second.sync.waitForInputs()
            let record = try #require(original), exchangeWitness = try #require(witness)
            try #require(requests == 1)
            let originalSnapshot = try #require(originalWork)
            guard case .succeeded(_, let heldWork, let pendingWork, _, let originalMedia) = record.outcome else {
                throw ServiceWorkflowFailure.refused
            }
            #expect(heldWork == UInt64(Set(originalSnapshot.heldPendingNames).union(originalSnapshot.heldAcknowledgedNames).count))
            #expect(pendingWork == UInt64(Set(originalSnapshot.pendingUnheldNames).count))
            #expect(originalMedia == originalSnapshot.media)
            let categories = [originalSnapshot.acknowledgedNames, originalSnapshot.cleanNames,
                originalSnapshot.droppedNames, originalSnapshot.absentNames, originalSnapshot.blockedNames,
                originalSnapshot.outstandingNames].map { Set($0) }
            let allSelected = categories.reduce(into: Set<String>()) { $0.formUnion($1) }
            #expect(allSelected.count == categories.reduce(0) { $0 + $1.count })
            #expect(originalSnapshot.blockedNames.isEmpty && originalSnapshot.outstandingNames.isEmpty)
            #expect(originalSnapshot.incoming.isEmpty)
            guard case .succeeded(let selectedCount, _, _, _, _) = record.outcome else { throw ServiceWorkflowFailure.refused }
            #expect(UInt64(allSelected.count) == selectedCount)
            let originalAck = try await role.observe()
            try #require(originalAck.entity.acknowledged && originalAck.entity.ackedEnvelopeDigest.count == 32)
            try #require(originalSnapshot.acknowledgedNames == [originalAck.entity.recordName])
            let originalEnvelopes = [[originalAck.entity.ackedEnvelopeDigest]]
            let before = try await CoreNoteService().readNote(id: input.noteID)
            let completedHandle = try #require(issued)
            await #expect(throws: (any Error).self) {
                try await inspector.originalExchangeWitness(command: completedHandle)
            }
            await #expect(throws: (any Error).self) {
                try await inspector.originalExchangeWork(command: completedHandle)
            }
            backend.beforeExecution = nil; backend.afterExecution = nil
            try #require(await second.owner.close())
            let originalRecordHandle = try #require(recordHandle)
            await #expect(throws: (any Error).self) {
                try await inspector.originalExchangeWork(command: originalRecordHandle)
            }
            if let issued {
                await #expect(throws: (any Error).self) { try await inspector.originalExchangeWitness(command: issued) }
            }

            let third = try ServiceAppHarness(root: first.root, wire: wire, defaults: first.defaults, clock: .system)
            active = third
            let freshRole = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
            var restored: HistoricalOriginalCloudEdit?
            let creationAcks = AsyncStream<HistoricalOriginalCloudAck>.makeStream()
            await third.factory.inspectCreation { _ in
                creationAcks.continuation.yield(try await freshRole.readOriginalAck(witness: editWitness))
                let generation = try await inspector.crosscheckOriginalAcknowledgements(exchange: record.exchange,
                    witness: exchangeWitness, envelopes: originalEnvelopes)
                let owned = try await freshRole.validateOwned()
                try #require(generation == owned.generation)
            }
            third.beforeBackend = {
                _ = try await inspector.control(original: publication)
                restored = try await freshRole.readOriginalEdit(witness: editWitness)
                // Before the new backend's required scan, old observer stamps
                // cannot establish a current-opening ACK.
                await #expect(throws: (any Error).self) { try await freshRole.readOriginalAck(witness: editWitness) }
                await #expect(throws: (any Error).self) {
                    try await inspector.crosscheckOriginalAcknowledgements(exchange: record.exchange,
                        witness: exchangeWitness, envelopes: originalEnvelopes)
                }
                let actual = try await inspector.readOriginalExchange(exchange: record.exchange, witness: exchangeWitness)
                try #require(actual.exchange == record.exchange && actual.endedMilliseconds == record.endedMilliseconds)
                guard case .succeeded(let selected, let held, let newer, let incoming, let media) = actual.outcome,
                      case .succeeded(let originalSelected, let originalHeld, let originalNewer, let originalIncoming, let originalMedia) = record.outcome else {
                    throw ServiceWorkflowFailure.refused
                }
                try #require(selected == originalSelected && held == originalHeld && newer == originalNewer
                    && incoming == originalIncoming && media == originalMedia)
                // Exercise the same refusal used by runtime recovery with the
                // real exact succeeded record. Accounting supplies no old ACK.
                #expect(throws: AgentFailure.implementationMissing) {
                    try AgentSelectedRestoration.requireExchangeRecovery(.syncNowMac)
                }
                try AgentSelectedRestoration.requireExchangeRecovery(.syncNowPhone)
                await #expect(throws: (any Error).self) {
                    try await inspector.readOriginalExchange(exchange: record.exchange, witness: Data(repeating: 0, count: 32))
                }
            }
            third.observeOwn = { observation in
                if observation.metadata.kind == .saveCompleteEpoch { Issue.record("Historical inspection must not republish") }
            }
            try await third.reopen()
            creationAcks.continuation.finish()
            var creationIterator = creationAcks.stream.makeAsyncIterator()
            let atCreation = try #require(await creationIterator.next())
            let currentAck = try await freshRole.readOriginalAck(witness: editWitness)
            #expect(try await inspector.crosscheckOriginalAcknowledgements(exchange: record.exchange,
                witness: exchangeWitness, envelopes: originalEnvelopes) == currentAck.generation)
            for envelopes in [[[Data(repeating: 0, count: 32)]], originalEnvelopes + originalEnvelopes] {
                await #expect(throws: (any Error).self) {
                    try await inspector.crosscheckOriginalAcknowledgements(exchange: record.exchange,
                        witness: exchangeWitness, envelopes: envelopes)
                }
            }
            #expect(currentAck.originalGeneration == edited.generation && currentAck.originalSeq == edited.seq)
            #expect(currentAck.localRevision == edited.localRevision && currentAck.ackedRevision == edited.localRevision)
            #expect(currentAck.noteDigest == edited.noteDigest && currentAck.ackedEnvelopeDigest.count == 32)
            await #expect(throws: (any Error).self) {
                try await inspector.originalExchangeWork(command: originalRecordHandle)
            }
            let recovered = try #require(restored)
            #expect(currentAck.generation == recovered.generation && currentAck.noteDigest == recovered.noteDigest)
            #expect(atCreation.generation == currentAck.generation && atCreation.ackedRevision == currentAck.ackedRevision)
            #expect(atCreation.ackedEnvelopeDigest == currentAck.ackedEnvelopeDigest)
            #expect(recovered.originalGeneration == edited.generation && recovered.originalSeq == edited.seq)
            #expect(recovered.generation != recovered.originalGeneration)
            #expect(recovered.localRevision == edited.localRevision && recovered.noteDigest == edited.noteDigest)
            #expect(try await CoreNoteService().readNote(id: input.noteID) == before)
            #expect(requests == 1)
            await #expect(throws: (any Error).self) { try await freshRole.fixedEdit() }
            // Changed original fields invalidate the historical effect even
            // though the immutable witness and original exchange still exist.
            _ = try await CoreNoteService().updateNote(.init(id: input.noteID, fields: ["later-conflicting-edit", before.fields[1]], tags: before.tags))
            await #expect(throws: (any Error).self) { try await freshRole.readOriginalEdit(witness: editWitness) }
            try #require(await third.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        } catch {
            if let backend = publicationOwner?.backends.first {
                selectedRestorationPublicationTrace(backend)
            }
            if let active { #expect(await active.owner.close()) }
            _ = await process.close()
            throw error
        }
    }

    @Test func selectedAdoptedPhoneReopensOriginalActivationWithoutAnotherAdoption() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-selected-adopt-reopen-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let input = try await selectedBootstrapSource(root: root)
        let service = root.appending(path: "service")
        try FileManager.default.createDirectory(at: service, withIntermediateDirectories: false)
        let process = try await CloudServiceProcess.open(stateDirectory: service, pageSize: 32)
        var active: ServiceAppHarness?
        var publicationOwner: ServiceAppHarness?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let source = try ServiceAppHarness(root: root.appending(path: "source-owner"), wire: wire, clock: .system)
            let peer = try ServiceAppHarness(root: root.appending(path: "peer-owner"), wire: wire, clock: .system)
            defer { source.defaults.removePersistentDomain(forName: source.suite); peer.defaults.removePersistentDomain(forName: peer.suite) }
            for app in [source, peer] {
                try FileManager.default.createDirectory(at: app.root, withIntermediateDirectories: false)
                try input.bytes.write(to: app.root.appending(path: "collection.anki2"))
            }
            publicationOwner = source
            active = source; try await selectedBootstrapPublish(source)
            try #require(await source.owner.close())
            active = peer
            await peer.owner.start(); await peer.sync.prepareConnectionAuthority()
            try await peer.owner.prepareCloudConnectionPreview(); await peer.sync.waitForConnectionPreview()
            await peer.sync.confirmAdoption(); await peer.sync.waitForConnectionPreview(); await peer.sync.waitForReplacement()
            await peer.owner.waitForCloudActivation(); await peer.sync.prepareConnectionAuthority(); await peer.sync.waitForInputs()
            let inspector = SelectedCloudRestorationInspection()
            let original = try await inspector.originalControlWitness()
            try #require(await peer.owner.close())
            let fresh = try ServiceAppHarness(root: peer.root, wire: wire, defaults: peer.defaults, clock: .system)
            active = fresh
            let role = try await OriginalCloudRoleInspection.inspect(main: input.bytes, wal: nil)
            fresh.beforeBackend = {
                let restored = try await inspector.control(original: original)
                let owned = try await role.validateOwned()
                try #require(restored.generation == owned.generation && !owned.qChanged)
            }
            try await fresh.reopen()
            let control = try await SelectedCloudControlInspection().current()
            #expect(control.status == .proven && control.provenance == .retainedAdoption)
            #expect(control.activation == .proven && control.ownedBackup == .proven)
            #expect(fresh.sync.preview == nil && fresh.sync.recovery == nil)
            try #require(await fresh.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        } catch {
            if let backend = publicationOwner?.backends.first {
                selectedRestorationPublicationTrace(backend)
            }
            if let active { #expect(await active.owner.close()) }
            _ = await process.close()
            throw error
        }
    }
}
// Read only the original publication backend's bounded fixed command trace.
@MainActor private func selectedRestorationPublicationTrace(_ backend: ServiceAppBackend) {
    let commands = backend.diagnostics.filter {
        $0.contains(" command.begin ordinal=") || $0.contains(" command.end ordinal=")
            || $0.contains(" command.configureAutomatic ordinal=")
    }.suffix(32)
    for entry in commands { print("Selected restoration original publication \(entry)") }
}
#endif
