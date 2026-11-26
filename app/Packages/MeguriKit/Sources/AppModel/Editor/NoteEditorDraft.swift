import CoreBridge
import Foundation
import Observation

/// The shared, collection-bound field targets. WebKit owns serialization and
/// caret state; this owner keeps original stored bytes, targets and UI state
/// across layout remounts and web-process recovery. It performs no note writes.
@Observable @MainActor
public final class NoteEditorDraft {
    /// Preparation failures are displayed by Add, Edit and Browse. Keep the
    /// diagnostic readable even for callers using String(describing:).
    public struct LayoutError: Error, LocalizedError, CustomStringConvertible, Sendable {
        public nonisolated let description: String
        public nonisolated var errorDescription: String? { description }
    }
    public enum Mode: String, Codable, Sendable { case rich, html }
    public struct Field: Sendable, Equatable {
        /// The raw stored definition, including its ordinal for stale checks.
        /// All editor/page addresses use this field's position in `fields`.
        public let definition: NotetypeField
        public fileprivate(set) var baseline: String
        /// The value the page was opened (or last reloaded) with: what its
        /// "unchanged" commit stands for. Only a Browse save moves `baseline`
        /// away from it (`acknowledgeSaved`).
        fileprivate var opened: String
        public fileprivate(set) var target: String
        /// The page differs from `opened`, independently of the save baseline.
        public fileprivate(set) var changed: Bool
        public fileprivate(set) var mode: Mode
        public fileprivate(set) var collapsed: Bool
    }
    public struct Commit: Sendable {
        public let ord: Int
        public let html: String
        public let unchanged: Bool
        public init(ord: Int, html: String, unchanged: Bool) {
            self.ord = ord; self.html = html; self.unchanged = unchanged
        }
    }
    public let identity = UUID()
    public let generation: UInt64
    /// Edit mode (`review/edit-current`): the stored note the fields were
    /// loaded from. Nil for an Add draft.
    public let noteID: Int64?
    public let notetype: NotetypeRecord
    public private(set) var deck: DeckNameEntry
    public private(set) var fields: [Field]
    public private(set) var tags: [String]
    /// Edit mode: the tags as stored when the note was loaded or reloaded.
    private var baselineTags: [String]
    public private(set) var pendingFields: Set<Int> = []
    public private(set) var focusedField: Int?
    public private(set) var fieldCheck: FieldCheck?
    public private(set) var fieldCheckError: String?
    private var revision: UInt64 = 0
    private var acknowledgedRevision: UInt64?
    // Internal suspension seam holds an actual Core conversion result, never
    // substitutes conversion or grants permission to mutate the draft.
    var afterConvertingFields: (@MainActor () async -> Void)?
    private var checkRevision: UInt64 = 0
    private var lastAddedValues: [String]?
    private let html = CoreEditorFieldHTML()

    public convenience init(generation: UInt64, notetype: NotetypeRecord, deck: DeckNameEntry,
                            values: [String]? = nil, tags: [String] = []) throws {
        try self.init(generation: generation, notetype: notetype, deck: deck, values: values, tags: tags, noteID: nil)
    }

    /// Edit mode: the stored note's fields are the baselines, so a field is
    /// changed only once its target differs from what is stored. The card's
    /// deck may be a filtered one; it is shown, never chosen, here.
    public convenience init(editing note: NoteRecord, notetype: NotetypeRecord, deck: DeckNameEntry, generation: UInt64) throws {
        guard note.notetypeId == notetype.id else {
            throw BridgeError.stale(table: .notetypes, key: String(note.notetypeId))
        }
        try self.init(generation: generation, notetype: notetype, deck: deck, values: note.fields, tags: note.tags, noteID: note.id)
    }

