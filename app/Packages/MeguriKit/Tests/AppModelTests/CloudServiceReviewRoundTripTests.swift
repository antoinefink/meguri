#if os(macOS)
@testable import AppModel
@testable import CoreBridge
import Foundation
@testable import Instrumentation
import func MeguriCoreBindings.testPhysicalReviewRows
import struct MeguriCoreBindings.PhysicalReviewRows
import Testing

extension ReviewCollectionStartupTests {
    @Test func realServiceReviewAndRegisteredMediaSurviveReopenAndFreshAdoption() async throws {
        let fixture = try InitialCloudFixture.load()
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-full-app-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        // Retained outside every sequential CoreHost. Only final cleanup joins
        // the one real CLI Service process; reopen never replaces the Service.
        let serviceDirectory = root.appending(path: "service")
        try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory)
        var active: ServiceAppHarness?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let a = try ServiceAppHarness(root: root.appending(path: "A"), wire: wire)
            let b = try ServiceAppHarness(root: root.appending(path: "B"), wire: wire)
            active = a
            let probe = InitialCloudEvents()
            defer {
                ChangeFanout.shared.unsubscribe(probe); probe.finish()
                a.defaults.removePersistentDomain(forName: a.suite)
                b.defaults.removePersistentDomain(forName: b.suite)
            }
            try await wire.seed(fixture)
            try await ChangeFanout.shared.subscribe(probe)
            try await a.adopt()
            try await a.exchange()
            let receiptStart = await wire.receipts.count
            if a.owner.policy != .ok { await a.owner.enableFsrs() }
            func flatten(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] { nodes.flatMap { [$0] + flatten($0.children) } }
            let deck = try #require(flatten(a.owner.decks).first { !$0.filtered && $0.counts.new > 0 })
            await a.owner.startStudy(deckID: deck.id)
            let review = try #require(a.owner.review)
            try #require(review.phase == .question)
            review.reveal(); await review.settled()
            try #require(review.phase == .answer)
            let beforeAnswer = probe.events.last?.seq ?? 0
            review.rate(.easy); await review.settled()
            let answeredID = try #require(review.record.answers.first?.cardID)
            try #require(review.record.answers.count == 1)
            try await CoreCollectionService().flushForBackground()
            let committed = try await probe.next(after: beforeAnswer, stage: "full-app review durable Changes") {
                guard case .changes(let changes) = $0.kind else { return false }
                return changes.cards.contains { $0.id == answeredID } && changes.revlogAdded.count == 1
            }
            guard case .changes(let changes) = committed.kind else { throw ServiceWorkflowFailure.wire }
            let cardID = try #require(changes.cards.first { $0.id == answeredID }?.id)
            let revlogID = try #require(changes.revlogAdded.first)
            try #require(review.record.answers.first?.durable == true)
            await a.owner.finishReview()
            try #require(a.owner.previousRecord?.answers.allSatisfy(\.durable) == true)
            await a.sync.waitForInputs()
            // Synthetic media bytes prove installation; no renderer claim.
            let mediaBytes = try #require(Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aM1sAAAAASUVORK5CYII="))
            let mediaName = try await CoreMediaService().addMedia(name: "synthetic-review-roundtrip.png", bytes: Array(mediaBytes))
            try await CoreCollectionService().flushForBackground()
            await a.sync.waitForInputs()
            try await a.exchange()
            try await a.backend.waitForInstalledMedia(name: mediaName, bytes: mediaBytes, receiptsAfter: receiptStart)
            await a.sync.waitForInputs()
            let receipts = Array(await wire.receipts.dropFirst(receiptStart))
            // Require actual original Service Saved outcomes for BOTH changed
            // rows and both media layers, never a dirty flag or sendable count.
            try #require(receipts.contains { $0.matchesEntity(kind: 2, key: serviceIDKey(cardID)) }, "No actual Card Saved receipt")
            try #require(receipts.contains { $0.matchesEntity(kind: 3, key: serviceIDKey(revlogID)) }, "No actual Revlog Saved receipt")
            let mediaA = try await serviceMediaProof(generation: a.owner.generation)
            let trackedName = try #require(mediaA.names.first { $0.name == mediaName })
            try #require(mediaA.bytes[mediaName] == mediaBytes)
            try #require(receipts.contains { $0.matchesEntity(kind: 9, key: Data(trackedName.caseKey.utf8)) }, "No actual MediaName Saved receipt")
            let blobs = try receipts.compactMap { try $0.blobBytes() }
            try #require(blobs.contains(mediaBytes), "No actual MGBlob Saved receipt containing the registered bytes")
            let rowsA = try await servicePhysicalRows(cardID: cardID, revlogID: revlogID)
            try requirePhysicalContext(rowsA, generation: a.owner.generation, since: committed.seq)
            try #require(rowsA.card?.id == cardID && rowsA.revlog?.id == revlogID)
            try #require(await a.owner.close())
            // Same A files, fresh production backend and SDK, no other Host.
            try await a.reopen()
            let reopenedA = try await servicePhysicalRows(cardID: cardID, revlogID: revlogID)
            try requirePhysicalContext(reopenedA, generation: a.owner.generation, since: rowsA.seq)
            try #require(reopenedA.generation > rowsA.generation)
            try comparePhysicalRows(expected: rowsA, actual: reopenedA, crossDevice: false)
            let mediaAReopen = try await serviceMediaProof(generation: a.owner.generation)
            try #require(mediaAReopen.root == mediaA.root && mediaAReopen.names == mediaA.names && mediaAReopen.bytes == mediaA.bytes)
            try #require(await a.owner.close())
            // A is closed before B's absent collection enters the SAME app
            // adoption path against the SAME Service's actual current records.
            active = b
            try await b.adopt()
            try await b.exchange()
            try await b.backend.waitForInstalledMedia(name: mediaName, bytes: mediaBytes)
            await b.sync.waitForInputs()
            let rowsB = try await servicePhysicalRows(cardID: cardID, revlogID: revlogID)
            try requirePhysicalContext(rowsB, generation: b.owner.generation, since: reopenedA.seq)
            try #require(rowsB.generation > reopenedA.generation)
            try comparePhysicalRows(expected: rowsA, actual: rowsB, crossDevice: true)
            let mediaB = try await serviceMediaProof(generation: b.owner.generation)
            try #require(mediaB.root != mediaA.root && mediaB.names == mediaA.names && mediaB.bytes == mediaA.bytes)
            try #require(mediaB.bytes[mediaName] == mediaBytes)
            try #require(await b.owner.close())
            try await b.reopen()
            let reopenedB = try await servicePhysicalRows(cardID: cardID, revlogID: revlogID)
            try requirePhysicalContext(reopenedB, generation: b.owner.generation, since: rowsB.seq)
            try #require(reopenedB.generation > rowsB.generation)
            try comparePhysicalRows(expected: rowsB, actual: reopenedB, crossDevice: false)
            let mediaBReopen = try await serviceMediaProof(generation: b.owner.generation)
            try #require(mediaBReopen.root == mediaB.root && mediaBReopen.names == mediaB.names && mediaBReopen.bytes == mediaB.bytes)
            try #require(await b.owner.close())
            let exit = await process.close()
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
        } catch {
            await active?.cleanup()
            _ = await process.cancel()
            throw error // Preserve the first real app/prerequisite failure.
        }
    }
}

