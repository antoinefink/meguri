import AppModel
import CoreBridge
import Foundation
#if os(iOS)
import UIKit
#endif

/// The deck package import sheet's words (`package/transfer-sheets` X2,
/// `package/transfer-strings`): S2's labels by their catalog keys, the rest
/// Meguri's own (S4).
enum DeckImportText {
    static var device: String {
        #if os(macOS)
        "Mac"
        #else
        UIDevice.current.userInterfaceIdiom == .pad ? "iPad" : "iPhone"
        #endif
    }

    static func count(_ value: UInt64, _ one: String, _ other: String) -> String {
        "\(value.formatted()) \(value == 1 ? one : other)"
    }

    static func bytes(_ value: UInt64) -> String { Int64(clamping: value).formatted(.byteCount(style: .file)) }

    /// A deck's full name as a path (N20.5).
    static func deckName(_ name: String) -> String { name.replacingOccurrences(of: "::", with: " › ") }

    /// A short list: the first names, then how many more.
    static func list(_ names: [String], shown: Int = 3) -> String {
        guard names.count > shown else { return names.formatted(.list(type: .and)) }
        let more = names.count - shown
        return names.prefix(shown).joined(separator: ", ") + ", and \(more.formatted()) more"
    }

    // MARK: Titles

    static func importTitle(_ file: String) -> String { "Import “\(file)”" }
    static func confirmTitle(_ file: String) -> String { "Import “\(file)”?" }
    static func importedTitle(_ file: String) -> String { "Imported “\(file)”" }
    static func readingTitle(_ file: String) -> String { "Reading “\(file)”…" }

    static func summary(_ facts: DeckImportFacts) -> String {
        [count(facts.notes, "note", "notes"), count(facts.cards, "card", "cards")].joined(separator: " · ")
    }

    // MARK: Facts (X2.1)

    static func media(_ facts: DeckImportFacts) -> String {
        facts.mediaFiles == 0 ? "None" : "\(count(facts.mediaFiles, "file", "files")) (\(bytes(facts.mediaBytes)))"
    }

    /// I4's repairs, in one sentence; nil when nothing was repaired.
    static func repairs(_ repairs: DeckImportRepairs) -> String? {
        var parts: [String] = []
        if repairs.noteIds > 0 { parts.append(count(repairs.noteIds, "note ID", "note IDs")) }
        if repairs.cardIds > 0 { parts.append(count(repairs.cardIds, "card ID", "card IDs")) }
        if repairs.revlogIds > 0 { parts.append(count(repairs.revlogIds, "review ID", "review IDs")) }
        if repairs.fieldCounts > 0 { parts.append(count(repairs.fieldCounts, "note’s fields", "notes’ fields")) }
        var sentences: [String] = []
        if !parts.isEmpty { sentences.append("Meguri repaired \(parts.formatted(.list(type: .and))) in the package.") }
        if repairs.schedulerConverted { sentences.append("Its learning progress was converted from the old scheduler.") }
        return sentences.isEmpty ? nil : sentences.joined(separator: " ")
    }

    // MARK: Options (X2.2; help is Meguri's)

    static func schedulingHelp(_ facts: DeckImportFacts?) -> String {
        facts?.hasScheduling == false
            ? "This package has no learning progress."
            : "Review history and due dates. When off, its cards start as new."
    }
    static func presetsHelp(_ facts: DeckImportFacts?) -> String {
        facts?.hasPresets == false ? "This package has no deck presets." : "The deck options presets the package’s decks use."
    }
    static let mergeHelp = "When a note type of the package has the same origin as one here, combine them instead of adding a copy."
    static let updatesFooter = "When a note or note type is already in your collection. “If newer” replaces it only when the package’s version was changed more recently."

    static let cancelled = "Import cancelled. Your collection is unchanged."
    static func otherFiles(_ file: String) -> String { "One file is imported at a time: “\(file)”." }

    // MARK: Progress (X2.4)

    static func progress(_ stage: DeckImportModel.ProgressStage?, file: String) -> String {
        switch stage {
        case .reading?, nil: readingTitle(file)
        case .planning?: "Preparing the Import…"
        case .media?: "Adding Media Files…"
        case .backup?: "Backing Up the Collection…"
        case .committing?: "Importing…"
        case .syncing?: "Syncing with Meguri Sync…"
        case .uploading?: "Uploading to Meguri Sync…"
        }
    }
    static let finishing = "Finishing. This step can’t be cancelled."

    // MARK: Confirmation (X2.3)

    static func emptied(_ row: DeckImportEmptiedField) -> String {
        "“\(row.fieldName)” of “\(row.notetypeName)”: its content in \(count(row.notes, "note", "notes")) is deleted"
    }
    static func removed(_ row: DeckImportRemovedNotetype, names: [Int64: String]) -> String {
        let target = row.movedToName ?? names[row.movedTo] ?? "another note type"
        let notes = count(row.notes, "note moves", "notes move")
        return "“\(row.name)” is removed; its \(notes) to “\(target)”"
    }
    static func dropped(_ row: DeckImportDroppedValues, names: [Int64: String]) -> String {
        let type = row.notetypeName ?? names[row.notetypeId] ?? "A note type"
        let holds = row.notes == 1 ? "note holds" : "notes hold"
        return "“\(type)”: \(row.notes.formatted()) \(holds) more values than it has fields, and the extra values are deleted"
    }
    static var backupLine: String { "A backup of the collection is kept on this \(device)." }
    static let syncReplaces = "Meguri Sync will be replaced."
    static var syncReplacesBody: String {
        "This import changes the structure of your note types, so it cannot be merged. Meguri syncs this \(device) first, imports, then uploads this collection in place of the one on Meguri Sync."
    }
    static let syncUnreachable = "Meguri Sync cannot be reached."
    static var syncUnreachableBody: String {
        "Meguri imports on this \(device) now and asks you to upload this collection when the server can be reached."
    }
    static let syncOtherDevices = "Sync your other devices before you continue. Changes on them that have not synced will be lost."

