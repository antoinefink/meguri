#if MEGURI_TEST_HOOKS
@_spi(MeguriTestHooks) import AppModel
import CoreBridge
import DesignSystem
import Home
import NoteTypes
import SwiftUI

extension TestHooks {
    /// The Note Types section's routes (U281; N22.1, N22.2, N22.5): the shell
    /// with Note Types selected over a made-up collection held in memory. No
    /// collection is opened and Save writes nothing.
    ///
    /// - `notetypes/fields/typical`: Vocabulary's Fields tab with no draft.
    /// - `notetypes/list/typical`: on iPad, the list of note types alone.
    /// - `notetypes/fields/edited`: a draft that deletes Tavrin Notes and adds
    ///   Origin: "Edited" in the list, "Save…" in the toolbar.
    /// - `notetypes/save/losses`: that draft's Save sheet with iCloud or no
    ///   service: the losses, then Cancel and a destructive Save.
    /// - `notetypes/save/meguri-sync`: the same with a ready Meguri Sync
    ///   binding: the sync paragraph and Save and Upload (U269 not built, so
    ///   the button is disabled).
    /// - `notetypes/first-run`: the shell with no collection on this device
    ///   (N18.3): Today's welcome, no deck, and no Note Types row; the route
    ///   asks for Note Types, and the shell keeps Today.
    enum NoteTypesRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.registerShell("notetypes/fields/typical") { _ in launch(.typical) }
            registry.registerShell("notetypes/list/typical") { _ in launch(.list) }
            registry.registerShell("notetypes/fields/edited") { _ in launch(.edited) }
            registry.registerShell("notetypes/save/losses") { _ in launch(.question(.other)) }
            registry.registerShell("notetypes/save/meguri-sync") { _ in launch(.question(.meguriSync)) }
            registry.registerShell("notetypes/first-run") { _ in firstRun() }
        }

        /// No collection: only Today (and, on iPad, Settings) is offered.
        @MainActor static func firstRun() -> ShellLaunch {
            var features = ShellSample.features(.typical)
            features.todayRoot = true
            #if os(macOS)
            features.nativeMacWorkspace = true
            features.availableSections = [.home, .noteTypes]
            #else
            features.availableSections = [.home, .noteTypes, .settings]
            #endif
            features.sectionShown = { ShellFeatures.sectionShown($0, hasCollection: false) }
            let placeholder = features.sectionRoot
            let welcome = HomeDeckListPreview.previewFixtures.first { $0.state == .empty }
            features.sectionRoot = { section, navigation in
                guard section == .home, let welcome else { return placeholder(section, navigation) }
                return AnyView(welcome.makeView().navigationTitle(Text(verbatim: "Today")))
            }
            features.sidebarDecks = { _ in
                AnyView(Text(verbatim: "No decks").foregroundStyle(.secondary).accessibilityIdentifier("home.sidebar.guidance"))
            }
            return ShellLaunch(features: features, probe: false) { navigation in
                navigation.select(.section(.noteTypes))
            }
        }

        enum State {
            /// The list alone: on iPad no note type is opened (on the Mac, as typical).
            case list
            case typical, edited
            case question(NoteTypeSaveService)
        }

        @MainActor static func launch(_ state: State) -> ShellLaunch {
            let backend = SampleNoteTypesBackend()
            let model = NoteTypesModel(backend: backend)
            if case .question(let service) = state { model.service = { service } }
            var features = ShellSample.features(.typical)
            features.todayRoot = true
            #if os(macOS)
            features.nativeMacWorkspace = true
            features.availableSections = [.home, .noteTypes]
            #else
            features.availableSections = [.home, .noteTypes, .settings]
            #endif
            let placeholder = features.sectionRoot
            features.sectionRoot = { section, navigation in
                section == .noteTypes ? AnyView(NoteTypesView(model: model)) : placeholder(section, navigation)
            }
            #if os(iOS)
            features.noteType = { id, _ in AnyView(NoteTypePage(model: model, id: id)) }
            #endif
            let sheets = features.sheet
            features.sheet = { route, navigation in
                route == .noteTypeSave ? AnyView(NoteTypeSaveSheet(model: model)) : sheets(route, navigation)
            }
            return ShellLaunch(features: features, probe: false) { navigation in
                model.presentQuestion = { navigation.present(.noteTypeSave); return true }
                model.dismissQuestion = { if navigation.sheet == .noteTypeSave { navigation.dismissSheet() } }
                navigation.select(.section(.noteTypes))
                Task { @MainActor in
                    await model.prepareForTestHooks(select: SampleNoteTypesBackend.vocabulary)
                    let id = SampleNoteTypesBackend.vocabulary
                    #if os(iOS)
                    if case .list = state {} else { navigation.push(.noteType(id)) }
                    #endif
                    switch state {
                    case .list, .typical:
                        model.selectedField[id] = 1
                    case .edited, .question:
                        model.stage(id) { draft in
                            draft.delete(11)
                            let added = draft.addField()
                            draft.rename(added, to: "Origin")
                        }
                        model.selectedField[id] = 1
                        if case .question = state {
                            if let draft = model.draft(id), let summary = try? await backend.summary(draft.structureDraft()) {
                                model.stageQuestion(id, summary: summary)
                            }
                        } else {
                            await model.settleForTestHooks()
                        }
                    }
                }
            }
        }
    }

    /// A made-up collection's note types, in memory (sample content only).
    @MainActor final class SampleNoteTypesBackend: NoteTypesBackend {
        static let vocabulary: Int64 = 1_700_000_100_009
        private static let names: [(String, UInt64)] = [
            ("Basic", 1204), ("Basic (and reversed card)", 86), ("Basic (type in the answer)", 12), ("Cloze", 318),
            ("Image Occlusion", 24), ("Glyphs", 2136), ("Phrases", 640), ("Verb Forms", 96), ("Vocabulary", 412),
        ]
        /// Vocabulary's fields: name and notes with content.
        private static let fields: [(String, UInt64)] = [
            ("Word", 412), ("Reading", 37), ("Meaning", 374), ("Part of Speech", 398), ("Example Sentence", 366),
            ("Translation", 371), ("Word Audio", 405), ("Sentence Audio", 212), ("Image", 188), ("Notes", 64),
            ("Related", 41), ("Tavrin Notes", 12), ("Variants", 9), ("Source", 0),
        ]
        private static let recognitionFront = "<div class=\"word\">{{Word}}</div>\n{{#Reading}}\n  <div class=\"reading\">{{Reading}}</div>\n{{/Reading}}\n{{Word Audio}}"
        private static let recognitionBack = "{{FrontSide}}\n<hr id=\"answer\">\n<div class=\"meaning\">{{Meaning}}</div>\n{{#Example Sentence}}\n  <div class=\"example\">{{Example Sentence}}</div>\n  <div class=\"translation\">{{hint:Translation}}</div>\n{{/Example Sentence}}\n{{#Image}}{{Image}}{{/Image}}"
        private static let productionFront = "<div class=\"meaning\">{{Meaning}}</div>\n<div class=\"pos\">{{Part of Speech}}</div>"
        private static let productionBack = "{{FrontSide}}\n<hr id=\"answer\">\n{{Word}}\n<div class=\"example\">{{Example Sentence}}</div>\n{{Translation}}\n{{#Notes}}{{Notes}}{{/Notes}}\n{{Variants}}"

        func list() async throws -> [NotetypeName] {
            Self.names.enumerated().map { NotetypeName(id: 1_700_000_100_001 + Int64($0.offset), name: $0.element.0) }
        }
        func noteCounts() async throws -> [Int64: UInt64] {
            Dictionary(uniqueKeysWithValues: Self.names.enumerated().map { (1_700_000_100_001 + Int64($0.offset), $0.element.1) })
        }
        func read(_ id: Int64) async throws -> NotetypeRecord {
            let name = Self.names[safe: Int(id - 1_700_000_100_001)]?.0 ?? "Basic"
            let fields = id == Self.vocabulary ? Self.fields.map(\.0) : ["Front", "Back"]
            let templates: [(String, String, String)] = id == Self.vocabulary
                ? [("Recognition", Self.recognitionFront, Self.recognitionBack), ("Production", Self.productionFront, Self.productionBack)]
                : [("Card 1", "{{Front}}", "{{FrontSide}}\n<hr id=answer>\n{{Back}}")]
            return NotetypeRecord(
                id: id, name: name, kind: 0, sortField: 0, css: ".card {\n  font-family: system-ui;\n}\n", base: [UInt8(truncatingIfNeeded: id)],
                fields: fields.enumerated().map { ord, field in
                    NotetypeField(ord: UInt32(ord), id: nil, name: field, sticky: false, rtl: false, fontName: "Arial", fontSize: 20,
                                  description: "", plainText: false, collapsed: field == "Reading", excludeFromSearch: false)
                },
                templates: templates.enumerated().map { ord, template in
                    NotetypeTemplate(ord: UInt32(ord), id: nil, name: template.0, front: template.1, back: template.2, qFormatBrowser: "",
                                     aFormatBrowser: "", targetDeckId: 0, browserFontName: "", browserFontSize: 0)
                })
        }
        func typeCounts(_ id: Int64) async throws -> NotetypeRemovalCounts {
            id == Self.vocabulary ? NotetypeRemovalCounts(notes: 412, cards: 786) : NotetypeRemovalCounts(notes: 0, cards: 0)
        }
        func fieldContentCounts(_ id: Int64) async throws -> NotetypeFieldContentCounts {
            id == Self.vocabulary ? NotetypeFieldContentCounts(notes: 412, fields: Self.fields.map(\.1))
                : NotetypeFieldContentCounts(notes: 0, fields: [0, 0])
        }
        /// D2 for a draft of Vocabulary's fields: removals with their counts, additions, a move, a new sort field.
        func summary(_ draft: NotetypeStructureDraft) async throws -> NotetypeDraftSummary {
            let kept = draft.fields.compactMap(\.origin)
            var changes: [NotetypeDraftChange] = draft.fields.filter { $0.origin == nil }.map { .fieldAdded(name: $0.name) }
            for (ord, field) in Self.fields.enumerated() where !kept.contains(UInt32(ord)) {
                changes.append(.fieldRemoved(ord: UInt32(ord), name: field.0, notesWithContent: field.1))
            }
            if kept != kept.sorted() { changes.append(.fieldsReordered) }
            let removed = changes.contains { if case .fieldRemoved = $0 { true } else { false } }
            let loses = changes.contains { if case .fieldRemoved(_, _, let notes) = $0 { notes > 0 } else { false } }
            return NotetypeDraftSummary(reasons: changes.isEmpty ? [] : [.fieldsChanged], changes: changes, losesContent: loses && removed)
        }
        func save(_ draft: NotetypeStructureDraft, progress: @escaping @MainActor (UInt64, UInt64?) -> Void) async throws {
            throw BridgeError.unavailable(reason: "sample collection")
        }
        func checkFieldName(_ typed: String, fields: [String], renaming: UInt32?) async throws -> NotetypeFieldNameCheck {
            let name = typed.replacingOccurrences(of: "\"", with: "").trimmingCharacters(in: .whitespaces)
            return name.isEmpty ? .ignored : .accepted(name)
        }
        func fieldReferences(_ texts: [String]) async throws -> [[String]] {
            texts.map { text in
                text.matches(of: /\{\{[#^\/]?([^}]*)\}\}/).compactMap { match -> String? in
                    let key = String(match.1).split(separator: ":").last.map(String.init) ?? ""
                    return key.trimmingCharacters(in: .whitespaces)
                }
            }
        }
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
#endif