private nonisolated func servicePhysicalRows(cardID: Int64, revlogID: Int64) async throws -> MeguriCoreBindings.PhysicalReviewRows {
    // Test-only export called directly on MaintenanceLane. No normal bridge hook.
    try await MaintenanceLane.shared.call(BridgeCall("testPhysicalReviewRows")) {
        try MeguriCoreBindings.testPhysicalReviewRows(cardId: cardID, revlogId: revlogID)
    }
}
private func requirePhysicalContext(_ rows: MeguriCoreBindings.PhysicalReviewRows, generation: UInt64, since: UInt64) throws {
    try #require(rows.generation == generation && rows.seq >= since)
    let card = try #require(rows.card), revlog = try #require(rows.revlog)
    try #require(card.cells.count == 18 && revlog.cells.count == 9)
    try #require(card.cells[0] == .integer(value: card.id))
    try #require(revlog.cells[0] == .integer(value: revlog.id))
    try #require(revlog.cells[1] == .integer(value: card.id))
}
private func comparePhysicalRows(expected: MeguriCoreBindings.PhysicalReviewRows,
                                 actual: MeguriCoreBindings.PhysicalReviewRows, crossDevice: Bool) throws {
    let card = try #require(actual.card), revlog = try #require(actual.revlog)
    let expectedCard = try #require(expected.card), expectedRevlog = try #require(expected.revlog)
    var cardCells = card.cells, revlogCells = revlog.cells
    try #require(cardCells.count == 18 && expectedCard.cells.count == 18 && revlogCells.count == 9 && expectedRevlog.cells.count == 9)
    try #require(card.id == expectedCard.id && revlog.id == expectedRevlog.id)
    if crossDevice {
        // K11.3 permits only usn to differ. All other storage cells, including
        // modification time and the original CardData bytes, compare exactly.
        try #require(cardCells[5] == .integer(value: -1) && revlogCells[2] == .integer(value: -1))
        cardCells[5] = expectedCard.cells[5]; revlogCells[2] = expectedRevlog.cells[2]
    }
    try #require(cardCells == expectedCard.cells && revlogCells == expectedRevlog.cells)
}
nonisolated struct ServiceMediaProof: Sendable {
    let root: URL
    let names: [TrackedMediaName]
    let bytes: [String: Data]
}
nonisolated func serviceMediaProof(generation: UInt64) async throws -> ServiceMediaProof {
    let inventory = try await CoreMediaService().trackedNames()
    try #require(inventory.generation == generation)
    let names = inventory.names.sorted { $0.name < $1.name }
    var bytes: [String: Data] = [:]
    for name in names { bytes[name.name] = try Data(contentsOf: inventory.root.appendingPathComponent(name.name)) }
    return .init(root: inventory.root, names: names, bytes: bytes)
}
private nonisolated func serviceIDKey(_ id: Int64) -> Data {
    let bits = UInt64(bitPattern: id)
    return Data((0..<8).reversed().map { UInt8(truncatingIfNeeded: bits >> ($0 * 8)) })
}
private extension ServiceSavedReceipt {
    // K5.1 canonical prefix only: format=1, kind, canonical key. This labels
    // actual server receipts; row equality comes solely from the reader above.
    func matchesEntity(kind: UInt8, key: Data) -> Bool {
        guard record.recordType == "MGEntity" else { return false }
        let bytes = Array(record.envelope)
        guard bytes.starts(with: [8, 1, 16, kind, 26]) else { return false }
        var offset = 5, count: UInt64 = 0
        for shift in stride(from: 0, through: 63, by: 7) {
            guard offset < bytes.count else { return false }
            let byte = bytes[offset]; offset += 1
            guard shift < 63 || byte <= 1 else { return false }
            count |= UInt64(byte & 127) << shift
            if byte & 128 == 0 {
                guard count == key.count, count <= bytes.count - offset else { return false }
                return Data(bytes[offset..<(offset + Int(count))]) == key
            }
        }
        return false
    }
}
#endif
