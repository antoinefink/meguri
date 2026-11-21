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
}

/// Why Delete is disabled for a row (W3), shown as its help text.
public nonisolated enum NoteTypeFieldDeleteBlock: Sendable, Hashable {
    /// The last field that is not crossed out.
    case lastField
    /// A field the note type's kind needs (S1.2 item 3).
    case protected
}

/// One note type's draft (`notetypes/draft-save` D1): every unsaved change
/// to its fields, field options and sort field, with the base it was made
/// from (its token goes to the save) and the editor's own undo steps (W13).
/// Card types, texts and styling are carried unchanged until the Cards tab
/// edits them (U282). A value type: the model keeps one per note type.
public nonisolated struct NoteTypeDraft: Sendable, Hashable {
    private struct State: Sendable, Hashable {
        var rows: [NoteTypeFieldRow]
        var sortRow: Int
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
    public var templates: [StructureDraftTemplate]
    public var css: String
    public var name: String

    public init(_ base: NotetypeRecord) {
        self.base = base
        let rows = base.fields.map {
            NoteTypeFieldRow(id: Int($0.ord), field: StructureDraftField($0), baseName: $0.name, protected: $0.preventDeletion, deleted: false)
        }
        state = State(rows: rows, sortRow: Int(base.sortField))
        templates = base.templates.map(StructureDraftTemplate.init)
        css = base.css
        name = base.name
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

    /// The field changes the save classifies as structural (S2): a field
    /// added, removed or moved, or another sort field. Renames and options
    /// are not.
    public var changesStructure: Bool {
        let kept = keptRows.map(\.origin)
        return kept != base.fields.map { Optional($0.ord) } || effectiveSortPosition != base.sortField
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
        guard let at = index(id), state.rows[at].deleted else { return false }
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
