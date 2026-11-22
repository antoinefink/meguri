import Foundation
import Instrumentation
internal import MeguriCoreBindings

// The Cards tab's core reads (U282, `notetypes/workspace` W5 to W7): each
// card type's cards (a job on one snapshot), the red live issues of a draft
// as it would be saved (one snapshot, no note read), W7's "Use “<name>”"
// fix, and the editor's colouring of a template or of the styling. Ranges
// are UTF-16 offsets, the text views' own unit. The colouring and the fix
// follow typing, so they run on the interactive lane.

/// The part of a card type an issue is about.
public enum NotetypeTemplatePart: Sendable, Hashable {
    case front, back
    init(_ raw: MeguriCoreBindings.NotetypeTemplatePart) {
        switch raw {
        case .front: self = .front
        case .back: self = .back
        }
    }
}

/// What a red issue says (W7), with what its fix needs.
public enum NotetypeTemplateIssueKind: Sendable, Hashable {
    /// The front repeats the card type at `earlier` (a draft position).
    case duplicateFront(earlier: UInt32)
    /// A section left open: `tag` as written; `close` the tag the fix appends.
    case unclosedSection(tag: String, close: String)
    case unopenedSection(tag: String)
    case mismatchedSection(tag: String, open: String)
    case unmatchedSection
    case noFrontField
    /// `suggestion`: the position in the draft's fields of W7.1's near name.
    case unknownField(name: String, suggestion: UInt32?)
    case missingCloze
    init(_ raw: MeguriCoreBindings.NotetypeTemplateIssueKind) {
        switch raw {
        case .duplicateFront(let earlier): self = .duplicateFront(earlier: earlier)
        case .unclosedSection(let tag, let close): self = .unclosedSection(tag: tag, close: close)
        case .unopenedSection(let tag): self = .unopenedSection(tag: tag)
        case .mismatchedSection(let tag, let open): self = .mismatchedSection(tag: tag, open: open)
        case .unmatchedSection: self = .unmatchedSection
        case .noFrontField: self = .noFrontField
        case .unknownField(let name, let suggestion): self = .unknownField(name: name, suggestion: suggestion)
        case .missingCloze: self = .missingCloze
        }
    }
}

/// One red issue, in `notetypes/validation` V4's order.
public struct NotetypeTemplateIssue: Sendable, Hashable {
    /// The card type's position in the draft's templates.
    public let template: UInt32
    public let part: NotetypeTemplatePart?
    /// The 1-based line of the part's draft text; nil for the whole part.
    public let line: UInt32?
    public let kind: NotetypeTemplateIssueKind
    public init(template: UInt32, part: NotetypeTemplatePart?, line: UInt32?, kind: NotetypeTemplateIssueKind) {
        self.template = template
        self.part = part
        self.line = line
        self.kind = kind
    }
    init(_ raw: MeguriCoreBindings.NotetypeTemplateIssue) {
        self.init(template: raw.template, part: raw.part.map(NotetypeTemplatePart.init), line: raw.line, kind: .init(raw.kind))
    }
}

/// The kind of a coloured range of the editor (W6).
public enum NotetypeSyntaxKind: Sendable, Hashable {
    case field, section, comment, tag, directive, atRule, property, value
    /// A colour value with the colour its swatch shows.
    case colour(CardTextColour)
    init(_ raw: MeguriCoreBindings.NotetypeSyntaxKind) {
        switch raw {
        case .field: self = .field
        case .section: self = .section
        case .comment: self = .comment
        case .tag: self = .tag
        case .directive: self = .directive
        case .atRule: self = .atRule
        case .property: self = .property
        case .value: self = .value
        case .colour(let colour): self = .colour(CardTextColour(colour))
        }
    }
}

/// One coloured range, in UTF-16 code units of the text.
public struct NotetypeSyntaxSpan: Sendable, Hashable {
    public let start: Int
    public let end: Int
    public let kind: NotetypeSyntaxKind
    public init(start: Int, end: Int, kind: NotetypeSyntaxKind) {
        self.start = start
        self.end = end
        self.kind = kind
    }
    init(_ raw: MeguriCoreBindings.NotetypeSyntaxSpan) {
        self.init(start: Int(raw.start), end: Int(raw.end), kind: .init(raw.kind))
    }
}

extension CoreNotetypeService {
    /// W5's counts: per stored template ordinal, its cards.
    public func startNotetypeTemplateCardCounts(notetypeId: Int64) async throws(BridgeError) -> CoreNotetypeReadJob<[UInt64]> {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startNotetypeTemplateCardCounts) {
            let raw = try MeguriCoreBindings.startNotetypeTemplateCardCounts(notetypeId: notetypeId)
            let result = raw.result
            return CoreNotetypeReadJob(job: try Jobs.track(raw.handle), resultCall: .notetypeTemplateCardCountsJobResult) {
                try result.result()
            }
        }
    }

    /// W7's red issues of the draft as it would be saved.
    public func notetypeTemplateIssues(_ draft: NotetypeStructureDraft) async throws(BridgeError) -> [NotetypeTemplateIssue] {
        let request = draft.raw
        return try await QueryLane.shared.call(.notetypeTemplateIssues) {
            try MeguriCoreBindings.notetypeTemplateIssues(draft: request).map(NotetypeTemplateIssue.init)
        }
    }

    /// W7's "Use “<to>”": every reference to `from` in one part's text names `to`.
    public func notetypeReplaceFieldReferences(_ text: String, from: String, to: String) async throws(BridgeError) -> String {
        try await InteractiveLane.shared.call(.notetypeReplaceFieldReferences) {
            try MeguriCoreBindings.notetypeReplaceFieldReferences(text: text, from: from, to: to)
        }
    }

    /// W6's colouring of a Front or Back text.
    public func notetypeTemplateSyntax(_ text: String) async throws(BridgeError) -> [NotetypeSyntaxSpan] {
        try await InteractiveLane.shared.call(.notetypeTemplateSyntax) {
            try MeguriCoreBindings.notetypeTemplateSyntax(text: text).map(NotetypeSyntaxSpan.init)
        }
    }

    /// W6's colouring of the styling, each colour value with its colour.
    public func notetypeStylingSyntax(_ css: String) async throws(BridgeError) -> [NotetypeSyntaxSpan] {
        try await InteractiveLane.shared.call(.notetypeStylingSyntax) {
            try MeguriCoreBindings.notetypeStylingSyntax(css: css).map(NotetypeSyntaxSpan.init)
        }
    }
}
