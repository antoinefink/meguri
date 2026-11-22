import CoreBridge
import Foundation

/// One row of the Fields tab (`notetypes/workspace` W3): a draft field, or a
/// base field marked deleted, which stays in the list crossed out until Save.
public nonisolated struct NoteTypeFieldRow: Identifiable, Hashable, Sendable {
    /// Stable for the draft's life: a base field's ordinal, or a negative
    /// number for a field the draft adds.
    public let id: Int
    public var field: StructureDraftField
    /// The base field's name; nil for a field the draft adds.
    public let baseName: String?
    /// The base field cannot be deleted (S1.2 item 3: Cloze and Image Occlusion fields).
    public let protected: Bool
    public var deleted: Bool

    public var name: String { field.name }
    public var origin: UInt32? { field.origin }
}

/// One row of the Cards tab's card types list (W5): a draft card type, or
/// a base one marked deleted, crossed out until Save. Its texts are kept
/// apart (`NoteTypeDraft.texts(_:)`): typing is the text view's own undo,
/// not a draft step (W13).
public nonisolated struct NoteTypeCardTypeRow: Identifiable, Hashable, Sendable {
    /// Stable for the draft's life: a base card type's ordinal, or a
    /// negative number for one the draft adds.
    public let id: Int
    /// The base ordinal; nil for a card type the draft adds.
    public let origin: UInt32?
    public var name: String
    public var deleted: Bool
}

/// One card type's texts in the draft: what the template editor edits, and
/// what the save carries unchanged (browser appearance, A12).
public nonisolated struct NoteTypeCardTexts: Hashable, Sendable {
    public var front: String
    public var back: String
    public var browserFront: String
    public var browserBack: String
    public var targetDeckId: Int64
    public var browserFontName: String
    public var browserFontSize: UInt32
}

/// The template editor's three parts (W6, ⌘1 to ⌘3).
public nonisolated enum NoteTypeTemplatePart: String, Sendable, Hashable, CaseIterable {
    case front, back, styling
}

/// The draft changes the editor's own undo steps back through (W13), by
/// the name Edit › Undo shows ("Undo Delete Field").
public nonisolated enum NoteTypeDraftStep: String, Sendable, Hashable, CaseIterable {
    case deleteField = "Delete Field"
    /// Restore in a crossed-out row's context menu (W3).
    case restoreField = "Restore Field"
    case addField = "Add Field"
    case moveField = "Move Field"
    case renameField = "Rename Field"
    case changeSortField = "Change Sort Field"
    case changeOption = "Change Option"
    case addCardType = "Add Card Type"
    case deleteCardType = "Delete Card Type"
    /// Restore in a crossed-out card type's context menu (W5).
    case restoreCardType = "Restore Card Type"
    case moveCardType = "Move Card Type"
    case renameCardType = "Rename Card Type"
}

/// Why Add, Duplicate or Delete is disabled in the card types list (W5),
/// shown as the control's help text.
public nonisolated enum NoteTypeCardTypeBlock: Sendable, Hashable {
    /// Delete: the last card type that is not crossed out.
    case lastCardType
    /// A cloze or Image Occlusion note type has one card type.
    case cloze
}

/// Why Delete is disabled for a row (W3), shown as its help text.
public nonisolated enum NoteTypeFieldDeleteBlock: Sendable, Hashable {
    /// The last field that is not crossed out.
    case lastField
    /// A field the note type's kind needs (S1.2 item 3).
    case protected
}

