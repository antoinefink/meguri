@testable import AppModel
import CoreBridge
import Foundation
import Testing

@MainActor struct DeletionScopeTests {
    @Test func checkedReadDiscardsReplacementEvenWhenTheValueMatches() async {
        var generation: UInt64 = 41
        await #expect(throws: BridgeError.staleGeneration(planned: 41, current: 42)) {
            try await CollectionGenerationRead.read(lifecycle: { .ready(generation: generation) }) {
                generation = 42
                return "Same name and ID"
            }
        }
    }

    @Test(arguments: [CollectionLifecycle.noCollection, .opening, .ready(generation: 0)])
    func absentScopeDoesNotStartARead(_ lifecycle: CollectionLifecycle) async {
        var read = false
        await #expect(throws: (any Error).self) {
            try await CollectionGenerationRead.read(lifecycle: { lifecycle }) { read = true; return 1 }
        }
        #expect(!read)
    }

    @Test func losingReadinessDiscardsTheRead() async {
        var lifecycle = CollectionLifecycle.ready(generation: 41)
        await #expect(throws: (any Error).self) {
            try await CollectionGenerationRead.read(lifecycle: { lifecycle }) {
                lifecycle = .opening
                return 1
            }
        }
    }
}

// These models deliberately receive no collection events: delayed UI epochs
// must never authorize a write against the successor's matching names and IDs.
extension ReviewCollectionStartupTests {
    @Test(arguments: [true, false])
    func tagCallerUsesReadScopeAcrossHostReplacementAndRetry(_ browseOpen: Bool) async throws {
        try await withDeletionCollection { core, location in
            let names = try await CoreNotetypeService().notetypeList()
            let id = try #require(names.first?.id)
            let record = try await CoreNotetypeService().readNotetype(id: id)
            let note = try await CoreNoteService().addNote(.init(notetypeId: id,
                fields: record.fields.enumerated().map { $0.offset == 0 ? "Scope sentinel" : "" },
                tags: ["Scope::shared"], deckId: 1, guid: nil, updateAddDefaults: false))
            let backend = CoreBrowseBackend(), model = BrowseModel(backend: backend)
            model.generation = { 9001 } // Explicitly different from the process engine.
            if browseOpen { model.appear() }
            defer { model.disappear() }
            await model.refreshTags()
            let captured = model.tagDeletion.target(["Scope"])
            let before = try await core.waitAvailable(timeoutMilliseconds: 1_000)
            try await core.close()
            try await core.open(location, randomSeed: 43)
            let after = try await core.waitAvailable(timeoutMilliseconds: 1_000)
            #expect(after > before)
            model.tagDeletion.delete(captured)
            await model.tagDeletion.waitForCompletion()
            #expect(model.tagDeletion.failed == captured)
            // Retry retains the original scope too.
            model.tagDeletion.delete(captured)
            await model.tagDeletion.waitForCompletion()
            #expect(model.tagDeletion.failed == captured)
            #expect(try await CoreNoteService().readNote(id: note.id).tags == ["Scope::shared"])
            await model.refreshTags()
            #expect(!model.tagDeletion.canDelete(captured))
            model.tagDeletion.delete(model.tagDeletion.target(["Scope"]))
            await model.tagDeletion.waitForCompletion()
            #expect(model.tagDeletion.failed == nil && model.tagDeletion.announcement?.count == 1)
            #expect(try await CoreNoteService().readNote(id: note.id).tags.isEmpty)
            await #expect(throws: (any Error).self) { try await backend.deleteTags(text: "Scope") }
        }
    }

