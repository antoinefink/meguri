@testable import AppModel
import CoreBridge
import Foundation
import Synchronization
import Testing

/// The app around the sheet: its presentation, names, undo and Meguri Sync.
@MainActor private final class FakeImportHost: DeckImportHost {
    var deckImportSheetShown = false
    var presented = 0
    var names: [Int64: String] = [:]
    var undoable: UInt64?
    var undone: [UInt64] = []
    var shown: [[Int64]] = []
    var browse = true
    var sync: FakeImportSync?
    var anotherFile = 0
    var messages: [ImportIntake.Message] = []

    func presentDeckImport() { if !deckImportSheetShown { presented += 1 }; deckImportSheetShown = true }
    func dismissDeckImport() { deckImportSheetShown = false }
    func deckImportNotetypeNames() async -> [Int64: String] { names }
    func deckImportCanUndo(step: UInt64) async -> Bool { undoable == step }
    func undoDeckImport(step: UInt64) async -> Bool {
        guard undoable == step else { return false }
        undone.append(step); undoable = nil
        return true
    }
    var deckImportShowInBrowse: (([Int64]) -> Void)? { browse ? { [weak self] in self?.shown.append($0) } : nil }
    var deckImportSync: (any DeckImportMeguriSync)? { sync }
    func chooseAnotherImportFile() { anotherFile += 1 }
    func deckImportMessage(_ message: ImportIntake.Message) { messages.append(message) }
}

/// MP7.1's side: answers what the test set.
@MainActor private final class FakeImportSync: DeckImportMeguriSync {
    var deckImportSyncReady = true
    var reachable = true
    var answers: [MeguriDeckImportOutcome] = []
    private(set) var jobs: [(confirmation: UInt64?, reachable: Bool)] = []
    func deckImportSyncReachable() async -> Bool { reachable }
    func importDeckPackage(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation,
                           reachable: Bool, stage: @escaping @MainActor (NoteTypeSyncStage) -> Void) async throws
        -> MeguriDeckImportOutcome {
        jobs.append((confirmation.fakeToken, reachable))
        stage(.syncing); stage(.saving)
        return answers.isEmpty ? .notStarted(.notReady) : answers.removeFirst()
    }
    func cancelDeckImport() {}
}

/// A service whose stored options cannot be read (X2.6's `read_options` row).
nonisolated private final class UnreadableOptions: DeckPackageImportService {
    let base: FakeDeckPackageImportService
    init(_ base: FakeDeckPackageImportService) { self.base = base }
    func classify(name: String) async throws(BridgeError) -> PackageFileClass? { try await base.classify(name: name) }
    func classify(file: URL) async throws -> PackageFileClass { try await base.classify(file: file) }
    func storedOptions() async throws(BridgeError) -> DeckImportOptions { throw .unavailable(reason: "closed") }
    func inspect(file: URL) -> DeckImportTask<DeckImportSession> { base.inspect(file: file) }
    func apply(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation?,
               askStructure: Bool) -> DeckImportTask<DeckImportApplyOutcome> {
        base.apply(session, options: options, confirmation: confirmation, askStructure: askStructure)
    }
    func result(of session: DeckImportSession) async throws(BridgeError) -> DeckImportResult? { try await base.result(of: session) }
    func logPage(_ session: DeckImportSession, category: DeckImportLogCategory, offset: UInt64,
                 count: UInt32) async throws(BridgeError) -> [DeckImportLogEntry] {
        try await base.logPage(session, category: category, offset: offset, count: count)
    }
    func close(_ session: DeckImportSession) async { await base.close(session) }
}

/// A gate a scripted call waits on.
nonisolated private final class Gate: Sendable {
    private let value = Mutex(false)
    var open: Bool { value.withLock { $0 } }
    func release() { value.withLock { $0 = true } }
}

private let facts = DeckImportFacts(notes: 120, cards: 240, decks: ["Sample", "Sample::Part One"], notetypes: ["Basic"],
                                    hasScheduling: true, mediaFiles: 3, mediaBytes: 4_096)
private let file = URL(fileURLWithPath: "/synthetic/Sample.apkg")

private func result(sequence: UInt64 = 7, undo: String? = "Import", new: UInt64 = 118, duplicate: UInt64 = 2,
                    mediaPending: Bool = false) -> DeckImportResult {
    var counts = DeckImportCounts(); counts.notesAdded = new; counts.cardsAdded = new * 2
    return DeckImportResult(sequence: sequence, generation: 1, undoStep: undo, counts: counts,
                            log: .init(foundNotes: new + duplicate, entries: [.new: new, .duplicate: duplicate]),
                            mediaPending: mediaPending)
}

