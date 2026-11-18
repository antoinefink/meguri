import CoreBridge
import Foundation
import Observation

/// The shared, collection-bound field targets. WebKit owns serialization and
/// caret state; this owner keeps original stored bytes, targets and UI state
/// across layout remounts and web-process recovery. It performs no note writes.
@Observable @MainActor
public final class NoteEditorDraft {
    public enum Mode: String, Codable, Sendable { case rich, html }
    public struct Field: Sendable, Equatable {
        public let definition: NotetypeField
        public let baseline: String
        public fileprivate(set) var target: String
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
    public let notetype: NotetypeRecord
    public private(set) var deck: DeckNameEntry
    public private(set) var fields: [Field]
    public private(set) var tags: [String]
    public private(set) var pendingFields: Set<Int> = []
    public private(set) var focusedField: Int?
    public private(set) var fieldCheck: FieldCheck?
    public private(set) var fieldCheckError: String?
    private var revision: UInt64 = 0
    // Internal suspension seam holds an actual Core conversion result, never
    // substitutes conversion or grants permission to mutate the draft.
    var afterConvertingFields: (@MainActor () async -> Void)?
    private var checkRevision: UInt64 = 0
    private var lastAddedValues: [String]?
    private let html = CoreEditorFieldHTML()

    public init(generation: UInt64, notetype: NotetypeRecord, deck: DeckNameEntry,
                values: [String]? = nil, tags: [String] = []) throws {
        guard !deck.filtered, notetype.originalStockKind != 6, !notetype.fields.isEmpty,
              notetype.fields.enumerated().allSatisfy({ Int($0.element.ord) == $0.offset }),
              values == nil || values?.count == notetype.fields.count else {
            throw BridgeError.invalidArgument("This note type or field layout is not supported by the field editor.")
        }
        self.generation = generation; self.notetype = notetype; self.deck = deck; self.tags = tags
        fields = notetype.fields.enumerated().map { index, definition in
            let value = values?[index] ?? ""
            return Field(definition: definition, baseline: value, target: value, changed: false,
                mode: definition.plainText ? .html : .rich, collapsed: definition.collapsed)
        }
    }

    public var values: [String] { fields.map(\.target) }
    public var isBlank: Bool { pendingFields.isEmpty && fields.allSatisfy { Self.blankValue($0.target).isEmpty } }
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
            values: fields.map { $0.definition.sticky ? $0.target : "" }, tags: tags)
        for index in fields.indices {
            next.fields[index].mode = fields[index].mode
            next.fields[index].collapsed = fields[index].collapsed
        }
        next.lastAddedValues = values
        return next
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
        guard fields.indices.contains(ord) else { return }
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
        let changed = commits.filter { !$0.unchanged }
        let targets = try await html.convert(changed.map(\.html), forDisplay: false)
        guard targets.count == changed.count else { throw BridgeError.internal("Invalid editor conversion result.") }
        let byOrdinal = Dictionary(uniqueKeysWithValues: zip(changed.map(\.ord), targets))
        await afterConvertingFields?()
        // Conversion suspends on CoreBridge. Check on this actor immediately
        // before changing either committed bytes or pending dirty state.
        guard current() else { throw BridgeError.unavailable(reason: "The editor owner changed.") }
        for commit in commits {
            fields[commit.ord].target = commit.unchanged ? fields[commit.ord].baseline : byOrdinal[commit.ord]!
            fields[commit.ord].changed = !commit.unchanged
            pendingFields.remove(commit.ord)
        }
        revision &+= 1
    }

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

    fileprivate var preparationRevision: UInt64 { revision }

    /// B4 display values are derived from targets, with the original baseline
    /// supplied on recovery for changed fields (F7).
    public func displayValues() async throws -> (targets: [String], baselines: [String]) {
        let count = fields.count
        let converted = try await html.convert(values + fields.map(\.baseline), forDisplay: true)
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
            fieldCheck = result; fieldCheckError = nil
        } catch {
            guard current(), expected == revision, request == checkRevision else { return }
            fieldCheckError = String(describing: error)
        }
    }
}

/// Prepared Add state belongs to Navigation, never to a SwiftUI presentation.
/// Production entry remains disconnected until writer/admission fences exist.
@Observable @MainActor
public final class AddNoteDraft {
    public private(set) var draft: NoteEditorDraft?
    public private(set) var preparing = false
    public private(set) var error: String?
    private weak var owner: ReviewCollection?
    private let defaults = CoreEditorServices()
    var afterReadingLifecycle: (@MainActor () async -> Void)?
    init(owner: ReviewCollection) { self.owner = owner }

    public func prepare(presetDeck: Int64? = nil) async {
        guard !preparing, let owner, owner.state == .ready, owner.foreground, !owner.resuming,
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
            // D3: an existing nonblank draft keeps its content and type. Only a
            // valid deck preset may change its deck. No default query is made.
            if let draft, draft.generation == generation, !draft.isBlank {
                let names = try await CoreDeckService().allDeckNames()
                guard current(), names.generation == generation else { throw BridgeError.staleGeneration(planned: generation, current: owner.generation) }
                guard self.draft === draft else { return }
                if let preset = names.decks.first(where: { $0.id == presetDeck && !$0.filtered }) {
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
                  let deck = names.decks.first(where: { $0.id == selection.deckId && !$0.filtered }) else {
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
        navigation.featureState(AddNoteDraft.self) { AddNoteDraft(owner: self) }
    }
}