    // MARK: Result (X2.5)

    static func found(_ value: UInt64) -> String { "\(count(value, "note", "notes")) in the file" }
    static func added(_ value: UInt64) -> String { "\(count(value, "new note", "new notes")) added" }
    static func duplicate(_ value: UInt64) -> String { "\(count(value, "note was", "notes were")) already in your collection" }
    static func updated(_ value: UInt64) -> String { "\(count(value, "note", "notes")) used to update existing notes" }
    static func notImported(_ value: UInt64) -> String { "\(count(value, "note", "notes")) could not be imported" }
    static func defaultDeck(_ value: UInt64) -> String { "\(count(value, "note", "notes")) placed in the default deck" }
    static func mediaSkipped(_ value: UInt64) -> String { "\(count(value, "media file was", "media files were")) not added" }
    static let mediaPending = "The notes were imported. Some media files are still being put in place; this finishes on its own."
    static let fieldsOmitted = "Some notes are listed without their text."
    static let textNotShown = "Text not shown here"
    static let uploadPending = "Meguri Sync doesn’t have this import yet. Upload it from the sync status."
    static let undone = "The import was undone. Your collection is as it was before."

    static func reason(_ category: DeckImportLogCategory) -> String {
        switch category {
        case .new: "Added"
        case .updated: "Updated"
        case .duplicate: "Skipped: already in your collection"
        case .conflicting: "Skipped: its ID belongs to a note of another note type"
        case .missingNotetype: "Skipped: its note type is not in the package"
        case .missingDeck: "Its cards went to the default deck"
        case .firstFieldMatch: "Matched an existing note by its first field"
        case .emptyFirstField: "Skipped: its first field is empty"
        }
    }

    // MARK: Refusals (X2.6)

    static let unchanged = "Your collection is unchanged."

    static func refusal(_ refusal: DeckImportModel.Refusal, source: DeckImportModel.RefusalSource?, file: String,
                        withScheduling: Bool) -> String {
        switch refusal {
        case .sync(let stop):
            switch stop {
            case .busy: return "Meguri Sync is busy. Try again in a moment."
            case .uploadPending: return "An earlier change is waiting to upload to Meguri Sync. Upload it from the sync status, then import again."
            case .fullSyncRequired: return "Meguri Sync needs you to choose which collection to keep first. The sync status shows the choice."
            case .roundStopped: return "Meguri Sync couldn’t sync this \(device) first, so nothing was imported. The sync status says why."
            case .unavailable: return "Meguri Sync couldn’t run this import."
            }
        case .engine(let value):
            switch value {
            case .unsupportedFile, .collectionPackage: return "“\(file)” isn’t a deck package."
            case .tooNew: return "This package was made by a newer version of Anki or Meguri. Update Meguri, or export the deck again for older versions."
            case .schemaUnsupported(.tooOld): return "This package’s collection is too old for Meguri to read."
            case .schemaUnsupported(.tooNew): return "This package’s collection is newer than this version of Meguri can read."
            case .schemaUnsupported(.other): return "This package uses the old scheduler, which Meguri can’t import. Export it again from a current version of Anki."
            case .corrupt: return "“\(file)” is damaged or isn’t a deck package."
            case .tooLarge(.rows, _, _) where source == .apply && withScheduling:
                return "With its learning progress, this package has too many notes, cards and reviews together to import. It can be imported without its learning progress."
            case .tooLarge(let what, _, _): return "This package is too large for Meguri to import: \(tooLarge(what))."
            case .notetypeKindConflict: return "A cloze note type and a standard note type can’t be merged. The package can be imported with Merge Note Types off."
            case .deckTooDeep(let name): return "The deck “\(deckName(name))” is nested too deeply to add."
            case .insufficientSpace(let needed, let available):
                return "This \(device) needs \(bytes(needed > available ? needed - available : needed)) more free space to import this package."
            case .diskFull, .io: return "This \(device) couldn’t write the files the import needs."
            case .stale: return "The collection kept changing while the import was prepared."
            case .busy(let operation):
                return operation.isEmpty ? "Another long task is running. Try again once it ends."
                    : "Another long task is running (\(operation)). Try again once it ends."
            case .safetyBackupFailed: return "The backup made before this import couldn’t be written, so nothing was imported."
            case .collectionUnavailable, .staleGeneration: return "The collection isn’t available right now."
            case .sessionClosed: return "Choose the file again to import it."
            case .unreadableFile: return "Meguri couldn’t read “\(file)”. It may have moved, or its provider may not have it on this \(device)."
            case .cancelled: return cancelled
            case .engine: return "The import couldn’t finish."
            case .committedWithoutResult: return importedWithoutResult
            }
        }
    }

    static let importedWithoutResult = "The notes were imported, but the result can’t be shown."

    static func tooLarge(_ what: DeckImportLimit) -> String {
        switch what {
        case .rows: "it has too many notes and cards"
        case .noteBytes: "a note’s text is too long"
        case .cardDataBytes: "a card’s data is too large"
        case .tagUses: "it has too many tags"
        case .guidBytes: "a note’s ID is too long"
        case .structureRows, .notetypeParts, .structureBytes: "its decks, presets or note types are too many or too large"
        case .other: "it is over one of Meguri’s limits"
        }
    }
}