private func confirmation(_ token: UInt64, reasons: [DeckImportStructuralReason] = [], rows: Bool = true) -> DeckImportConfirmation {
    let preview = rows ? DeckImportPreview(
        emptiedFields: [.init(notetypeId: 5, notetypeName: "Basic", position: 1, fieldName: "Back", notes: 12)],
        removedNotetypes: [.init(notetypeId: 6, name: "Basic-copy", movedTo: 5, movedToName: nil, notes: 4)])
        : DeckImportPreview()
    return DeckImportConfirmation(preview: preview, reasons: reasons, fakeToken: token)
}

@MainActor private func model(_ script: FakeDeckPackageImportService.Script, host: FakeImportHost = FakeImportHost())
    -> (DeckImportModel, FakeDeckPackageImportService, FakeImportHost) {
    let service = FakeDeckPackageImportService(script)
    let model = DeckImportModel(host: host, service: service)
    model.progressDelay = .zero
    return (model, service, host)
}

@MainActor private func opened(_ script: FakeDeckPackageImportService.Script, host: FakeImportHost = FakeImportHost())
    async -> (DeckImportModel, FakeDeckPackageImportService, FakeImportHost) {
    let value = model(script, host: host)
    #expect(value.0.begin(file))
    await value.0.waitForWork()
    return value
}

@MainActor @Suite struct DeckImportModelTests {
    @Test func inspectShowsTheFactsAndTheStoredOptions() async throws {
        var script = FakeDeckPackageImportService.Script(inspect: .success(facts), applies: [])
        script.storedOptions = DeckImportOptions(withScheduling: true, withDeckConfigs: false, mergeNotetypes: true,
                                                 updateNotes: .always, updateNotetypes: .never)
        let (model, service, host) = await opened(script)
        #expect(model.step == .options)
        #expect(model.facts == facts && model.fileName == "Sample.apkg")
        #expect(model.options == script.storedOptions)
        #expect(host.deckImportSheetShown && host.presented == 1)
        #expect(service.openSessions == 1 && service.applied.isEmpty)
        // K4: a second file while this one shows is not taken.
        #expect(!model.begin(file))
        model.done()
        #expect(model.step == .idle && !host.deckImportSheetShown)
        try await Task.sleep(for: .milliseconds(20))
        #expect(service.openSessions == 0)
    }

    @Test func importSendsTheChosenOptionsAndShowsTheResult() async throws {
        let host = FakeImportHost(); host.undoable = 7
        let (model, service, _) = await opened(.init(inspect: .success(facts), applies: [.imported(result())]), host: host)
        model.options.withScheduling = true
        model.options.updateNotes = .never
        model.importPackage()
        await model.waitForWork()
        #expect(model.step == .result)
        let applied = try #require(service.applied.first)
        #expect(applied.options.withScheduling && applied.options.updateNotes == .never)
        #expect(applied.confirmation == nil && !applied.askStructure)
        #expect(model.result?.log[.new] == 118 && model.canUndo)
        // A second Import never runs from the result.
        model.importPackage()
        await model.waitForWork()
        #expect(service.applied.count == 1)
        model.undoImport()
        try await Task.sleep(for: .milliseconds(20))
        #expect(host.undone == [7] && model.undone && !model.canUndo)
    }

    @Test func noUndoWithoutAStepOrOnceItIsNotTheNewest() async {
        let host = FakeImportHost(); host.undoable = 99
        let (model, _, _) = await opened(.init(inspect: .success(facts), applies: [.imported(result(undo: nil))]), host: host)
        model.importPackage(); await model.waitForWork()
        #expect(model.step == .result && !model.canUndo)
        let (other, _, _) = await opened(.init(inspect: .success(facts), applies: [.imported(result(sequence: 8))]), host: host)
        other.importPackage(); await other.waitForWork()
        #expect(other.step == .result && !other.canUndo)
    }

    @Test func aConfirmationIsShownThenSentWithItsToken() async throws {
        let host = FakeImportHost(); host.names = [5: "Basic"]
        let (model, service, _) = await opened(.init(inspect: .success(facts),
            applies: [.confirmationRequired(confirmation(1)), .confirmationRequired(confirmation(2)), .imported(result())]), host: host)
        model.options.mergeNotetypes = true
        model.importPackage(); await model.waitForWork()
        #expect(model.step == .confirming && model.confirmation?.fakeToken == 1 && model.confirmationHasLosses)
        #expect(model.syncParagraph == nil)
        // X2.3: names given by id only are read from the collection.
        #expect(model.notetypeNames[5] == "Basic")
        // Cancel calls nothing.
        model.cancelConfirmation()
        #expect(model.step == .options && service.applied.count == 1)
        model.importPackage(); await model.waitForWork()
        #expect(model.confirmation?.fakeToken == 2)
        // Changing an option goes back to the options; the old token is never sent.
        model.options.updateNotetypes = .always
        #expect(model.step == .options && model.confirmation == nil)
        model.importPackage(); await model.waitForWork()
        #expect(model.step == .result)
        #expect(service.applied.map(\.confirmation) == [nil, nil, nil])
    }

