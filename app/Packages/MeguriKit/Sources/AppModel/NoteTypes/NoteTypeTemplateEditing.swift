import Foundation

/// One text edit the template editor applies: replace `range` (UTF-16
/// offsets of the shown text) with `text`, then put the insertion point at
/// `caret` (a UTF-16 offset in the new text). The text view applies it, so
/// it joins the text view's own undo (W13).
public nonisolated struct NoteTypeTextEdit: Sendable, Hashable {
    public let range: Range<Int>
    public let text: String
    public let caret: Int
    public init(range: Range<Int>, text: String, caret: Int) {
        self.range = range
        self.text = text
        self.caret = caret
    }
}

/// The template editor's text helpers (`notetypes/workspace` W6,
/// `notetypes/editor-actions` A14, A15): completion after `{{`, the Insert
/// menu, ⌘/ and Flip. Pure functions of the shown text and selection.
public nonisolated enum NoteTypeTemplateEditing {
    /// `render/special-fields` P1's names, in the order W6 lists them.
    public static let specialFields = ["FrontSide", "Tags", "Deck", "Subdeck", "Card", "CardFlag", "CardID", "Type"]
    /// The filter prefixes after which completion lists the fields again (W6).
    public static let filters = ["hint", "type", "cloze", "furigana", "text"]
    /// A15's separator, exactly.
    public static let answerDivider = "<hr id=answer>"

    // MARK: Completion (W6)

    /// What typing after `{{` completes: the typed part of the name, where
    /// it starts, and whether accepting adds the matching close.
    public struct CompletionContext: Sendable, Hashable {
        /// The UTF-16 range of the name typed so far (it ends at the caret).
        public let range: Range<Int>
        public let typed: String
        /// `{{#` or `{{^`: accepting also inserts `{{/<name>}}` after the caret.
        public let section: Bool
        /// After a filter prefix (`hint:`): fields only, no special names.
        public let afterFilter: Bool
        public init(range: Range<Int>, typed: String, section: Bool, afterFilter: Bool) {
            self.range = range
            self.typed = typed
            self.section = section
            self.afterFilter = afterFilter
        }
    }

    /// The completion context at `caret`, or nil when the caret is not
    /// right after `{{`, a section marker or a filter prefix and a name
    /// being typed on the same line.
    public static func completionContext(_ text: String, caret: Int) -> CompletionContext? {
        let units = Array(text.utf16)
        guard caret >= 0, caret <= units.count else { return nil }
        func ends(_ unit: UInt16) -> Bool { unit == 0x7B || unit == 0x7D || unit == 0x3A || unit == 0x0A || unit == 0x0D }
        // Back over the piece typed so far: no brace, colon or line break.
        var start = caret
        while start > 0, !ends(units[start - 1]) { start -= 1 }
        var nameStart = start
        // `{{#` and `{{^` open a section; a close (`{{/`) completes nothing.
        let section = nameStart < caret && (units[nameStart] == 0x23 || units[nameStart] == 0x5E)
        if section { nameStart += 1 } else if nameStart < caret, units[nameStart] == 0x2F { return nil }
        // Filter pieces (`hint:`) before the name, each a filter and a colon.
        var afterFilter = false
        var at = start
        while at > 0, units[at - 1] == 0x3A {
            guard !section else { return nil }
            var pieceStart = at - 1
            while pieceStart > 0, !ends(units[pieceStart - 1]) { pieceStart -= 1 }
            let piece = String(decoding: units[pieceStart..<(at - 1)], as: UTF16.self)
            guard filters.contains(piece.trimmingCharacters(in: .whitespaces)) else { return nil }
            afterFilter = true
            at = pieceStart
        }
        // Then `{{` (a third `{` is a triple-brace replacement, still a name).
        guard at >= 2, units[at - 1] == 0x7B, units[at - 2] == 0x7B else { return nil }
        while nameStart < caret, units[nameStart] == 0x20 { nameStart += 1 }
        let typed = String(decoding: units[nameStart..<caret], as: UTF16.self)
        return CompletionContext(range: nameStart..<caret, typed: typed, section: section, afterFilter: afterFilter)
    }

    /// The names completion lists for `context`: the draft's fields in
    /// order, then (not after a filter) the special names, each whose start
    /// matches what was typed, ignoring case.
    public static func completions(_ context: CompletionContext, fields: [String]) -> [String] {
        let names = context.afterFilter ? fields : fields + specialFields.filter { !fields.contains($0) }
        guard !context.typed.isEmpty else { return names }
        return names.filter { $0.range(of: context.typed, options: [.caseInsensitive, .anchored]) != nil }
    }

    /// Accepting `name`: it replaces the typed part, then `}}` unless the
    /// text already has it there; after `{{#` or `{{^` the matching
    /// `{{/<name>}}` follows the caret.
    public static func accept(_ name: String, context: CompletionContext, in text: String) -> NoteTypeTextEdit {
        let units = Array(text.utf16)
        // Inside a name already written (`{{F|ront}}`), the rest of it goes too.
        var end = context.range.upperBound
        while end < units.count, ![0x7B, 0x7D, 0x3A, 0x0A, 0x0D].contains(units[end]) { end += 1 }
        let after = units[end...].prefix(2)
        let closed = Array(after) == [0x7D, 0x7D]
        var inserted = name + (closed ? "" : "}}")
        var caret = context.range.lowerBound + inserted.utf16.count + (closed ? 2 : 0)
        var range = context.range.lowerBound..<end
        if context.section {
            if closed {
                // The open tag is already closed: the close goes after it.
                range = context.range.lowerBound..<(end + 2)
                inserted = name + "}}"
                caret = context.range.lowerBound + inserted.utf16.count
            }
            inserted += "{{/\(name)}}"
        }
        return NoteTypeTextEdit(range: range, text: inserted, caret: caret)
    }

    // MARK: The Insert menu (W6, A14)

    /// T2's alternate mode and its lead (A14): the text, after White_Space
    /// at its start, begins with `{{=<% %>=}}`; the lead is that White_Space
    /// and every repeated directive.
    public static func alternateLead(_ text: String) -> Int? {
        let directive = "{{=<% %>=}}"
        let trimmed = text.drop { $0.isWhitespace }
        guard trimmed.hasPrefix(directive) else { return nil }
        var rest = Substring(trimmed)
        while rest.hasPrefix(directive) { rest = rest.dropFirst(directive.count) }
        return text.utf16.count - rest.utf16.count
    }

    /// The delimiters of the shown part.
    static func delimiters(_ text: String) -> (open: String, close: String) {
        alternateLead(text) == nil ? ("{{", "}}") : ("<%", "%>")
    }

    /// Whether a field can be referenced in the shown part: in alternate
    /// mode a name holding `%>` would end the tag early (A14).
    public static func canReference(_ name: String, in text: String) -> Bool {
        alternateLead(text) == nil || !name.contains("%>")
    }

    /// What an Insert menu item puts in.
    public enum Item: Sendable, Hashable {
        case field(String)
        case showIfContent(String)
        case showIfEmpty(String)
        case filter(String, String)
        case answerDivider
        case darkRule
    }

    /// The edit an Insert menu item makes at `selection` (A14): the tag in
    /// the part's delimiters replaces the selection, except that the two
    /// conditionals wrap it; never inside the alternate-mode lead. The Dark
    /// Appearance Rule goes at the end of the styling.
    public static func insert(_ item: Item, in text: String, selection: Range<Int>) -> NoteTypeTextEdit {
        let length = text.utf16.count
        if case .darkRule = item {
            // On a line of its own after a blank line; the caret inside the block.
            let opening = ".card.nightMode {\n"
            let lead = text.isEmpty ? "" : (text.hasSuffix("\n") ? "\n" : "\n\n")
            return NoteTypeTextEdit(range: length..<length, text: lead + opening + "}",
                                    caret: length + lead.utf16.count + opening.utf16.count)
        }
        let (open, close) = delimiters(text)
        var range = selection.clamped(to: 0..<length)
        if let lead = alternateLead(text), range.lowerBound < lead { range = lead..<lead }
        let units = Array(text.utf16)
        let selected = String(utf16CodeUnits: Array(units[range]), count: range.count)
        let tag: (String) -> String = { open + $0 + close }
        switch item {
        case .field(let name):
            let inserted = tag(name)
            return NoteTypeTextEdit(range: range, text: inserted, caret: range.lowerBound + inserted.utf16.count)
        case .filter(let filter, let name):
            let inserted = tag("\(filter):\(name)")
            return NoteTypeTextEdit(range: range, text: inserted, caret: range.lowerBound + inserted.utf16.count)
        case .showIfContent(let name), .showIfEmpty(let name):
            let marker = if case .showIfContent = item { "#" } else { "^" }
            let opening = tag(marker + name)
            let inserted = opening + selected + tag("/" + name)
            // The caret goes inside the section, after what it wraps.
            return NoteTypeTextEdit(range: range, text: inserted, caret: range.lowerBound + opening.utf16.count + selected.utf16.count)
        case .answerDivider:
            return NoteTypeTextEdit(range: range, text: answerDivider, caret: range.lowerBound + answerDivider.utf16.count)
        case .darkRule:
            return NoteTypeTextEdit(range: range, text: "", caret: range.lowerBound)
        }
    }

    // MARK: ⌘/ (W6)

    /// ⌘/ on the lines the selection touches: each non-blank line is wrapped
    /// in `<!-- -->` (Front, Back) or `/* */` (Styling) after its
    /// indentation; when every non-blank line already is, they are unwrapped.
    public static func toggleComment(_ text: String, selection: Range<Int>, styling: Bool) -> NoteTypeTextEdit {
        let (open, close) = styling ? ("/* ", " */") : ("<!-- ", " -->")
        let ns = text as NSString
        let clamped = selection.clamped(to: 0..<ns.length)
        let lines = ns.lineRange(for: NSRange(location: clamped.lowerBound, length: clamped.count))
        let block = ns.substring(with: lines)
        let endsWithBreak = block.utf16.last == 0x0A
        var parts = block.components(separatedBy: "\n")
        if endsWithBreak { parts.removeLast() }
        // A CR LF line keeps its CR after the comment's close.
        let returns = parts.map { $0.utf16.last == 0x0D }
        parts = zip(parts, returns).map { $1 ? String(String.UnicodeScalarView($0.unicodeScalars.dropLast())) : $0 }
        func split(_ line: String) -> (indent: Substring, body: Substring) {
            let indent = line.prefix { $0 == " " || $0 == "\t" }
            return (indent, line.dropFirst(indent.count))
        }
        let open0 = open.trimmingCharacters(in: .whitespaces), close0 = close.trimmingCharacters(in: .whitespaces)
        let bodies = parts.map { split($0).body.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        let commented = !bodies.isEmpty && bodies.allSatisfy { $0.hasPrefix(open0) && $0.hasSuffix(close0) && $0.count >= open0.count + close0.count }
        let rewritten = parts.map { line -> String in
            let (indent, body) = split(line)
            guard !body.trimmingCharacters(in: .whitespaces).isEmpty else { return line }
            if commented {
                var inner = body.trimmingCharacters(in: .whitespaces)
                inner.removeFirst(open0.count)
                inner.removeLast(close0.count)
                if inner.hasPrefix(" ") { inner.removeFirst() }
                if inner.hasSuffix(" ") { inner.removeLast() }
                return String(indent) + inner
            }
            return String(indent) + open + body + close
        }
        let replacement = zip(rewritten, returns).map { $1 ? $0 + "\r" : $0 }.joined(separator: "\n") + (endsWithBreak ? "\n" : "")
        let range = lines.location..<(lines.location + lines.length)
        // The insertion point ends the last line it rewrote.
        let caret = range.lowerBound + replacement.utf16.count - (endsWithBreak ? 1 : 0) - (endsWithBreak && returns.last == true ? 1 : 0)
        return NoteTypeTextEdit(range: range, text: replacement, caret: caret)
    }

    // MARK: Flip (A15)

    /// A15: the new front and back, or nil when the back has no separator
    /// with at least one character on each side (nothing changes).
    public static func flip(front: String, back: String) -> (front: String, back: String)? {
        var search = back.startIndex..<back.endIndex
        var chosen: Range<String.Index>?
        while let found = back.range(of: answerDivider, range: search) {
            if found.lowerBound > back.startIndex, found.upperBound < back.endIndex { chosen = found }
            search = found.upperBound..<back.endIndex
        }
        guard let chosen else { return nil }
        let newFront = back[chosen.upperBound...].trimmingCharacters(in: .whitespacesAndNewlines)
        return (newFront, "{{FrontSide}}\n\n\(answerDivider)\n\n" + front)
    }

    // MARK: Lines

    /// The UTF-16 offset where 1-based `line` starts (W7: clicking an issue
    /// moves the caret to its line): LF, CR LF and a lone CR end a line, as
    /// the core counts them. The end of the text past the last line.
    public static func offset(ofLine line: Int, in text: String) -> Int {
        let units = Array(text.utf16)
        var current = 1
        var at = 0
        while current < line, at < units.count {
            let unit = units[at]
            at += 1
            if unit == 0x0D, at < units.count, units[at] == 0x0A { at += 1 }
            if unit == 0x0A || unit == 0x0D { current += 1 }
        }
        return at
    }
}

/// A command for the template editor that the text view applies, so the
/// change joins its own undo (W13): an Insert menu item, ⌘/, a fix, a
/// field dropped from the Fields list, or a caret move to an issue's line.
public nonisolated struct NoteTypeEditorRequest: Sendable, Hashable {
    public enum Command: Sendable, Hashable {
        case insert(NoteTypeTemplateEditing.Item)
        case toggleComment
        /// The whole part's text becomes this (W7's "Use “<name>”").
        case replaceAll(String)
        /// W10's Card Text row: the styling becomes this, and the value it
        /// wrote (UTF-16 offsets in the new text) is highlighted briefly.
        case replaceHighlighting(String, highlight: Range<Int>)
        /// Appended at the end of the part (W7's "Add {{/<x>}}").
        case append(String)
        /// The caret goes to the start of this 1-based line (W7).
        case moveCaret(line: Int)
    }
    public let token: UInt64
    public let notetype: Int64
    public let card: Int
    public let part: NoteTypeTemplatePart
    public let command: Command
}
