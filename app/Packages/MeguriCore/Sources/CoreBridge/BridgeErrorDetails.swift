internal import MeguriCoreBindings

public enum CoreTable: Sendable, Hashable {
    case col
    case notes
    case cards
    case revlog
    case graves
    case deckConfig
    case config
    case tags
    case fields
    case templates
    case notetypes
    case decks
    init(_ raw: MeguriCoreBindings.CoreTable) {
        switch raw {
        case .col: self = .col
        case .notes: self = .notes
        case .cards: self = .cards
        case .revlog: self = .revlog
        case .graves: self = .graves
        case .deckConfig: self = .deckConfig
        case .config: self = .config
        case .tags: self = .tags
        case .fields: self = .fields
        case .templates: self = .templates
        case .notetypes: self = .notetypes
        case .decks: self = .decks
        }
    }
}

public enum SchemaChangeReason: Sendable, Hashable {
    case fieldsChanged
    case sortFieldChanged
    case templatesChanged
    case notetypeRemoved
    case notetypeRestored
    case notesNotetypeChanged
    case presetRemoved
    case notetypesMerged
    case collectionReplaced
    case checkDatabaseRepair
    case syncSanityFailed
    case fullUpload
    case cloudkitStructureApplied
    case forceOneWay
    init(_ raw: MeguriCoreBindings.SchemaChangeReason) {
        switch raw {
        case .fieldsChanged: self = .fieldsChanged
        case .sortFieldChanged: self = .sortFieldChanged
        case .templatesChanged: self = .templatesChanged
        case .notetypeRemoved: self = .notetypeRemoved
        case .notetypeRestored: self = .notetypeRestored
        case .notesNotetypeChanged: self = .notesNotetypeChanged
        case .presetRemoved: self = .presetRemoved
        case .notetypesMerged: self = .notetypesMerged
        case .collectionReplaced: self = .collectionReplaced
        case .checkDatabaseRepair: self = .checkDatabaseRepair
        case .syncSanityFailed: self = .syncSanityFailed
        case .fullUpload: self = .fullUpload
        case .cloudkitStructureApplied: self = .cloudkitStructureApplied
        case .forceOneWay: self = .forceOneWay
        }
    }
}

public enum DeckRefusal: Sendable, Hashable {
    case filteredParent
    case tooDeep
    case underDescendant
    case filteredDeck
    case defaultPreset
    init(_ raw: MeguriCoreBindings.DeckRefusal) {
        switch raw {
        case .filteredParent: self = .filteredParent
        case .tooDeep: self = .tooDeep
        case .underDescendant: self = .underDescendant
        case .filteredDeck: self = .filteredDeck
        case .defaultPreset: self = .defaultPreset
        }
    }
}

public enum TemplateCheckKind: Sendable, Hashable {
    case duplicateFront
    case templateParseError
    case noFrontField
    case unknownField
    case missingCloze
    init(_ raw: MeguriCoreBindings.TemplateCheckKind) {
        switch raw {
        case .duplicateFront: self = .duplicateFront
        case .templateParseError: self = .templateParseError
        case .noFrontField: self = .noFrontField
        case .unknownField: self = .unknownField
        case .missingCloze: self = .missingCloze
        }
    }
}

public enum NotetypeTemplateSide: Sendable, Hashable {
    case front
    case back
    init(_ raw: MeguriCoreBindings.NotetypeTemplateSide) {
        switch raw {
        case .front: self = .front
        case .back: self = .back
        }
    }
}

public enum InvalidNotetypeReason: Sendable, Hashable {
    case noField, noTemplate, emptyName
    case emptyFieldName(ord: UInt64), emptyTemplateName(ord: UInt64)
    init(_ raw: MeguriCoreBindings.InvalidNotetypeReason) {
        switch raw {
        case .noField: self = .noField
        case .noTemplate: self = .noTemplate
        case .emptyName: self = .emptyName
        case .emptyFieldName(let ord): self = .emptyFieldName(ord: ord)
        case .emptyTemplateName(let ord): self = .emptyTemplateName(ord: ord)
        }
    }
}
public enum NotetypeRefusal: Sendable, Hashable {
    case invalid(reason: InvalidNotetypeReason)
    case template(kind: TemplateCheckKind, template: UInt64, side: NotetypeTemplateSide?, name: String?, earlier: UInt64?)
    init(_ raw: MeguriCoreBindings.NotetypeRefusal) {
        switch raw {
        case .invalid(let reason): self = .invalid(reason: InvalidNotetypeReason(reason))
        case .template(let kind, let template, let side, let name, let earlier):
            self = .template(kind: TemplateCheckKind(kind), template: template, side: side.map(NotetypeTemplateSide.init), name: name, earlier: earlier)
        }
    }
}