    @Test func aRepeatedConfirmationReplacesTheShownOne() async throws {
        let (model, service, _) = await opened(.init(inspect: .success(facts),
            applies: [.confirmationRequired(confirmation(1)), .confirmationRequired(confirmation(2)), .imported(result())]))
        model.importPackage(); await model.waitForWork()
        model.confirm(); await model.waitForWork()
        #expect(model.step == .confirming && model.confirmation?.fakeToken == 2)
        model.confirm(); await model.waitForWork()
        #expect(model.step == .result)
        #expect(service.applied.map(\.confirmation) == [nil, 1, 2])
    }

    @Test func meguriSyncShowsTheParagraphAndRunsTheJob() async throws {
        let host = FakeImportHost(); let sync = FakeImportSync(); host.sync = sync
        sync.answers = [.imported(result(), upload: .uploaded(mediaComplete: true))]
        let (model, service, _) = await opened(.init(inspect: .success(facts),
            applies: [.confirmationRequired(confirmation(3, reasons: [.notetypesMerged], rows: false))]), host: host)
        model.importPackage(); await model.waitForWork()
        #expect(service.applied.first?.askStructure == true)
        #expect(model.step == .confirming && model.syncParagraph == .replaces && !model.confirmationHasLosses)
        model.confirm(); await model.waitForWork()
        #expect(sync.jobs.count == 1 && sync.jobs[0].confirmation == 3 && sync.jobs[0].reachable)
        #expect(model.step == .result && model.upload == .uploaded(mediaComplete: true))
        #expect(service.applied.count == 1)
    }

    @Test func unreachableMeguriSyncSaysSo() async {
        let host = FakeImportHost(); let sync = FakeImportSync(); host.sync = sync; sync.reachable = false
        sync.answers = [.imported(result(), upload: .pending(.unreachable))]
        let (model, _, _) = await opened(.init(inspect: .success(facts),
            applies: [.confirmationRequired(confirmation(3, reasons: [.sortFieldChanged]))]), host: host)
        model.importPackage(); await model.waitForWork()
        #expect(model.syncParagraph == .unreachable && model.confirmationHasLosses)
        model.confirm(); await model.waitForWork()
        #expect(sync.jobs.first?.reachable == false && model.step == .result && model.upload == .pending(.unreachable))
    }

    @Test func notReadyShowsTheConfirmationAgainThenImportsAlone() async throws {
        let host = FakeImportHost(); let sync = FakeImportSync(); host.sync = sync
        sync.answers = [.notStarted(.notReady)]
        let (model, service, _) = await opened(.init(inspect: .success(facts),
            applies: [.confirmationRequired(confirmation(4, reasons: [.notetypesMerged])), .imported(result())]), host: host)
        model.importPackage(); await model.waitForWork()
        model.confirm(); await model.waitForWork()
        // Rows: the confirmation again, without its paragraph.
        #expect(model.step == .confirming && model.syncParagraph == nil && model.confirmation?.fakeToken == 4)
        model.confirm(); await model.waitForWork()
        #expect(model.step == .result)
        let alone = try #require(service.applied.last)
        #expect(alone.confirmation == 4 && !alone.askStructure)
        // Read at acceptance: no longer ready, the job is not asked.
        sync.deckImportSyncReady = false
        let (other, otherService, _) = await opened(.init(inspect: .success(facts),
            applies: [.confirmationRequired(confirmation(5, reasons: [.notetypesMerged], rows: false)), .imported(result())]), host: host)
        other.importPackage(); await other.waitForWork()
        other.confirm(); await other.waitForWork()
        #expect(other.step == .result && sync.jobs.count == 1)
        #expect(otherService.applied.last?.confirmation == 5 && otherService.applied.last?.askStructure == false)
    }