    private init(generation: UInt64, notetype: NotetypeRecord, deck: DeckNameEntry,
                 values: [String]?, tags: [String], noteID: Int64?) throws {
        guard noteID != nil || !deck.filtered else {
            throw LayoutError(description: "Choose a normal deck before adding a note.")
        }
        guard !notetype.fields.isEmpty else {
            throw LayoutError(description: "This note type has no fields. Run Check Database before editing it.")
        }
        // N1 permits gaps, including before the first row. Storage supplies
        // ord order; preserve those identities while addressing values by position.
        guard zip(notetype.fields, notetype.fields.dropFirst()).allSatisfy({ pair in pair.0.ord < pair.1.ord }) else {
            throw LayoutError(description: "This note type has duplicate or unordered field ordinals. Run Check Database before editing it.")
        }
        guard values == nil || values?.count == notetype.fields.count else {
            throw LayoutError(description: "The note’s field count (\(values?.count ?? 0)) does not match its note type (\(notetype.fields.count)). Run Check Database before editing it.")
        }
        self.generation = generation; self.notetype = notetype; self.deck = deck; self.tags = tags
        self.noteID = noteID; baselineTags = tags
        fields = notetype.fields.enumerated().map { index, definition in
            let value = values?[index] ?? ""
            return Field(definition: definition, baseline: value, opened: value, target: value, changed: false,
                mode: definition.plainText ? .html : .rich, collapsed: definition.collapsed)
        }
    }

    public var values: [String] { fields.map(\.target) }
    public var occlusionRoles: OcclusionFieldRoles? { OcclusionFieldRoles(notetype: notetype) }
    public var hostFields: Set<Int> {
        occlusionRoles?.hidden ?? (notetype.originalStockKind == 6 ? [0] : [])
    }
    /// Native state survives switching between fields and masks and layout changes.
    public var showsMaskEditor = true
    public var hostFieldsHeld = false
    public var occlusionAddRefused = false
    /// The caller already checked its retained host's identity and admission.
    /// Page commits never address these fields, including an outstanding flush.
    public func acceptHostField(_ value: String, at index: Int) {
        guard !hostFieldsHeld, hostFields.contains(index), fields.indices.contains(index),
              fields[index].target != value else { return }
        fields[index].target = value
        fields[index].changed = value != fields[index].opened
        occlusionAddRefused = false
        revision &+= 1
    }
    public var isBlank: Bool { pendingFields.isEmpty && fields.allSatisfy { Self.blankValue($0.target).isEmpty } }
    /// N17.1 enables Add. Typing counts before its commit arrives; a committed
    /// value follows the live field check, so markup with no content stays out.
    public var firstFieldHasContent: Bool {
        if pendingFields.contains(0) { return true }
        guard let first = fields.first, !Self.blankValue(first.target).isEmpty else { return false }
        // Only a check of exactly this content can say the field is empty. A
        // failed or older check is unknown: Add stays enabled and checks itself.
        return !(fieldCheckError == nil && checkedRevision == revision && fieldCheck?.state == .empty)
    }
    private var checkedRevision: UInt64?
    public var unchangedSinceLastAdd: Bool {
        guard pendingFields.isEmpty else { return false }
        return fields.enumerated().allSatisfy { index, field in
            let value = Self.blankValue(field.target)
            return value.isEmpty || (field.definition.sticky && lastAddedValues.map { value == Self.blankValue($0[index]) } == true)
        }
    }

    /// Called only after an acknowledged add. D6 carries draft bytes before
    /// normalization; this does not infer success or perform another query.
    public func nextAfterAcknowledgedAdd() throws -> NoteEditorDraft {
        guard pendingFields.isEmpty else { throw BridgeError.unavailable(reason: "Flush the fields before adding.") }
        let next = try NoteEditorDraft(generation: generation, notetype: notetype, deck: deck,
            values: fields.enumerated().map { !hostFields.contains($0.offset) && $0.element.definition.sticky ? $0.element.target : "" }, tags: tags)
        for index in fields.indices {
            next.fields[index].mode = fields[index].mode
            next.fields[index].collapsed = fields[index].collapsed
        }
        next.lastAddedValues = values
        next.focusedField = fields.indices.first
        return next
    }

