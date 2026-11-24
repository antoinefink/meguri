#if MEGURI_TEST_HOOKS
import AppModel
import CoreBridge
import DesignSystem
import Foundation
import Home
import Settings
import SwiftUI

extension TestHooks {
    /// The deck package import sheet (U162, `package/transfer-sheets` X2): its
    /// production view over fixed values (captures), the real sheet model over
    /// `FakeDeckPackageImportService` (UI tests), and the window's drop outline.
    enum DeckImportRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("collection/deck-import/options-small") { _ in .sheet { DeckImportSheetPreview.fixture("options-small") } }
            registry.register("collection/deck-import/options-large") { _ in .sheet { DeckImportSheetPreview.fixture("options-large") } }
            registry.register("collection/deck-import/options-changed") { _ in .sheet { DeckImportSheetPreview.fixture("options-changed") } }
            registry.register("collection/deck-import/importing") { _ in .sheet { DeckImportSheetPreview.fixture("importing") } }
            registry.register("collection/deck-import/confirm-losses") { _ in .sheet { DeckImportSheetPreview.fixture("confirm-losses") } }
            registry.register("collection/deck-import/confirm-sync") { _ in .sheet { DeckImportSheetPreview.fixture("confirm-sync") } }
            registry.register("collection/deck-import/confirm-sync-losses") { _ in
                .sheet { DeckImportSheetPreview.fixture("confirm-sync-losses") }
            }
            registry.register("collection/deck-import/confirm-unreachable") { _ in
                .sheet { DeckImportSheetPreview.fixture("confirm-unreachable") }
            }
            registry.register("collection/deck-import/result") { _ in .sheet { DeckImportSheetPreview.fixture("result") } }
            registry.register("collection/deck-import/result-log") { _ in .sheet { DeckImportSheetPreview.fixture("result-log") } }
            registry.register("collection/deck-import/result-undone") { _ in .sheet { DeckImportSheetPreview.fixture("result-undone") } }
            registry.register("collection/deck-import/refused-too-new") { _ in .sheet { DeckImportSheetPreview.fixture("refused-too-new") } }
            registry.register("collection/deck-import/refused-space") { _ in .sheet { DeckImportSheetPreview.fixture("refused-space") } }
            registry.register("collection/deck-import/refused-kind-conflict") { _ in
                .sheet { DeckImportSheetPreview.fixture("refused-kind-conflict") }
            }
            registry.register("collection/deck-import/refused-rows") { _ in .sheet { DeckImportSheetPreview.fixture("refused-rows") } }
            registry.register("collection/deck-import/refused-sync") { _ in .sheet { DeckImportSheetPreview.fixture("refused-sync") } }
            registry.register("collection/deck-import/imported-without-result") { _ in
                .sheet { DeckImportSheetPreview.fixture("imported-without-result") }
            }
            // The real sheet model, scripted: Import, a merge confirmation, a refusal.
            registry.register("collection/deck-import/live") { _ in .screen { LiveDeckImport(script: .plain) } }
            registry.register("collection/deck-import/live-confirm") { _ in .screen { LiveDeckImport(script: .confirm) } }
            registry.register("collection/deck-import/live-refused") { _ in .screen { LiveDeckImport(script: .refused) } }
            // K1: the outline a main window draws while a file is dragged over it.
            registry.register("collection/deck-import/drop-target") { _ in
                .screen { HomeDeckListPreview.typicalWithActivity().overlay { FileDropOutline() } }
            }
        }
    }

    /// A window that opens the production sheet over a scripted service.
    struct LiveDeckImport: View {
        enum Script { case plain, confirm, refused }
        let script: Script
        @State private var host = LiveDeckImportHost()
        @State private var model: DeckImportModel?

        var body: some View {
            List {
                Button("Import Sample Deck") { start() }
                    .accessibilityIdentifier("testhooks.deckImport.start")
                Text(verbatim: host.outcome)
                    .accessibilityIdentifier("testhooks.deckImport.outcome")
            }
            .sheet(isPresented: Binding(get: { host.shown }, set: { if !$0 { host.shown = false; model?.presentationChanged() } })) {
                if let model { DeckImportSheet(model: model) }
            }
            .onAppear { start() }
        }

        private func start() {
            guard model?.active != true else { return }
            let service = FakeDeckPackageImportService(Self.script(script))
            let value = DeckImportModel(host: host, service: service)
            host.service = service
            model = value
            value.begin(URL(fileURLWithPath: "/synthetic/Spanish Verbs.apkg"))
        }

        static func script(_ script: Script) -> FakeDeckPackageImportService.Script {
            let facts = DeckImportFacts(notes: 42, cards: 84, decks: ["Spanish Verbs"], notetypes: ["Basic (and reversed card)"],
                                        hasScheduling: true, mediaFiles: 12, mediaBytes: 3_400_000)
            var counts = DeckImportCounts(); counts.notesAdded = 40; counts.cardsAdded = 80
            let result = DeckImportResult(sequence: 1, generation: 1, undoStep: "Import", counts: counts,
                                          log: .init(foundNotes: 42, entries: [.new: 40, .duplicate: 2]))
            let preview = DeckImportPreview(emptiedFields: [.init(notetypeId: 5, notetypeName: "Basic (and reversed card)",
                                                                  position: 2, fieldName: "Example", notes: 18)])
            var value = FakeDeckPackageImportService.Script(inspect: .success(facts), applies: [])
            switch script {
            case .plain: value.applies = [.imported(result)]
            case .confirm:
                value.applies = [.confirmationRequired(.init(preview: preview, reasons: [], fakeToken: 1)),
                                 .confirmationRequired(.init(preview: preview, reasons: [], fakeToken: 2)), .imported(result)]
            case .refused: value.applies = [.refused(.stale), .imported(result)]
            }
            value.log = [.new: (1...40).map { .init(noteId: Int64($0), fields: ["word \($0)", "meaning"]) },
                         .duplicate: [.init(noteId: 41, fields: ["zorvel"]), .init(noteId: 42, fields: ["quandle"])]]
            return value
        }
    }

    @Observable @MainActor final class LiveDeckImportHost: DeckImportHost {
        var shown = false
        var outcome = ""
        @ObservationIgnored var service: FakeDeckPackageImportService?
        var deckImportSheetShown: Bool { shown }
        func presentDeckImport() { shown = true }
        func dismissDeckImport() {
            shown = false
            let applied = service?.applied ?? []
            outcome = "applies \(applied.count); tokens \(applied.map { $0.confirmation.map(String.init) ?? "none" }.joined(separator: ","))"
        }
        func deckImportNotetypeNames() async -> [Int64: String] { [:] }
        func deckImportCanUndo(step: UInt64) async -> Bool { true }
        func undoDeckImport(step: UInt64) async -> Bool { true }
        var deckImportShowInBrowse: (([Int64]) -> Void)? { nil }
        var deckImportSync: (any DeckImportMeguriSync)? { nil }
        func chooseAnotherImportFile() {}
        func deckImportMessage(_ message: ImportIntake.Message) { outcome = "message" }
        func deckImportFoundCollection(_ url: URL, release: (@MainActor () -> Void)?) { outcome = "collection" }
    }
}
#endif