    @Test(arguments: [true, false])
    func noteTypeCallerCarriesOriginalScopeThroughTheLastAwaitToLocalAndMP7(_ mp7: Bool) async throws {
        try await withDeletionCollection { core, location in
            let types = CoreNotetypeService()
            let type = try await types.addNotetype(.init(name: "Scope shared", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{Back}}", browserFront: "", browserBack: "")], css: nil))
            let backend = CoreNoteTypesBackend(), model = NoteTypesModel(backend: backend)
            defer { model.invalidate() }
            let sync = ScopedRemovalSync()
            if mp7 { model.meguriSync = sync; model.service = { .meguriSync } }
            model.delete(type.id)
            await model.settle()
            let shown = try #require(model.deletionQuestion)
            let original = try await core.waitAvailable(timeoutMilliseconds: 1_000)
            #expect(shown.expectedGeneration == original)
            try await core.close()
            try await core.open(location, randomSeed: 43)
            let before = try await core.waitAvailable(timeoutMilliseconds: 1_000)
            #expect(before > original)
            model.confirmDeletion()
            await model.settle()
            #expect(model.deletionQuestion?.expectedGeneration == before)
            #expect(model.deletionQuestion?.name == shown.name && model.deletionQuestion?.counts == shown.counts)
            #expect(sync.request == nil && model.failure == nil)
            var after: UInt64 = 0
            model.deletionService = {
                do {
                    try await core.close()
                    try await core.open(location, randomSeed: 44)
                    after = try await core.waitAvailable(timeoutMilliseconds: 1_000)
                    if mp7 { try await sync.attach() }
                    return mp7 ? .meguriSync : .other
                } catch { Issue.record(error); return nil }
            }
            model.confirmDeletion()
            await model.settle()
            #expect(after > before)
            #expect(model.failure == .notSaved)
            #expect(try await types.readNotetype(id: type.id).name == "Scope shared")
            if mp7 {
                #expect(sync.request?.expectedGeneration == before)
                #expect(sync.error == .staleGeneration(planned: before, current: after))
                let round = try #require(sync.round)
                await #expect(throws: (any Error).self) {
                    try await round.removeNotetypeStructure(.init(notetypeId: type.id, confirmedNotes: 0, confirmedCards: 0),
                        reachable: false)
                }
            }
            await #expect(throws: (any Error).self) {
                try await backend.remove(.init(notetypeId: type.id, confirmedNotes: 0, confirmedCards: 0)) { _, _ in }
            }
        }
    }
}

@MainActor private func withDeletionCollection(
    _ body: (CoreCollectionService, CoreBridge.CollectionLocation) async throws -> Void
) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let directory = root.appending(path: "Collection"), backups = root.appending(path: "Backups")
    let transfers = root.appending(path: "transfers")
    let location = CoreBridge.CollectionLocation(collectionDirectory: directory, backupsDirectory: backups,
        appGroupSnapshot: root.appending(path: "widget"), manualTransferDirectory: transfers)
    var configuration = LaunchConfiguration()
    configuration.collectionLocation = AppModel.CollectionLocation(directory: directory, backupsDirectory: backups)
    let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), providerDirectory: transfers)
    await owner.start()
    await owner.startEmpty()
    try #require(owner.state == .ready)
    try #require(await owner.close())
    let core = CoreCollectionService()
    do {
        try await core.open(location, randomSeed: 42)
        try await body(core, location)
        try await core.close()
    } catch { try? await core.close(); throw error }
}

private actor DeletionCredentials: MeguriProviderCredentialStore {
    var value: MeguriProviderCredentials?
    func load(connection: MeguriProviderConnection) -> MeguriProviderCredentials? { value }
    func save(_ credentials: MeguriProviderCredentials) { value = credentials }
    func delete(connection: MeguriProviderConnection) { value = nil }
}

/// Only the UI's sync state/reachability is scripted. Removal reaches the real
/// MP7 facade and scoped FFI admission before any provider/network work.
@MainActor private final class ScopedRemovalSync: NoteTypesMeguriSync {
    let noteTypeSyncState: NoteTypeSyncState = .ready
    var owner: MeguriProviderConnectionOwner?
    var round: CoreMeguriRoundService?
    var request: NotetypeRemoval?
    var error: BridgeError?
    func attach() async throws {
        let core = try await CoreProviderService.attached(), store = DeletionCredentials()
        let owner = MeguriProviderConnectionOwner(service: core, store: store)
        try await owner.configure(endpoint: "https://deletion.synthetic.example/")
        let configured = try #require(try await core.configuredMeguri())
        await store.save(.init(connection: configured.connection, username: "synthetic", hostKey: "synthetic"))
        try await owner.load()
        self.owner = owner
        round = try await owner.roundService()
    }
    func noteTypeSyncReachable() async -> Bool { false }
    func saveNotetypeStructure(_ draft: NotetypeStructureDraft, reachable: Bool,
                              progress: @escaping @MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void) async throws -> NoteTypeSyncSaveResult {
        Issue.record("Removal must never call Save")
        return .notSaved
    }
    func removeNotetypeStructure(_ request: NotetypeRemoval, reachable: Bool,
                                progress: @escaping @MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void) async throws -> NoteTypeSyncSaveResult {
        self.request = request
        let round = try #require(round)
        do {
            _ = try await round.removeNotetypeStructure(request, reachable: reachable)
            Issue.record("Stale removal reached MP7 work")
            return .notSaved
        } catch {
            self.error = error as? BridgeError
            throw error
        }
    }
    func cancelNotetypeStructureSave() {}
}
