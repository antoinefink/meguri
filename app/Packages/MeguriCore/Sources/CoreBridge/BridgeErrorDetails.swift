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