/// One note type's draft (`notetypes/draft-save` D1): every unsaved change
/// to its fields, field options, sort field, card types, texts and styling,
/// with the base it was made from (its token goes to the save) and the
/// editor's own undo steps (W13). A value type: the model keeps one per
/// note type.
public nonisolated struct NoteTypeDraft: Sendable, Hashable {
    private struct State: Sendable, Hashable {
        var rows: [NoteTypeFieldRow]
        var sortRow: Int
        var cards: [NoteTypeCardTypeRow]
    }
    private struct Entry: Sendable, Hashable {
        let step: NoteTypeDraftStep
        let state: State
    }

    public let base: NotetypeRecord
    private var state: State
    private var undoStack: [Entry] = []
    private var redoStack: [Entry] = []
    private var nextNewID = -1
    private var nextNewCardID = -1
    /// Each card type's texts by row id, crossed-out and undone rows' too,
    /// so undoing a card type step never takes back what was typed.
    private var cardTexts: [Int: NoteTypeCardTexts]
    public var css: String
    public var name: String

    public init(_ base: NotetypeRecord) {
        self.base = base
        let rows = base.fields.map {
            NoteTypeFieldRow(id: Int($0.ord), field: StructureDraftField($0), baseName: $0.name, protected: $0.preventDeletion, deleted: false)
        }
        let cards = base.templates.map { NoteTypeCardTypeRow(id: Int($0.ord), origin: $0.ord, name: $0.name, deleted: false) }
        state = State(rows: rows, sortRow: Int(base.sortField), cards: cards)
        cardTexts = Dictionary(uniqueKeysWithValues: base.templates.map { template in
            (Int(template.ord), NoteTypeCardTexts(front: template.front, back: template.back, browserFront: template.qFormatBrowser,
                                                  browserBack: template.aFormatBrowser, targetDeckId: template.targetDeckId,
                                                  browserFontName: template.browserFontName, browserFontSize: template.browserFontSize))
        })
        css = base.css
        name = base.name
    }

    /// Cloze and Image Occlusion note types (stored kind 1) have one card type.
    public var isCloze: Bool { base.kind == 1 }

    /// Every card type row in draft order, crossed-out ones included (W5).
    public var cardRows: [NoteTypeCardTypeRow] { state.cards }
    public func cardRow(_ id: Int) -> NoteTypeCardTypeRow? { state.cards.first { $0.id == id } }
    /// The card types that stay after Save.
    public var keptCardRows: [NoteTypeCardTypeRow] { state.cards.filter { !$0.deleted } }
    public func texts(_ card: Int) -> NoteTypeCardTexts? { cardTexts[card] }

    /// A part's text: a card type's front or back, or the shared styling.
    public func text(_ card: Int, _ part: NoteTypeTemplatePart) -> String? {
        switch part {
        case .front: cardTexts[card]?.front
        case .back: cardTexts[card]?.back
        case .styling: css
        }
    }

    /// The kept card types as the save takes them (S1.1), in draft order.
    public var templates: [StructureDraftTemplate] {
        keptCardRows.compactMap { row in
            guard let texts = cardTexts[row.id] else { return nil }
            return StructureDraftTemplate(origin: row.origin, name: row.name, front: texts.front, back: texts.back,
                                          browserFront: texts.browserFront, browserBack: texts.browserBack, targetDeckId: texts.targetDeckId,
                                          browserFontName: texts.browserFontName, browserFontSize: texts.browserFontSize)
        }
    }

    // MARK: Reading

    /// Every row in draft order, crossed-out ones included.
    public var rows: [NoteTypeFieldRow] { state.rows }
    public func row(_ id: Int) -> NoteTypeFieldRow? { state.rows.first { $0.id == id } }
    /// The row whose radio is on in the Sort column.
    public var sortRow: Int { state.sortRow }
    /// The rows that stay after Save.
    public var keptRows: [NoteTypeFieldRow] { state.rows.filter { !$0.deleted } }

    /// The draft differs from its base (the list's Edited, Revert, Save).
    public var isEdited: Bool {
        keptRows.map(\.field) != base.fields.map(StructureDraftField.init) || effectiveSortPosition != base.sortField
            || !fieldsOnly || name != base.name
    }

    /// The changes the save classifies as structural (S2): a field or card
    /// type added, removed or moved, or another sort field. Renames, options
    /// and texts are not.
    public var changesStructure: Bool {
        let kept = keptRows.map(\.origin)
        return kept != base.fields.map { Optional($0.ord) } || effectiveSortPosition != base.sortField
            || keptCardRows.map(\.origin) != base.templates.map { Optional($0.ord) }
    }

    /// The draft leaves the templates and styling as they are: the save is
    /// the field editor's, unchecked (`notetypes/validation` V1).
    public var fieldsOnly: Bool {
        templates == base.templates.map(StructureDraftTemplate.init) && css == base.css
    }

    public func deleteBlock(_ id: Int) -> NoteTypeFieldDeleteBlock? {
        guard let row = row(id), !row.deleted else { return nil }
        if row.protected { return .protected }
        if keptRows.count <= 1 { return .lastField }
        return nil
    }

    /// Restore is refused while another field that stays has this name
    /// (under case folding): two fields would share it.
    public func canRestore(_ id: Int) -> Bool {
        guard let row = row(id), row.deleted else { return false }
        // Unicode case folding, as the core compares names (`ſc` and `sc` collide).
        func folded(_ name: String) -> String { name.folding(options: [.caseInsensitive], locale: nil) }
        let name = folded(row.name)
        return !keptRows.contains { folded($0.name) == name }
    }

    /// The step ⌘Z undoes and Edit › Undo names; nil when there is none.
    public var undoStep: NoteTypeDraftStep? { undoStack.last?.step }
    public var redoStep: NoteTypeDraftStep? { redoStack.last?.step }

    /// What the save and the reads of D2 take (S1.1). `confirmed` is what
    /// the Save sheet showed (D5 step 5), empty when no sheet showed.
    public func structureDraft(confirmed: NotetypeConfirmedLosses = .init()) -> NotetypeStructureDraft {
        let kept = keptRows
        let sort = kept.firstIndex { $0.id == state.sortRow }.map(UInt32.init)
        return NotetypeStructureDraft(
            notetypeId: base.id, base: base.base, name: name, css: css, fields: kept.map(\.field), templates: templates,
            // A crossed-out sort field: the base's rule (S3 step 4) picks the sort field.
            sortField: sort, skipChecks: fieldsOnly, confirmed: confirmed)
    }

    private var effectiveSortPosition: UInt32 {
        keptRows.firstIndex { $0.id == state.sortRow }.map(UInt32.init) ?? base.sortField
    }

    // MARK: Changing

    /// Marks a row deleted (− or ⌫); refused while `deleteBlock` names a reason.
    @discardableResult public mutating func delete(_ id: Int) -> Bool {
        guard deleteBlock(id) == nil, let at = index(id), !state.rows[at].deleted else { return false }
        let base = Int(base.sortField)
        record(.deleteField) { state in
            state.rows[at].deleted = true
            // The Sort radio leaves a crossed-out row for the field the save would
            // sort by (S3 step 4: the base's index, lowered to the last field), so
            // the sort field never changes unseen.
            if state.sortRow == id {
                let kept = state.rows.filter { !$0.deleted }
                if let row = kept[safe: min(base, kept.count - 1)] { state.sortRow = row.id }
            }
        }
        return true
    }

    /// Takes a crossed-out row back (its context menu's Restore).
    @discardableResult public mutating func restore(_ id: Int) -> Bool {
        guard let at = index(id), state.rows[at].deleted, canRestore(id) else { return false }
        record(.restoreField) { $0.rows[at].deleted = false }
        return true
    }

    /// Adds "Field <n>", the first free n, after the last row (N5.2's
    /// defaults); returns its row, which the table puts in rename mode.
    @discardableResult public mutating func addField() -> Int {
        let taken = Set(state.rows.map { $0.name.lowercased() })
        var number = 1
        while taken.contains("field \(number)") { number += 1 }
        let id = nextNewID
        nextNewID -= 1
        let field = StructureDraftField(origin: nil, name: "Field \(number)", sticky: false, rtl: false, fontName: "Arial", fontSize: 20,
                                        description: "", plainText: false, collapsed: false, excludeFromSearch: false)
        record(.addField) { $0.rows.append(NoteTypeFieldRow(id: id, field: field, baseName: nil, protected: false, deleted: false)) }
        return id
    }

    /// Renames a row to a name V5 accepted. The same name changes nothing.
    @discardableResult public mutating func rename(_ id: Int, to newName: String) -> Bool {
        guard let at = index(id), state.rows[at].name != newName else { return false }
        record(.renameField) { $0.rows[at].field.name = newName }
        return true
    }

    /// Moves the rows at `offsets` to `destination` (a drag; `onMove`'s
    /// convention). A move to the same place changes nothing.
    @discardableResult public mutating func move(fromOffsets offsets: IndexSet, toOffset destination: Int) -> Bool {
        var moved = state.rows
        moved.move(fromOffsets: offsets, toOffset: destination)
        guard moved != state.rows else { return false }
        record(.moveField) { $0.rows = moved }
        return true
    }

    /// Turns on the Sort radio of a row that stays.
    @discardableResult public mutating func setSortField(_ id: Int) -> Bool {
        guard let row = row(id), !row.deleted, state.sortRow != id else { return false }
        record(.changeSortField) { $0.sortRow = id }
        return true
    }

    /// One of W4's options: Collapsed in Editor, Right-to-Left, Placeholder.
    @discardableResult public mutating func setOption(_ id: Int, _ change: (inout StructureDraftField) -> Void) -> Bool {
        guard let at = index(id) else { return false }
        var field = state.rows[at].field
        change(&field)
        guard field != state.rows[at].field else { return false }
        record(.changeOption) { $0.rows[at].field = field }
        return true
    }

    // MARK: Card types (W5) and texts (W6)

    /// Why Add and Duplicate are disabled (W5): a cloze kind has one card type.
    public var addCardTypeBlock: NoteTypeCardTypeBlock? { isCloze ? .cloze : nil }

    /// Why Delete is disabled for a card type (W5), nil when it can go.
    public func cardDeleteBlock(_ id: Int) -> NoteTypeCardTypeBlock? {
        guard let row = cardRow(id), !row.deleted else { return nil }
        if isCloze { return .cloze }
        if keptCardRows.count <= 1 { return .lastCardType }
        return nil
    }

    /// `editor/editing-strings` E10's name for a card type added now:
    /// `Card N`, N one more than the card types that stay, raised while one
    /// of them has exactly that name.
    public var newCardTypeName: String {
        let taken = Set(keptCardRows.map(\.name))
        var number = keptCardRows.count + 1
        while taken.contains("Card \(number)") { number += 1 }
        return "Card \(number)"
    }

    /// Add (+) and Duplicate (`notetypes/editor-actions` A6): a new card type
    /// after the last, with `source`'s front and back exactly and nothing
    /// else (no browser texts, no deck override). Returns its row.
    @discardableResult public mutating func addCardType(copying source: Int) -> Int? {
        guard addCardTypeBlock == nil, let copied = cardTexts[source] else { return nil }
        let id = nextNewCardID
        nextNewCardID -= 1
        let name = newCardTypeName
        cardTexts[id] = NoteTypeCardTexts(front: copied.front, back: copied.back, browserFront: "", browserBack: "", targetDeckId: 0,
                                          browserFontName: "", browserFontSize: 0)
        record(.addCardType) { $0.cards.append(NoteTypeCardTypeRow(id: id, origin: nil, name: name, deleted: false)) }
        return id
    }

    /// − or ⌫ (A7 as changed by round 9): the card type is crossed out until Save.
    @discardableResult public mutating func deleteCardType(_ id: Int) -> Bool {
        guard cardDeleteBlock(id) == nil, let at = cardIndex(id), !state.cards[at].deleted else { return false }
        record(.deleteCardType) { $0.cards[at].deleted = true }
        return true
    }

    /// Takes a crossed-out card type back (its context menu's Restore).
    @discardableResult public mutating func restoreCardType(_ id: Int) -> Bool {
        guard let at = cardIndex(id), state.cards[at].deleted else { return false }
        record(.restoreCardType) { $0.cards[at].deleted = false }
        return true
    }

    /// A8: every `"` is removed; a blank result, or the same name, changes
    /// nothing. The result is not trimmed (the save makes names unique, N4).
    @discardableResult public mutating func renameCardType(_ id: Int, to typed: String) -> Bool {
        let name = typed.replacingOccurrences(of: "\"", with: "")
        guard let at = cardIndex(id), !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, state.cards[at].name != name else {
            return false
        }
        record(.renameCardType) { $0.cards[at].name = name }
        return true
    }

    /// A drag in the card types list (`onMove`'s convention).
    @discardableResult public mutating func moveCardTypes(fromOffsets offsets: IndexSet, toOffset destination: Int) -> Bool {
        var moved = state.cards
        moved.move(fromOffsets: offsets, toOffset: destination)
        guard moved != state.cards else { return false }
        record(.moveCardType) { $0.cards = moved }
        return true
    }

    /// Move Up (−1) and Move Down (+1), over every row of the list.
    public func canMoveCardType(_ id: Int, by delta: Int) -> Bool {
        guard let at = cardIndex(id) else { return false }
        return state.cards.indices.contains(at + delta)
    }

    @discardableResult public mutating func moveCardType(_ id: Int, by delta: Int) -> Bool {
        guard canMoveCardType(id, by: delta), let at = cardIndex(id) else { return false }
        return moveCardTypes(fromOffsets: IndexSet(integer: at), toOffset: delta > 0 ? at + delta + 1 : at + delta)
    }

    /// Typing in the template editor: a front, a back, or the shared
    /// styling. Not a draft step: the text view keeps its own undo (W13).
    @discardableResult public mutating func setText(_ card: Int, _ part: NoteTypeTemplatePart, _ text: String) -> Bool {
        switch part {
        case .styling:
            guard css != text else { return false }
            css = text
        case .front:
            guard cardTexts[card] != nil, cardTexts[card]?.front != text else { return false }
            cardTexts[card]?.front = text
        case .back:
            guard cardTexts[card] != nil, cardTexts[card]?.back != text else { return false }
            cardTexts[card]?.back = text
        }
        return true
    }

    /// The editor's own undo (W13).
    @discardableResult public mutating func undo() -> NoteTypeDraftStep? {
        guard let entry = undoStack.popLast() else { return nil }
        redoStack.append(Entry(step: entry.step, state: state))
        state = entry.state
        return entry.step
    }

    @discardableResult public mutating func redo() -> NoteTypeDraftStep? {
        guard let entry = redoStack.popLast() else { return nil }
        undoStack.append(Entry(step: entry.step, state: state))
        state = entry.state
        return entry.step
    }

    private func index(_ id: Int) -> Int? { state.rows.firstIndex { $0.id == id } }
    private func cardIndex(_ id: Int) -> Int? { state.cards.firstIndex { $0.id == id } }

    private mutating func record(_ step: NoteTypeDraftStep, _ change: (inout State) -> Void) {
        undoStack.append(Entry(step: step, state: state))
        redoStack.removeAll()
        change(&state)
    }
}