    @Test func importedWithoutResultNeverOffersToImportAgain() async {
        let host = FakeImportHost(); let sync = FakeImportSync(); host.sync = sync
        sync.answers = [.importedWithoutResult(upload: nil)]
        let (model, service, _) = await opened(.init(inspect: .success(facts),
            applies: [.confirmationRequired(confirmation(6, reasons: [.notetypesMerged]))]), host: host)
        model.importPackage(); await model.waitForWork()
        model.confirm(); await model.waitForWork()
        #expect(model.step == .importedWithoutResult)
        model.importPackage(); model.retry(); model.confirm(); await model.waitForWork()
        #expect(service.applied.count == 1 && model.step == .importedWithoutResult)
        let (plain, plainService, _) = await opened(.init(inspect: .success(facts), applies: [.refused(.committedWithoutResult)]))
        plain.importPackage(); await plain.waitForWork()
        #expect(plain.step == .importedWithoutResult)
        plain.retry(); plain.importPackage(); await plain.waitForWork()
        #expect(plainService.applied.count == 1)
    }

    @Test func syncStopsAreRefusalsWithRetry() async {
        for (answer, stop) in [(MeguriDeckImportOutcome.fullSyncRequired, DeckImportModel.SyncStop.fullSyncRequired),
                               (.notStarted(.busy), .busy), (.notStarted(.uploadPending), .uploadPending),
                               (.unavailable, .unavailable)] {
            let host = FakeImportHost(); let sync = FakeImportSync(); host.sync = sync
            sync.answers = [answer]
            let (model, service, _) = await opened(.init(inspect: .success(facts),
                applies: [.confirmationRequired(confirmation(1, reasons: [.notetypesMerged])), .imported(result())]), host: host)
            model.importPackage(); await model.waitForWork()
            model.confirm(); await model.waitForWork()
            #expect(model.step == .refused && model.refusal == .sync(stop) && model.refusalSource == .apply)
            // Retry: the apply again on the same session.
            model.retry(); await model.waitForWork()
            #expect(service.openSessions == 1 && service.applied.count == 2)
        }
    }

    @Test(arguments: [DeckImportRefusal.stale, .busy(runningOperation: "Check Database"), .safetyBackupFailed,
                      .insufficientSpace(needed: 10, available: 2), .diskFull, .deckTooDeep(name: "A::B"),
                      .corrupt(detail: "x"), .collectionUnavailable(reason: "x"), .engine(.internal("x"))])
    func applyRefusalsAreShownAndKeepTheSession(_ refusal: DeckImportRefusal) async {
        let (model, service, _) = await opened(.init(inspect: .success(facts), applies: [.refused(refusal), .imported(result())]))
        model.importPackage(); await model.waitForWork()
        #expect(model.step == .refused && model.refusal == .engine(refusal) && model.refusalSource == .apply)
        #expect(service.openSessions == 1)
        model.retry(); await model.waitForWork()
        #expect(model.step == .result && service.applied.count == 2)
    }

    @Test func tooManyRowsWithLearningProgressImportsWithout() async throws {
        let (model, service, _) = await opened(.init(inspect: .success(facts),
            applies: [.refused(.tooLarge(what: .rows, found: 9, limit: 1)), .imported(result())]))
        model.options.withScheduling = true
        model.importPackage(); await model.waitForWork()
        #expect(model.refusal == .engine(.tooLarge(what: .rows, found: 9, limit: 1)))
        model.importWithoutLearningProgress(); await model.waitForWork()
        #expect(model.step == .result)
        #expect(service.applied.map(\.options.withScheduling) == [true, false])
    }

    @Test func aKindConflictImportsWithoutMerging() async {
        let (model, service, _) = await opened(.init(inspect: .success(facts),
            applies: [.refused(.notetypeKindConflict(incoming: 1, target: 2)), .imported(result())]))
        model.options.mergeNotetypes = true
        model.importPackage(); await model.waitForWork()
        model.importWithoutMerging(); await model.waitForWork()
        #expect(model.step == .result && service.applied.map(\.options.mergeNotetypes) == [true, false])
    }

    @Test func cancelledApplyGoesBackToTheOptions() async {
        let (model, _, _) = await opened(.init(inspect: .success(facts), applies: [.refused(.cancelled)]))
        model.importPackage(); await model.waitForWork()
        #expect(model.step == .options && model.cancelledNotice)
    }

    @Test func aClosedSessionOffersAnotherFile() async throws {
        let (model, service, host) = await opened(.init(inspect: .success(facts), applies: [.refused(.sessionClosed)]))
        model.importPackage(); await model.waitForWork()
        #expect(model.step == .refused && model.refusal == .engine(.sessionClosed))
        model.chooseAnotherFile()
        try await Task.sleep(for: .milliseconds(20))
        #expect(model.step == .idle && host.anotherFile == 1 && service.openSessions == 0)
    }