    /// Edit mode: input not committed yet, a committed field that differs from
    /// its stored HTML, or changed tags (`review/edit-current` E2).
    public var hasChanges: Bool {
        !pendingFields.isEmpty || tags != baselineTags || fields.contains { $0.target != $0.baseline }
    }

    /// Edit mode, after a flush: one patch with every field whose target
    /// differs from its stored HTML and the tags when they changed
    /// (`editor/field-html` S2). Nil when there is nothing to send.
    public func patch() -> NoteFieldPatch? {
        guard let noteID, pendingFields.isEmpty else { return nil }
        var changed: [Int: String] = [:]
        for (ord, field) in fields.enumerated() where field.target != field.baseline { changed[ord] = field.target }
        let tags = self.tags == baselineTags ? nil : self.tags
        guard !changed.isEmpty || tags != nil else { return nil }
        return NoteFieldPatch(id: noteID, notetype: notetype, fields: changed, tags: tags, generation: generation)
    }

    /// Browse's inspector (`browser/workspace` V12): the changed fields as
    /// last committed and the tags when they changed, whatever input is
    /// still on its way in another field (that input is saved by a later
    /// save). Nil when nothing committed differs from what is stored, so a
    /// save then writes nothing.
    public func committedPatch() -> NoteFieldPatch? {
        guard let noteID else { return nil }
        var changed: [Int: String] = [:]
        for (ord, field) in fields.enumerated() where field.target != field.baseline { changed[ord] = field.target }
        let tags = self.tags == baselineTags ? nil : self.tags
        guard !changed.isEmpty || tags != nil else { return nil }
        return NoteFieldPatch(id: noteID, notetype: notetype, fields: changed, tags: tags, generation: generation)
    }

    /// Browse's inspector: `patch` committed, so what it sent is what is
    /// stored now. Each sent field and the sent tags become the baseline; a
    /// field edited again since keeps its newer target and stays changed.
    public func acknowledgeSaved(_ patch: NoteFieldPatch) {
        guard patch.id == noteID else { return }
        for (ord, html) in patch.fields where fields.indices.contains(ord) {
            fields[ord].baseline = html
            // Saving does not change what the page opened with. Recovery
            // still needs that baseline, or an unchanged page commit could
            // restore the text from before this save.
        }
        if let tags = patch.tags { baselineTags = tags }
        revision &+= 1
    }

    /// Browse's inspector (V12): the fields the page leaves out, behind the
    /// "<e> Empty Fields" row. Set before the page opens the draft, or
    /// changed through the runtime; UI state only.
    public var hiddenFields: Set<Int> = []

    /// Edit mode, for a change made elsewhere (`editor/edit-flows` W5.2 with
    /// E2's rule): every field without an unsaved edit takes the stored value
    /// as its baseline and target; a changed or pending field keeps the
    /// draft. Returns the ordinals the page must load again.
    @discardableResult
    public func reloadUnchanged(stored values: [String], tags stored: [String]) -> [Int] {
        guard noteID != nil, values.count == fields.count else { return [] }
        var reloaded: [Int] = []
        for index in fields.indices where !pendingFields.contains(index) && fields[index].target == fields[index].baseline {
            guard fields[index].baseline != values[index] else { continue }
            fields[index].baseline = values[index]; fields[index].target = values[index]; fields[index].opened = values[index]
            fields[index].changed = false
            reloaded.append(index)
        }
        if tags == baselineTags { tags = stored }
        baselineTags = stored
        if !reloaded.isEmpty { revision &+= 1 }
        return reloaded
    }

    /// The page's display form of one field's target (B4).
    public func displayValue(_ ord: Int) async throws -> String {
        guard fields.indices.contains(ord) else { throw BridgeError.invalidArgument("Invalid editor field.") }
        let converted = try await html.convert([fields[ord].target], forDisplay: true)
        guard let value = converted.first, converted.count == 1 else { throw BridgeError.internal("Invalid editor conversion result.") }
        return value
    }