extension NoteTypeDraft {
    /// W1's Used On for each row: the positions in `templates` of the card
    /// types whose front or back refers to it, from each text's T11.1
    /// references (`references[2i]` the front of card type i, `2i + 1` its
    /// back). The draft's renames are applied first, as the save rewrites
    /// the texts (`notetypes/field-rename` F3): a key that names a base
    /// field refers to that field's row, renamed or not; any other key
    /// refers to the added field of that name.
    public func usedOn(references: [[String]]) -> [Int: [Int]] {
        var byKey: [String: Int] = [:]
        for row in rows where row.baseName == nil { byKey[row.name] = byKey[row.name] ?? row.id }
        for row in rows { if let baseName = row.baseName { byKey[baseName] = row.id } }
        var result: [Int: [Int]] = Dictionary(uniqueKeysWithValues: rows.map { ($0.id, []) })
        for template in templates.indices {
            let keys = Set((references[safe: 2 * template] ?? []) + (references[safe: 2 * template + 1] ?? []))
            for id in Set(keys.compactMap { byKey[$0] }) { result[id, default: []].append(template) }
        }
        return result
    }

    /// The texts `usedOn(references:)` takes, in its order.
    public var referenceTexts: [String] { templates.flatMap { [$0.front, $0.back] } }
}

extension Array {
    nonisolated subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