    @Test(arguments: [DeckImportRefusal.tooNew, .schemaUnsupported(.tooOld), .schemaUnsupported(.tooNew),
                      .schemaUnsupported(.other), .corrupt(detail: "x"), .tooLarge(what: .noteBytes, found: 2, limit: 1),
                      .unreadableFile])
    func inspectRefusalsShowTheSheetAndRetryInspectsAgain(_ refusal: DeckImportRefusal) async {
        let (model, service, host) = await opened(.init(inspect: .failure(refusal), applies: []))
        #expect(model.step == .refused && model.refusal == .engine(refusal) && model.refusalSource == .inspect)
        #expect(host.deckImportSheetShown && service.openSessions == 0)
        model.retry(); await model.waitForWork()
        #expect(model.step == .refused && model.refusalSource == .inspect)
    }

    @Test func storedOptionsThatCannotBeReadOpenNoSheet() async throws {
        let host = FakeImportHost()
        let base = FakeDeckPackageImportService(.init(inspect: .success(facts), applies: []))
        let model = DeckImportModel(host: host, service: UnreadableOptions(base))
        model.progressDelay = .seconds(5)
        #expect(model.begin(file))
        await model.waitForWork()
        try await Task.sleep(for: .milliseconds(20))
        #expect(model.step == .idle && !host.deckImportSheetShown && host.messages == [.collectionUnavailable])
        #expect(base.openSessions == 0)
    }

    @Test func aFastInspectShowsNoProgressAndASlowOneDoes() async throws {
        let gate = Gate()
        var script = FakeDeckPackageImportService.Script(inspect: .success(facts), applies: [])
        script.hold = { while !gate.open { try? await Task.sleep(for: .milliseconds(5)) } }
        let (model, _, host) = model(script)
        model.progressDelay = .milliseconds(30)
        #expect(model.begin(file))
        #expect(!host.deckImportSheetShown && !model.progressVisible)
        try await Task.sleep(for: .milliseconds(120))
        #expect(host.deckImportSheetShown && model.progressVisible && model.step == .inspecting)
        gate.release()
        await model.waitForWork()
        #expect(model.step == .options && !model.progressVisible)
    }

    @Test func cancellingTheInspectClosesTheSheet() async throws {
        let gate = Gate()
        var script = FakeDeckPackageImportService.Script(inspect: .success(facts), applies: [])
        script.hold = { while !gate.open && !Task.isCancelled { try? await Task.sleep(for: .milliseconds(5)) } }
        let (model, service, host) = model(script)
        #expect(model.begin(file))
        try await Task.sleep(for: .milliseconds(40))
        model.cancelWork()
        await model.waitForWork()
        #expect(model.step == .idle && !host.deckImportSheetShown && service.openSessions == 0)
    }

    @Test func aDismissedSheetEndsTheSession() async throws {
        let (model, service, host) = await opened(.init(inspect: .success(facts), applies: []))
        host.deckImportSheetShown = false
        model.presentationChanged()
        try await Task.sleep(for: .milliseconds(20))
        #expect(model.step == .idle && service.openSessions == 0)
    }

    @Test func theLogIsReadInPagesAndShowOpensBrowse() async throws {
        var script = FakeDeckPackageImportService.Script(inspect: .success(facts), applies: [.imported(result(new: 3, duplicate: 0))])
        script.log = [.new: (1...3).map { DeckImportLogEntry(noteId: Int64($0), fields: ["front \($0)"]) }]
        let (model, _, host) = await opened(script)
        model.logPageSize = 2
        model.importPackage(); await model.waitForWork()
        model.loadLog(.new)
        try await Task.sleep(for: .milliseconds(20))
        #expect(model.log[.new]?.count == 2 && model.moreLog(.new))
        model.loadLog(.new)
        try await Task.sleep(for: .milliseconds(20))
        #expect(model.log[.new]?.map(\.noteId) == [1, 2, 3] && !model.moreLog(.new))
        model.showInBrowse([1, 2, 3])
        #expect(host.shown == [[1, 2, 3]] && model.step == .idle)
    }

    @Test func mediaStillSettlingIsAResult() async {
        let (model, _, _) = await opened(.init(inspect: .success(facts), applies: [.imported(result(mediaPending: true))]))
        model.importPackage(); await model.waitForWork()
        #expect(model.step == .result && model.result?.mediaPending == true)
    }

    @Test func theCollectionClosingEndsTheSession() async throws {
        let (model, service, host) = await opened(.init(inspect: .success(facts), applies: []))
        await model.collectionClosing()
        try await Task.sleep(for: .milliseconds(20))
        #expect(model.step == .idle && !host.deckImportSheetShown && service.openSessions == 0)
    }
}