/// One change of a note type save's summary (`notetypes/draft-save` D2).
public enum NotetypeDraftChange: Sendable, Hashable {
    case fieldAdded(name: String)
    case fieldRemoved(ord: UInt32, name: String, notesWithContent: UInt64)
    case fieldRenamed(from: String, to: String)
    case fieldsReordered
    case sortFieldChanged(name: String)
    case templateAdded(name: String, cardsCreated: UInt64)
    case templateRemoved(ord: UInt32, name: String, cardsRemoved: UInt64, notesWithoutCards: UInt64)
    case templateRenamed(from: String, to: String)
    case templatesReordered
    case notetypeRenamed(from: String, to: String)
    case textOrStylingChanged
    init(_ raw: MeguriCoreBindings.NotetypeDraftChange) {
        switch raw {
        case .fieldAdded(let name): self = .fieldAdded(name: name)
        case .fieldRemoved(let ord, let name, let notes): self = .fieldRemoved(ord: ord, name: name, notesWithContent: notes)
        case .fieldRenamed(let from, let to): self = .fieldRenamed(from: from, to: to)
        case .fieldsReordered: self = .fieldsReordered
        case .sortFieldChanged(let name): self = .sortFieldChanged(name: name)
        case .templateAdded(let name, let cards): self = .templateAdded(name: name, cardsCreated: cards)
        case .templateRemoved(let ord, let name, let cards, let notes):
            self = .templateRemoved(ord: ord, name: name, cardsRemoved: cards, notesWithoutCards: notes)
        case .templateRenamed(let from, let to): self = .templateRenamed(from: from, to: to)
        case .templatesReordered: self = .templatesReordered
        case .notetypeRenamed(let from, let to): self = .notetypeRenamed(from: from, to: to)
        case .textOrStylingChanged: self = .textOrStylingChanged
        }
    }
}

/// A note type save's summary (`notetypes/draft-save` D2).
public struct NotetypeDraftSummary: Sendable, Hashable {
    public let reasons: [SchemaChangeReason]
    public let changes: [NotetypeDraftChange]
    public let losesContent: Bool
    init(_ raw: MeguriCoreBindings.NotetypeDraftSummary) {
        reasons = raw.reasons.map(SchemaChangeReason.init)
        changes = raw.changes.map(NotetypeDraftChange.init)
        losesContent = raw.losesContent
    }
}

/// Restore to Default's summary (`notetypes/restore-to-default` D7.1).
public struct NotetypeRestoreSummary: Sendable, Hashable {
    public let summary: NotetypeDraftSummary
    public let kindChangeCardsRemoved: UInt64
    init(_ raw: MeguriCoreBindings.NotetypeRestoreSummary) {
        summary = NotetypeDraftSummary(raw.summary)
        kindChangeCardsRemoved = raw.kindChangeCardsRemoved
    }
}

/// Why a note type structure op wrote nothing
/// (`notetypes/structure-changes` S1.2, S12; `restore-to-default` D7.2).
public enum NotetypeStructureRefusal: Sendable, Hashable {
    case protectedField(field: UInt32)
    case noteFieldCountMismatch(note: Int64)
    /// The losses grew past the confirmed ones: ask again with these.
    case lossesChanged(summary: NotetypeDraftSummary)
    case removalLossesChanged(notes: UInt64, cards: UInt64)
    case restoreLossesChanged(summary: NotetypeRestoreSummary)
    case lastNotetype
    case needsStockKind
    init(_ raw: MeguriCoreBindings.NotetypeStructureRefusal) {
        switch raw {
        case .protectedField(let field): self = .protectedField(field: field)
        case .noteFieldCountMismatch(let note): self = .noteFieldCountMismatch(note: note)
        case .lossesChanged(let summary): self = .lossesChanged(summary: NotetypeDraftSummary(summary))
        case .removalLossesChanged(let notes, let cards): self = .removalLossesChanged(notes: notes, cards: cards)
        case .restoreLossesChanged(let summary): self = .restoreLossesChanged(summary: NotetypeRestoreSummary(summary))
        case .lastNotetype: self = .lastNotetype
        case .needsStockKind: self = .needsStockKind
        }
    }
}