    /// Explicit recovery acknowledges WebKit's lost uncommitted input; the
    /// committed targets and original baselines are always retained.
    public func acknowledgeProcessLoss() { pendingFields.removeAll(); revision &+= 1 }

    // D4 uses Unicode White_Space, not Foundation's broader control-character set.
    public static func blankValue(_ value: String) -> String {
        let scalars = value.replacingOccurrences(of: "<br>", with: "").unicodeScalars
        let space: (Unicode.Scalar) -> Bool = { scalar in
            switch scalar.value {
            case 0x09...0x0D, 0x20, 0x85, 0xA0, 0x1680, 0x2000...0x200A,
                 0x2028, 0x2029, 0x202F, 0x205F, 0x3000: true
            default: false
            }
        }
        return String(String.UnicodeScalarView(scalars.drop(while: space).reversed().drop(while: space).reversed()))
    }

    public func markDirty(_ ord: Int) {
        guard fields.indices.contains(ord), !hostFields.contains(ord) else { return }
        pendingFields.insert(ord); revision &+= 1
    }

    /// Apply one ordered page message as a unit. Unchanged fields never pass
    /// through a parser or a media decoder, even after undo or a mode switch.
    public func accept(_ commits: [Commit], ifCurrent current: @MainActor () -> Bool) async throws {
        guard current() else { throw BridgeError.unavailable(reason: "The editor owner changed.") }
        guard !commits.isEmpty else { return }
        guard Set(commits.map(\.ord)).count == commits.count,
              commits.allSatisfy({ fields.indices.contains($0.ord) }) else {
            throw BridgeError.invalidArgument("Invalid editor field commit.")
        }
        let commits = commits.filter { !hostFields.contains($0.ord) }
        let changed = commits.filter { !$0.unchanged }
        let targets = try await html.convert(changed.map(\.html), forDisplay: false)
        guard targets.count == changed.count else { throw BridgeError.internal("Invalid editor conversion result.") }
        let byOrdinal = Dictionary(uniqueKeysWithValues: zip(changed.map(\.ord), targets))
        await afterConvertingFields?()
        // Conversion suspends on CoreBridge. Check on this actor immediately
        // before changing either committed bytes or pending dirty state.
        guard current() else { throw BridgeError.unavailable(reason: "The editor owner changed.") }
        for commit in commits {
            // "Unchanged" is the page's word for "as I was opened with", which a
            // Browse save may have left behind as the stored value.
            fields[commit.ord].target = commit.unchanged ? fields[commit.ord].opened : byOrdinal[commit.ord]!
            fields[commit.ord].changed = !commit.unchanged
            pendingFields.remove(commit.ord)
        }
        revision &+= 1
    }

    public func setTags(_ tags: [String]) { self.tags = tags; revision &+= 1 }

    public func setFocus(_ ord: Int?) { focusedField = ord }
    public func setMode(_ mode: Mode, ord: Int) {
        guard fields.indices.contains(ord) else { return }; fields[ord].mode = mode; revision &+= 1
    }
    public func setCollapsed(_ collapsed: Bool, ord: Int) {
        guard fields.indices.contains(ord) else { return }; fields[ord].collapsed = collapsed; revision &+= 1
    }
    public func selectDeck(_ deck: DeckNameEntry) throws {
        guard !deck.filtered else { throw BridgeError.invalidArgument("Choose a normal deck.") }
        self.deck = deck; revision &+= 1
    }

    var preparationRevision: UInt64 { revision }
    // Only the actual durable reply disarms the submitted revision. A newer
    // revision in this identity is separate input and remains recoverable.
    var submissionAcknowledged: Bool { acknowledgedRevision == revision }
    func acknowledgeSubmission(revision: UInt64) { acknowledgedRevision = revision }

