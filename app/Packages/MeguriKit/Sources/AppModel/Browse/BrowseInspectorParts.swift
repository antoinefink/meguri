import CoreBridge
import Foundation
import Observation

/// `text/field-emptiness` E1: a field is empty when its text is only ASCII
/// white space and spacer tags (`<br>`, `<div>`, `</div>` and their
/// variants E1 lists). Browse's inspector leaves such fields behind its
/// "<e> Empty Fields" row (`browser/workspace` V12).
public nonisolated enum FieldEmptiness {
    public static func isEmpty(_ value: String) -> Bool {
        let bytes = Array(value.utf8)
        var index = 0
        func lower(_ byte: UInt8) -> UInt8 { (0x41...0x5A).contains(byte) ? byte | 0x20 : byte }
        func take(_ word: [UInt8]) -> Bool {
            guard index + word.count <= bytes.count else { return false }
            for (offset, letter) in word.enumerated() where lower(bytes[index + offset]) != letter { return false }
            index += word.count
            return true
        }
        while index < bytes.count {
            switch bytes[index] {
            case 0x09...0x0D, 0x20:
                index += 1
            case UInt8(ascii: "<"):
                index += 1
                if index < bytes.count, bytes[index] == UInt8(ascii: "/") { index += 1 }
                guard take(Array("br".utf8)) || take(Array("div".utf8)) else { return false }
                if index < bytes.count, bytes[index] == 0x20 { index += 1 }
                if index < bytes.count, bytes[index] == UInt8(ascii: "/") { index += 1 }
                guard index < bytes.count, bytes[index] == UInt8(ascii: ">") else { return false }
                index += 1
            default:
                return false
            }
        }
        return true
    }
}

/// The reads and the save Browse's inspector makes (U273). Production is
/// `CoreBrowseInspectorBackend`; tests give their own. Every read is pure.
@MainActor public protocol BrowseInspectorBackend: AnyObject {
    func note(_ id: Int64) async throws -> NoteRecord
    func notetype(_ id: Int64) async throws -> NotetypeRecord
    /// The note's cards by ordinal, then id.
    func cards(ofNote id: Int64) async throws -> [NoteCard]
    /// Every deck, for the Deck row and for naming a card's deck.
    func deckNames() async throws -> [DeckNameEntry]
    /// `editor/field-html` S2's patch: one undoable step, or nothing written.
    func save(_ patch: NoteFieldPatch) async throws -> NoteMutation
}

@MainActor public final class CoreBrowseInspectorBackend: BrowseInspectorBackend {
    private let notes = CoreNoteService()
    private let notetypes = CoreNotetypeService()
    private let decks = CoreDeckService()
    public init() {}
    public func note(_ id: Int64) async throws -> NoteRecord { try await notes.readNote(id: id) }
    public func notetype(_ id: Int64) async throws -> NotetypeRecord { try await notetypes.readNotetype(id: id) }
    public func cards(ofNote id: Int64) async throws -> [NoteCard] { try await notes.noteCards(noteId: id) }
    public func deckNames() async throws -> [DeckNameEntry] { try await decks.allDeckNames().decks }
    public func save(_ patch: NoteFieldPatch) async throws -> NoteMutation { try await notes.patchNote(patch) }
}

/// The field page Browse's inspector shows a note in: the round 5 editor's
/// runtime (`FieldEditorHost` in production), with what the inspector adds.
@MainActor public protocol BrowseFieldRuntime: AddNoteFieldRuntime {
    /// A field lost the keyboard focus, after its commit: V12 saves then.
    var onFieldBlur: (@MainActor () -> Void)? { get set }
    /// The page can take input.
    var editable: Bool { get }
    /// A held flush made the page refuse input until `release`.
    var held: Bool { get }
    /// V12: the fields the page leaves out (the draft's `hiddenFields`).
    func setHiddenFields(_ ords: Set<Int>) async throws
    /// N21.8's Edit Note: the field takes the keyboard focus.
    func focusField(_ ord: Int) async throws
}

/// The note Browse's inspector shows (V12): its fields in the editor's
/// draft and page, its cards, the shown card's deck.
@Observable @MainActor
public final class BrowseInspectorNote {
    public let noteID: Int64
    public let notetype: NotetypeRecord
    public let draft: NoteEditorDraft
    public internal(set) var runtime: (any BrowseFieldRuntime)?
    /// The note's cards by ordinal, then id.
    public internal(set) var cards: [NoteCard]
    /// The card the preview, the Deck row and Card Info show (V7, V12).
    public internal(set) var cardIndex: Int
    /// V12: the fields that were empty when the note was shown; they keep
    /// that place until another note is shown.
    public let emptyFields: [Int]
    /// The last save of this note failed (W9's lines); the text stays.
    public internal(set) var saveFailure: EditNoteFlow.SaveFailure?
    /// The page could not open this note (a note type the editor cannot show).
    public internal(set) var error: String?

    init(noteID: Int64, notetype: NotetypeRecord, draft: NoteEditorDraft, cards: [NoteCard], cardIndex: Int, emptyFields: [Int]) {
        self.noteID = noteID
        self.notetype = notetype
        self.draft = draft
        self.cards = cards
        self.cardIndex = cardIndex
        self.emptyFields = emptyFields
    }

    public var card: NoteCard? { cards.indices.contains(cardIndex) ? cards[cardIndex] : nil }
    /// V12's Cloze control: a cloze note type with a field the page shows.
    public var isCloze: Bool { notetype.kind == 1 && !notetype.clozeFields.isEmpty }
}

/// V7: the selection's summary in the inspector, from the commands' facts.
public nonisolated struct BrowseSelectionSummary: Equatable, Sendable {
    public let mode: BrowseMode
    /// The rows selected: "<n> Cards Selected" or "<n> Notes Selected".
    public let rows: Int
    /// Cards mode: the notes of the selected cards.
    public let notes: UInt64?
    /// The one home deck all selected cards share, by its full name.
    public let deck: String?
    /// The home decks when there are several.
    public let decks: UInt64?
    public let suspended: UInt64?
}
