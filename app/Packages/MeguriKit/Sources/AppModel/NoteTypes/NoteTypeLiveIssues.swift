import CoreBridge
import Foundation

/// One line of the Cards tab's live issues (`notetypes/workspace` W7): a
/// red failure of `notetypes/validation` V4, which blocks Save, or a yellow
/// warning of W8, which does not; each with its one-click fix.
public nonisolated struct NoteTypeLiveIssue: Identifiable, Hashable, Sendable {
    public enum Severity: Sendable, Hashable { case red, yellow }

    /// What the issue says (the view words it, W15).
    public enum Kind: Sendable, Hashable {
        case unknownField(name: String)
        case unclosedSection(tag: String)
        case unopenedSection(tag: String)
        case mismatchedSection(tag: String, open: String)
        case unmatchedSection
        /// The front repeats the card type named `earlier`.
        case duplicateFront(earlier: String)
        case noFrontField
        case missingCloze
        /// W8: the front is blank for `notes` notes; `field` decides it.
        case blankFront(notes: Int, field: String?)
    }

    public enum Fix: Sendable, Hashable {
        /// W7.1: every reference to `from` in the part names `to`.
        case useField(from: String, to: String)
        /// Appends `close` at the end of the part.
        case addClose(String)
        /// Opens Browse on these notes (`browser/workspace` V15).
        case showNotes([Int64])
    }

    public let id: String
    public let severity: Severity
    /// The card type's row (`NoteTypeCardTypeRow.id`).
    public let card: Int
    public let cardName: String
    /// Nil: the issue is about the whole card type.
    public let part: NoteTypeTemplatePart?
    public let line: Int?
    public let kind: Kind
    public let fix: Fix?

    public init(id: String, severity: Severity, card: Int, cardName: String, part: NoteTypeTemplatePart?, line: Int?, kind: Kind,
                fix: Fix?) {
        self.id = id
        self.severity = severity
        self.card = card
        self.cardName = cardName
        self.part = part
        self.line = line
        self.kind = kind
        self.fix = fix
    }
}

/// The red issues as read for one revision of the draft: positions are the
/// draft's at that read (its kept card types and fields).
public nonisolated struct NoteTypeIssueRead: Sendable, Hashable {
    public let cards: [Int]
    public let fields: [String]
    public let issues: [NotetypeTemplateIssue]
    /// The texts read, by card type row and part: a fix applies only to these.
    public let texts: [Int: [NoteTypeTemplatePart: String]]
    public init(cards: [Int], fields: [String], issues: [NotetypeTemplateIssue], texts: [Int: [NoteTypeTemplatePart: String]] = [:]) {
        self.cards = cards
        self.fields = fields
        self.issues = issues
        self.texts = texts
    }
}

/// W8 as read for one revision of the draft.
public nonisolated struct NoteTypeBlankRead: Sendable, Hashable {
    public let cards: [Int]
    public let fields: [String]
    public let fronts: [NotetypeBlankFront]
    public init(cards: [Int], fields: [String], fronts: [NotetypeBlankFront]) {
        self.cards = cards
        self.fields = fields
        self.fronts = fronts
    }
}

extension NoteTypeLiveIssue {
    /// The lines W7 shows: the red issues in V4's order, then the yellow
    /// ones in card type order. A read's positions name rows by the draft
    /// it was made for, so a line keeps its card type while the next read
    /// runs; one whose card type is gone is not shown.
    public static func lines(red: NoteTypeIssueRead?, yellow: NoteTypeBlankRead?, draft: NoteTypeDraft) -> [NoteTypeLiveIssue] {
        var lines: [NoteTypeLiveIssue] = []
        func name(_ row: Int) -> String? { draft.cardRow(row)?.name }
        if let red {
            for (index, issue) in red.issues.enumerated() {
                guard let row = red.cards[safe: Int(issue.template)], let cardName = name(row), draft.cardRow(row)?.deleted == false else {
                    continue
                }
                let part: NoteTypeTemplatePart? = issue.part.map { $0 == .front ? .front : .back }
                var fix: Fix?
                let kind: Kind
                switch issue.kind {
                case .unknownField(let unknown, let suggestion):
                    kind = .unknownField(name: unknown)
                    if let suggestion, let to = red.fields[safe: Int(suggestion)] { fix = .useField(from: unknown, to: to) }
                case .unclosedSection(let tag, let close):
                    kind = .unclosedSection(tag: tag)
                    fix = .addClose(close)
                case .unopenedSection(let tag): kind = .unopenedSection(tag: tag)
                case .mismatchedSection(let tag, let open): kind = .mismatchedSection(tag: tag, open: open)
                case .unmatchedSection: kind = .unmatchedSection
                case .duplicateFront(let earlier):
                    kind = .duplicateFront(earlier: red.cards[safe: Int(earlier)].flatMap(name) ?? "")
                case .noFrontField: kind = .noFrontField
                case .missingCloze: kind = .missingCloze
                }
                lines.append(NoteTypeLiveIssue(id: "red.\(index).\(row)", severity: .red, card: row, cardName: cardName, part: part,
                                               line: issue.line.map(Int.init), kind: kind, fix: fix))
            }
        }
        if let yellow {
            for (index, front) in yellow.fronts.enumerated() where !front.noteIds.isEmpty {
                guard let row = yellow.cards[safe: index], let cardName = name(row), draft.cardRow(row)?.deleted == false else { continue }
                let field = front.field.flatMap { yellow.fields[safe: Int($0)] }
                lines.append(NoteTypeLiveIssue(id: "yellow.\(row)", severity: .yellow, card: row, cardName: cardName, part: .front, line: nil,
                                               kind: .blankFront(notes: front.noteIds.count, field: field), fix: .showNotes(front.noteIds)))
            }
        }
        return lines
    }
}
