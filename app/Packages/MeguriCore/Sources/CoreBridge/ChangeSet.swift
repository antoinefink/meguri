internal import MeguriCoreBindings

/// Where a core op came from.
public enum OpSource: Sendable, Hashable {
    case userUndoable
    case userNonUndoable
    case syncIncoming
    case localApi
    case optimizer
    case rollover
    case maintenance

    init(_ source: MeguriCoreBindings.OpSource) {
        switch source {
        case .userUndoable: self = .userUndoable
        case .userNonUndoable: self = .userNonUndoable
        case .syncIncoming: self = .syncIncoming
        case .localApi: self = .localApi
        case .optimizer: self = .optimizer
        case .rollover: self = .rollover
        case .maintenance: self = .maintenance
        }
    }
}

/// Everything one committed op changed, from a `changes` event. Every list is
/// sorted by its key and holds each key once.
public struct ChangeSet: Sendable, Hashable {
    /// How one row changed in the op.
    public enum Change: Sendable, Hashable {
        case added
        case updated
        case removed

        init(_ kind: MeguriCoreBindings.ChangeKind) {
            switch kind {
            case .added: self = .added
            case .updated: self = .updated
            case .removed: self = .removed
            }
        }
    }

    public struct Card: Sendable, Hashable {
        public let id: Int64
        /// The note after the op; for a removed card, before it.
        public let noteID: Int64
        public let change: Change
        /// Only the FSRS memory state changed: nothing a queue or a due date
        /// depends on.
        public let memoryStateOnly: Bool
    }

    /// A changed note, deck or deck config.
    public struct Row: Sendable, Hashable {
        public let id: Int64
        public let change: Change
    }

    public struct Notetype: Sendable, Hashable {
        public let id: Int64
        public let change: Change
        /// Added or removed, or a field or template added, removed or renamed.
        public let schemaChanged: Bool
    }

    /// A changed config key or tag.
    public struct Named: Sendable, Hashable {
        public let name: String
        public let change: Change
    }

    /// An external writer invalidated every cached collection value.
    public let wholeCollection: Bool
    public let source: OpSource
    public let cards: [Card]
    public let notes: [Row]
    public let notetypes: [Notetype]
    public let decks: [Row]
    public let deckConfigs: [Row]
    public let configKeys: [Named]
    public let tags: [Named]
    public let revlogAdded: [Int64]
    public let revlogRemoved: [Int64]
    public let mediaAdded: [String]
    public let mediaRemoved: [String]
    /// A collection-row column other than its modification time changed.
    public let collectionRow: Bool
}

// The bindings init lives here, so the struct keeps its memberwise one.
extension ChangeSet {
    init(_ changes: MeguriCoreBindings.ChangeSet) {
        let rows = { (list: [IdChange]) in list.map { Row(id: $0.id, change: Change($0.kind)) } }
        let named = { (list: [NameChange]) in list.map { Named(name: $0.name, change: Change($0.kind)) } }
        wholeCollection = changes.wholeCollection
        source = OpSource(changes.source)
        cards = changes.cards.map {
            Card(id: $0.id, noteID: $0.noteId, change: Change($0.kind), memoryStateOnly: $0.memoryStateOnly)
        }
        notes = rows(changes.notes)
        notetypes = changes.notetypes.map {
            Notetype(id: $0.id, change: Change($0.kind), schemaChanged: $0.schemaChanged)
        }
        decks = rows(changes.decks)
        deckConfigs = rows(changes.deckConfigs)
        configKeys = named(changes.configKeys)
        tags = named(changes.tags)
        revlogAdded = changes.revlogAdded
        revlogRemoved = changes.revlogRemoved
        mediaAdded = changes.mediaAdded
        mediaRemoved = changes.mediaRemoved
        collectionRow = changes.collectionRow
    }
}