    /// Rebind only after a held flush. D5 maps a missing type by A3.1;
    /// a deck-only repair preserves the last-add comparison.
    func replacingSelection(notetype: NotetypeRecord, deck: DeckNameEntry) throws -> NoteEditorDraft {
        let changedType = notetype.id != self.notetype.id
        var mapped = values
        if changedType {
            mapped = Array(repeating: "", count: notetype.fields.count)
            var used: Set<Int> = [], filled: Set<Int> = []
            for (index, field) in notetype.fields.enumerated() {
                if let old = fields.indices.first(where: {
                    fields[$0].definition.name == field.name && !fields[$0].target.isEmpty
                }) {
                    mapped[index] = fields[old].target; used.insert(old); filled.insert(index)
                }
            }
            var remaining = fields.indices.filter { !used.contains($0) && !fields[$0].target.isEmpty }.makeIterator()
            for index in mapped.indices where !filled.contains(index) {
                if let old = remaining.next() { mapped[index] = fields[old].target }
            }
        }
        if changedType, let roles = OcclusionFieldRoles(notetype: notetype) {
            mapped = Array(repeating: "", count: notetype.fields.count)
            var used: Set<Int> = [], filled: Set<Int> = []
            if let old = occlusionRoles {
                for (source, target) in zip([old.occlusions, old.image], [roles.occlusions, roles.image]) {
                    mapped[target] = values[source]; used.insert(source); filled.insert(target)
                }
            }
            let destinations = notetype.fields.indices.filter { !roles.hidden.contains($0) }
            for target in destinations where !filled.contains(target) {
                if let source = fields.indices.first(where: {
                    !used.contains($0) && !hostFields.contains($0) && fields[$0].definition.name == notetype.fields[target].name && !values[$0].isEmpty
                }) { mapped[target] = values[source]; used.insert(source); filled.insert(target) }
            }
            var remaining = fields.indices.filter { !used.contains($0) && !hostFields.contains($0) && !values[$0].isEmpty }.makeIterator()
            for target in destinations where !filled.contains(target) {
                if let source = remaining.next() { mapped[target] = values[source] }
            }
        }
        let next = try NoteEditorDraft(generation: generation, notetype: notetype, deck: deck, values: mapped, tags: tags)
        if changedType {
            // A3 step 5: the same position, or the new type's last field.
            next.focusedField = min(focusedField ?? 0, next.fields.count - 1)
        } else {
            next.lastAddedValues = lastAddedValues
            for index in fields.indices {
                next.fields[index].mode = fields[index].mode
                next.fields[index].collapsed = fields[index].collapsed
            }
        }
        return next
    }

    /// B4 display values are derived from targets, with the original baseline
    /// supplied on recovery for changed fields (F7).
    public func displayValues() async throws -> (targets: [String], baselines: [String]) {
        let count = fields.count
        let pageValues = fields.enumerated().map { hostFields.contains($0.offset) ? "" : $0.element.target }
        let pageBaselines = fields.enumerated().map { hostFields.contains($0.offset) ? "" : $0.element.opened }
        let converted = try await html.convert(pageValues + pageBaselines, forDisplay: true)
        guard converted.count == count * 2 else { throw BridgeError.internal("Invalid editor conversion result.") }
        return (Array(converted.prefix(count)), Array(converted.suffix(count)))
    }

    public func checkFields(ifCurrent current: @escaping @MainActor () -> Bool) async {
        guard current() else { return }
        checkRevision &+= 1
        let expected = revision, request = checkRevision
        do {
            let result = try await CoreNoteService().checkNoteFields(notetypeId: notetype.id, fields: values, ownNote: nil)
            guard current(), expected == revision, request == checkRevision else { return }
            fieldCheck = result; fieldCheckError = nil; checkedRevision = expected
        } catch {
            guard current(), expected == revision, request == checkRevision else { return }
            fieldCheckError = String(describing: error)
        }
    }
}

/// Prepared Add state belongs to the collection, never to a SwiftUI presentation.
@Observable @MainActor
public final class AddNoteDraft {
    public private(set) var draft: NoteEditorDraft?
    public private(set) var preparing = false
    public private(set) var error: String?
    private weak var owner: ReviewCollection?
    private let defaults = CoreEditorServices()
    var afterReadingLifecycle: (@MainActor () async -> Void)?
    init(owner: ReviewCollection) { self.owner = owner }

    func replace(with draft: NoteEditorDraft?) { self.draft = draft; error = nil }

    public func prepare(presetDeck: Int64? = nil) async {
        guard !preparing, let owner, !owner.addNoteFlow.isPresented, owner.state == .ready, owner.foreground, !owner.resuming,
              owner.lifecycle == .ready(generation: owner.generation) else { return }
        preparing = true
        defer { preparing = false }
        let generation = owner.generation
        let previous = draft, previousRevision = draft?.preparationRevision
        let current = { self.owner === owner && owner.state == .ready && owner.foreground && !owner.resuming &&
            owner.generation == generation && owner.lifecycle == .ready(generation: generation) }
        do {
            guard try await CoreCollectionService().lifecycle() == .ready(generation: generation), current() else {
                throw BridgeError.unavailable(reason: "The collection changed. Your draft has been kept.")
            }
            if let presetDeck {
                let names = try await CoreDeckService().allDeckNames()
                guard current(), names.generation == generation,
                      names.decks.contains(where: { $0.id == presetDeck && !$0.filtered }) else {
                    throw BridgeError.unavailable(reason: "The selected deck is no longer a normal deck. Choose a deck again.")
                }
            }
            // D3: an existing nonblank draft keeps its content and type. Only a
            // valid deck preset may change its deck. No default query is made.
            if let draft, draft.generation == generation, !draft.isBlank {
                let names = try await CoreDeckService().allDeckNames()
                guard current(), names.generation == generation else { throw BridgeError.staleGeneration(planned: generation, current: owner.generation) }
                guard self.draft === draft else { return }
                if let presetDeck {
                    guard let preset = names.decks.first(where: { $0.id == presetDeck && !$0.filtered }) else {
                        throw BridgeError.unavailable(reason: "The selected deck changed. Choose a deck again.")
                    }
                    try draft.selectDeck(preset)
                }
                error = nil; return
            }
            let selection = try await defaults.addDefaults(reviewCard: owner.review?.item?.cardId, presetDeck: presetDeck)
            guard current() else { throw BridgeError.staleGeneration(planned: generation, current: owner.generation) }
            let notetype = try await CoreNotetypeService().readNotetype(id: selection.notetypeId)
            guard current() else { throw BridgeError.staleGeneration(planned: generation, current: owner.generation) }
            let names = try await CoreDeckService().allDeckNames()
            guard current(), names.generation == generation,
                  try await CoreCollectionService().lifecycle() == .ready(generation: generation),
                  let deck = names.decks.first(where: { $0.id == selection.deckId && !$0.filtered }),
                  presetDeck == nil || deck.id == presetDeck else {
                throw BridgeError.unavailable(reason: "The Add selection changed. Your draft has been kept.")
            }
            await afterReadingLifecycle?()
            guard current() else { throw BridgeError.staleGeneration(planned: generation, current: owner.generation) }
            // A blank draft can acquire input while any default/type/deck or
            // lifecycle query is suspended. Preserve that exact draft, even
            // if input was subsequently undone back to blank.
            guard draft === previous, draft?.preparationRevision == previousRevision else { return }
            // An obsolete draft is retained for recovery; never reinterpret its
            // identities or nonempty content against another collection.
            if let draft, draft.generation != generation, !draft.isBlank {
                throw BridgeError.unavailable(reason: "This draft belongs to another collection. Its contents have been kept.")
            }
            draft = try NoteEditorDraft(generation: generation, notetype: notetype, deck: deck)
            error = nil
        } catch { self.error = String(describing: error) }
    }
}

extension ReviewCollection {
    public var addNoteDraft: AddNoteDraft {
        addNoteFlow.preparation
    }
}
