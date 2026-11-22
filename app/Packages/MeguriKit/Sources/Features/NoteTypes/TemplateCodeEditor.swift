import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The text a template editor shows: one card type's front or back, or the
/// note type's shared styling.
struct TemplateEditorKey: Hashable {
    let notetype: Int64
    let card: Int
    let part: NoteTypeTemplatePart
}

/// One entry of the Insert menu (W6), shared by the editor bar's menu and
/// ⌃Space's menu at the insertion point.
enum InsertMenuEntry: Hashable {
    case item(title: String, item: NoteTypeTemplateEditing.Item, enabled: Bool)
    case submenu(title: String, entries: [InsertMenuEntry])

    /// W6's Insert menu for a part: in Front and Back the field and tag
    /// items (A14: a field whose name would end an alternate-mode tag is
    /// disabled); in Styling the Dark Appearance Rule.
    static func menu(part: NoteTypeTemplatePart, text: String, fields: [String], locale: Locale) -> [InsertMenuEntry] {
        func title(_ key: String.LocalizationValue) -> String { NoteTypesStrings.text(key, locale: locale) }
        guard part != .styling else { return [.item(title: title("noteTypes.insert.darkRule"), item: .darkRule, enabled: true)] }
        func names(_ item: (String) -> NoteTypeTemplateEditing.Item) -> [InsertMenuEntry] {
            fields.map { .item(title: $0, item: item($0), enabled: NoteTypeTemplateEditing.canReference($0, in: text)) }
        }
        return [
            .submenu(title: title("noteTypes.insert.field"), entries: names { .field($0) }),
            .submenu(title: title("noteTypes.insert.special"),
                     entries: NoteTypeTemplateEditing.specialFields.map { .item(title: $0, item: .field($0), enabled: true) }),
            .submenu(title: title("noteTypes.insert.ifContent"), entries: names { .showIfContent($0) }),
            .submenu(title: title("noteTypes.insert.ifEmpty"), entries: names { .showIfEmpty($0) }),
            .submenu(title: title("noteTypes.insert.hint"), entries: names { .filter("hint", $0) }),
            .submenu(title: title("noteTypes.insert.typeIn"), entries: names { .filter("type", $0) }),
            .submenu(title: title("noteTypes.insert.textOnly"), entries: names { .filter("text", $0) }),
            .submenu(title: title("noteTypes.insert.furigana"), entries: names { .filter("furigana", $0) }),
            .item(title: title("noteTypes.insert.divider"), item: .answerDivider, enabled: true),
        ]
    }
}

/// The colour each coloured range takes (W6). System colours only: they
/// follow light, dark and increased contrast.
enum TemplateSyntaxColour {
    case field, section, comment, tag, directive, atRule, property

    init?(_ kind: NotetypeSyntaxKind) {
        switch kind {
        case .field: self = .field
        case .section: self = .section
        case .comment: self = .comment
        case .tag: self = .tag
        case .directive: self = .directive
        case .atRule: self = .atRule
        case .property: self = .property
        case .value, .colour: return nil
        }
    }
}

/// W6's swatch edit: a colour value is written `#rrggbb` in lower case, or
/// `#rrggbbaa` when it is not opaque (as W10.2 writes a text colour).
enum SwatchHex {
    static func string(red: Double, green: Double, blue: Double, alpha: Double) -> String {
        func byte(_ value: Double) -> Int { Int((min(max(value, 0), 1) * 255).rounded()) }
        let rgb = String(format: "#%02x%02x%02x", byte(red), byte(green), byte(blue))
        return byte(alpha) == 255 ? rgb : rgb + String(format: "%02x", byte(alpha))
    }
}

#if os(macOS)
import AppKit

extension TemplateSyntaxColour {
    var colour: NSColor {
        switch self {
        case .field: .systemPurple
        case .section: .systemOrange
        case .comment: .secondaryLabelColor
        case .tag: .systemBlue
        case .directive: .systemTeal
        case .atRule: .systemPink
        case .property: .systemTeal
        }
    }
}

/// The template editor's text view on the Mac (W6): the stored text byte
/// for byte, plain quotes, no correction, Tab indents, ⌘/ comments the
/// line, the system find bar (⌘F), completion after `{{`, ⌃Space's Insert
/// menu, colouring from the core, and a colour swatch on each colour value
/// of the styling. Each text keeps its own undo history.
struct TemplateCodeEditor: NSViewRepresentable {
    let key: TemplateEditorKey
    let text: String
    let editable: Bool
    let fields: [String]
    let request: NoteTypeEditorRequest?
    let insertMenu: [InsertMenuEntry]
    let accessibilityLabel: String
    let onChange: (String) -> Void
    let onConsume: (UInt64) -> Void
    let onFlip: (() -> Void)?
    let syntax: (String, Bool) async -> [NotetypeSyntaxSpan]

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .noBorder
        let textView = CodeTextView(usingTextLayoutManager: false)
        textView.coordinator = context.coordinator
        textView.isRichText = false
        textView.importsGraphics = false
        textView.allowsUndo = true
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isAutomaticLinkDetectionEnabled = false
        textView.isAutomaticDataDetectionEnabled = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isGrammarCheckingEnabled = false
        textView.smartInsertDeleteEnabled = false
        textView.font = Coordinator.font
        textView.typingAttributes = Coordinator.plain
        textView.textContainerInset = NSSize(width: Spacing.xxSmall, height: Spacing.xSmall)
        textView.minSize = .zero
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.textContainer?.widthTracksTextView = true
        textView.delegate = context.coordinator
        textView.setAccessibilityIdentifier("noteTypes.cards.editor")
        scroll.documentView = textView
        context.coordinator.textView = textView
        context.coordinator.parent = self
        context.coordinator.show(key: key, text: text)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let coordinator = context.coordinator
        coordinator.parent = self
        guard let textView = coordinator.textView else { return }
        textView.isEditable = editable
        textView.setAccessibilityLabel(accessibilityLabel)
        if coordinator.key != key || textView.string != text {
            coordinator.show(key: key, text: text)
        }
        if let request, request.notetype == key.notetype, request.card == key.card, request.part == key.part,
           coordinator.applied != request.token {
            coordinator.applied = request.token
            coordinator.apply(request.command)
            let token = request.token
            DispatchQueue.main.async { onConsume(token) }
        }
    }

    @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
        /// The person's fixed-pitch font at its own size (Settings › Fonts).
        static let font = NSFont.userFixedPitchFont(ofSize: 0) ?? NSFont.preferredFont(forTextStyle: .body)
        static let plain: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.labelColor]
        /// The room a colour swatch takes before its value, and its side.
        static let well: CGFloat = 16

        weak var textView: CodeTextView?
        var parent: TemplateCodeEditor?
        var key: TemplateEditorKey?
        var applied: UInt64?
        private var undoManagers: [TemplateEditorKey: UndoManager] = [:]
        private var selections: [TemplateEditorKey: NSRange] = [:]
        private var generation: UInt64 = 0
        /// The colour values of the shown styling, as last coloured.
        private var colours: [NSRange] = []
        private var swatches: [NSColorWell] = []
        /// The completion the text view is showing.
        var completion: NoteTypeTemplateEditing.CompletionContext?

        /// Shows another text, or the same one changed elsewhere (Revert,
        /// Flip): the text view's history for it no longer applies.
        func show(key newKey: TemplateEditorKey, text: String) {
            guard let textView else { return }
            if let key, key != newKey { selections[key] = textView.selectedRange() }
            let sameKey = key == newKey
            key = newKey
            if textView.string != text {
                if sameKey { undoManagers[newKey]?.removeAllActions() }
                textView.string = text
            }
            let length = (text as NSString).length
            let selection = sameKey ? textView.selectedRange() : (selections[newKey] ?? NSRange(location: 0, length: 0))
            textView.setSelectedRange(NSRange(location: min(selection.location, length), length: 0))
            recolour()
        }

        func undoManager(for view: NSTextView) -> UndoManager? {
            guard let key else { return nil }
            if let manager = undoManagers[key] { return manager }
            let manager = UndoManager()
            undoManagers[key] = manager
            return manager
        }

        func textDidChange(_ notification: Notification) {
            guard let textView else { return }
            parent?.onChange(textView.string)
            recolour()
            // W6: typing `{{`, `{{#`, `{{^` or a filter prefix lists the names.
            let caret = textView.selectedRange().location
            // A completion's preview changes the text too: it starts no other completion.
            if key?.part != .styling, !textView.hasMarkedText(), !textView.completing,
               NoteTypeTemplateEditing.completionContext(textView.string, caret: caret) != nil {
                DispatchQueue.main.async { [weak textView] in textView?.complete(nil) }
            }
        }

        func textView(_ textView: NSTextView, completions words: [String], forPartialWordRange charRange: NSRange,
                      indexOfSelectedItem index: UnsafeMutablePointer<Int>?) -> [String] {
            guard let completion else { return [] }
            index?.pointee = 0
            return NoteTypeTemplateEditing.completions(completion, fields: parent?.fields ?? [])
        }

        /// A command from the model, as an edit the text view's undo records.
        func apply(_ command: NoteTypeEditorRequest.Command) {
            guard let textView else { return }
            let text = textView.string
            let selection = textView.selectedRange()
            let range = selection.location..<(selection.location + selection.length)
            let edit: NoteTypeTextEdit
            switch command {
            case .insert(let item): edit = NoteTypeTemplateEditing.insert(item, in: text, selection: range)
            case .toggleComment: edit = NoteTypeTemplateEditing.toggleComment(text, selection: range, styling: key?.part == .styling)
            case .replaceAll(let replacement):
                let length = (text as NSString).length
                edit = NoteTypeTextEdit(range: 0..<length, text: replacement, caret: min(selection.location, (replacement as NSString).length))
            case .append(let suffix):
                let length = (text as NSString).length
                edit = NoteTypeTextEdit(range: length..<length, text: suffix, caret: length + (suffix as NSString).length)
            case .moveCaret(let line):
                let offset = NoteTypeTemplateEditing.offset(ofLine: line, in: text)
                textView.window?.makeFirstResponder(textView)
                textView.setSelectedRange(NSRange(location: offset, length: 0))
                textView.scrollRangeToVisible(NSRange(location: offset, length: 0))
                return
            }
            perform(edit)
        }

        func perform(_ edit: NoteTypeTextEdit) {
            guard let textView, textView.isEditable else { return }
            let range = NSRange(location: edit.range.lowerBound, length: edit.range.count)
            guard textView.shouldChangeText(in: range, replacementString: edit.text) else { return }
            textView.textStorage?.replaceCharacters(in: range, with: NSAttributedString(string: edit.text, attributes: Self.plain))
            textView.didChangeText()
            textView.setSelectedRange(NSRange(location: edit.caret, length: 0))
            textView.scrollRangeToVisible(NSRange(location: edit.caret, length: 0))
            textView.window?.makeFirstResponder(textView)
        }

        /// The core's colouring of the text as it is now; an older answer
        /// for a text since changed is dropped.
        func recolour() {
            guard let textView, let parent else { return }
            generation &+= 1
            let generation = generation
            let text = textView.string
            let styling = key?.part == .styling
            Task { @MainActor [weak self] in
                let spans = await parent.syntax(text, styling)
                guard let self, self.generation == generation, let textView = self.textView, textView.string == text,
                      !textView.hasMarkedText() else { return }
                self.colour(spans)
            }
        }

        private func colour(_ spans: [NotetypeSyntaxSpan]) {
            guard let textView, let storage = textView.textStorage else { return }
            let length = storage.length
            storage.beginEditing()
            storage.setAttributes(Self.plain, range: NSRange(location: 0, length: length))
            var colours: [(NSRange, CardTextColour)] = []
            for span in spans where span.start < span.end && span.end <= length {
                let range = NSRange(location: span.start, length: span.end - span.start)
                if let colour = TemplateSyntaxColour(span.kind) {
                    storage.addAttribute(.foregroundColor, value: colour.colour, range: range)
                } else if case .colour(let value) = span.kind, span.start > 0 {
                    // Room for the well before the value: the character before it is wider.
                    storage.addAttribute(.kern, value: Self.well + Spacing.xxSmall, range: NSRange(location: span.start - 1, length: 1))
                    colours.append((range, value))
                }
            }
            storage.endEditing()
            self.colours = colours.map(\.0)
            layoutSwatches(colours)
        }

        private func layoutSwatches(_ colours: [(NSRange, CardTextColour)]) {
            guard let textView, let manager = textView.layoutManager, let container = textView.textContainer else { return }
            manager.ensureLayout(for: container)
            while swatches.count < colours.count {
                let well = NSColorWell(style: .minimal)
                well.target = self
                well.action = #selector(wellChanged(_:))
                well.isBordered = false
                textView.addSubview(well)
                swatches.append(well)
            }
            for (index, well) in swatches.enumerated() {
                guard index < colours.count else { well.isHidden = true; continue }
                let (range, value) = colours[index]
                let glyphs = manager.glyphRange(forCharacterRange: NSRange(location: range.location, length: 1), actualCharacterRange: nil)
                let rect = manager.boundingRect(forGlyphRange: glyphs, in: container)
                let origin = textView.textContainerOrigin
                well.frame = NSRect(x: origin.x + rect.minX - Self.well - Spacing.xxSmall / 2,
                                    y: origin.y + rect.midY - Self.well / 2, width: Self.well, height: Self.well)
                well.color = .swatch(value)
                well.tag = index
                well.isEnabled = textView.isEditable
                well.isHidden = false
                well.setAccessibilityIdentifier("noteTypes.cards.swatch.\(index)")
            }
        }

        /// A colour chosen in a well rewrites only that value's characters.
        @objc private func wellChanged(_ well: NSColorWell) {
            guard colours.indices.contains(well.tag), let colour = well.color.usingColorSpace(.sRGB) else { return }
            let range = colours[well.tag]
            let hex = SwatchHex.string(red: colour.redComponent, green: colour.greenComponent, blue: colour.blueComponent, alpha: colour.alphaComponent)
            colours[well.tag] = NSRange(location: range.location, length: (hex as NSString).length)
            perform(NoteTypeTextEdit(range: range.location..<(range.location + range.length), text: hex,
                                     caret: range.location + (hex as NSString).length))
        }

        /// The text wrapped anew: the swatches follow their values.
        func relayoutSwatches() {
            if !colours.isEmpty { recolour() }
        }
    }
}

/// The editor's text view: completion over the tag being typed, ⌘/,
/// ⌃Space's Insert menu and Flip in the Back's context menu.
final class CodeTextView: NSTextView {
    weak var coordinator: TemplateCodeEditor.Coordinator?
    /// The completion list shows and previews its highlighted name.
    private(set) var completing = false

    override var rangeForUserCompletion: NSRange {
        let caret = selectedRange().location
        guard coordinator?.key?.part != .styling,
              let context = NoteTypeTemplateEditing.completionContext(string, caret: caret) else {
            coordinator?.completion = nil
            return NSRange(location: NSNotFound, length: 0)
        }
        coordinator?.completion = context
        return NSRange(location: context.range.lowerBound, length: context.range.count)
    }

    override func insertCompletion(_ word: String, forPartialWordRange charRange: NSRange, movement: Int, isFinal flag: Bool) {
        // While it previews and while it finishes, the text's changes start no other completion.
        completing = true
        defer { completing = !flag }
        guard flag, movement != NSTextMovement.cancel.rawValue, let context = coordinator?.completion else {
            super.insertCompletion(word, forPartialWordRange: charRange, movement: movement, isFinal: flag)
            return
        }
        // The name goes in as AppKit inserts it; then `}}` and, after `{{#`
        // or `{{^`, the matching close, as one more edit.
        super.insertCompletion(word, forPartialWordRange: charRange, movement: movement, isFinal: flag)
        coordinator?.completion = nil
        let end = selectedRange().location
        let start = end - (word as NSString).length
        guard start >= 0 else { return }
        let inserted = NoteTypeTemplateEditing.CompletionContext(range: start..<end, typed: word, section: context.section,
                                                                 afterFilter: context.afterFilter)
        coordinator?.perform(NoteTypeTemplateEditing.accept(word, context: inserted, in: string))
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if window?.firstResponder === self, modifiers == .command, event.charactersIgnoringModifiers == "/" {
            coordinator?.apply(.toggleComment)
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if modifiers == .control, event.charactersIgnoringModifiers == " " {
            showInsertMenu()
            return
        }
        super.keyDown(with: event)
    }

    override func setFrameSize(_ newSize: NSSize) {
        let widthChanged = newSize.width != frame.width
        super.setFrameSize(newSize)
        if widthChanged { coordinator?.relayoutSwatches() }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = super.menu(for: event) ?? NSMenu()
        if coordinator?.key?.part == .back, coordinator?.parent?.onFlip != nil, isEditable {
            let item = NSMenuItem(title: NoteTypesStrings.text("noteTypes.editor.flip"), action: #selector(flipCard), keyEquivalent: "")
            item.target = self
            menu.insertItem(item, at: 0)
            menu.insertItem(.separator(), at: 1)
        }
        return menu
    }

    @objc private func flipCard() { coordinator?.parent?.onFlip?() }

    /// ⌃Space: the Insert menu at the insertion point.
    private func showInsertMenu() {
        guard isEditable, let entries = coordinator?.parent?.insertMenu, !entries.isEmpty,
              let manager = layoutManager, let container = textContainer else { return }
        let menu = NSMenu()
        menuItems = []
        func add(_ entries: [InsertMenuEntry], to menu: NSMenu) {
            for entry in entries {
                switch entry {
                case .item(let title, let item, let enabled):
                    let menuItem = NSMenuItem(title: title, action: enabled ? #selector(insertItem(_:)) : nil, keyEquivalent: "")
                    menuItem.tag = menuItems.count
                    menuItems.append(item)
                    menuItem.target = self
                    menuItem.isEnabled = enabled
                    menu.addItem(menuItem)
                case .submenu(let title, let children):
                    let parent = NSMenuItem(title: title, action: nil, keyEquivalent: "")
                    let submenu = NSMenu(title: title)
                    add(children, to: submenu)
                    parent.submenu = submenu
                    menu.addItem(parent)
                }
            }
        }
        add(entries, to: menu)
        let caret = selectedRange().location
        let glyph = manager.glyphIndexForCharacter(at: min(caret, max((string as NSString).length - 1, 0)))
        let rect = manager.boundingRect(forGlyphRange: NSRange(location: glyph, length: (string as NSString).length == 0 ? 0 : 1), in: container)
        let point = NSPoint(x: textContainerOrigin.x + rect.minX, y: textContainerOrigin.y + rect.maxY)
        menu.popUp(positioning: nil, at: point, in: self)
    }

    /// The items of the menu ⌃Space showed, by tag.
    private var menuItems: [NoteTypeTemplateEditing.Item] = []

    @objc private func insertItem(_ sender: NSMenuItem) {
        guard menuItems.indices.contains(sender.tag) else { return }
        coordinator?.apply(.insert(menuItems[sender.tag]))
    }
}
#endif

#if os(iOS)
import UIKit

extension TemplateSyntaxColour {
    var colour: UIColor {
        switch self {
        case .field: .systemPurple
        case .section: .systemOrange
        case .comment: .secondaryLabel
        case .tag: .systemBlue
        case .directive: .systemTeal
        case .atRule: .systemPink
        case .property: .systemTeal
        }
    }
}

/// The template editor's text view on iPad (W6): the stored text byte for
/// byte, no autocorrection or smart punctuation, the system find (⌘F),
/// ⌘/ comments the line, colouring from the core, and a colour swatch on
/// each colour value of the styling.
struct TemplateCodeEditor: UIViewRepresentable {
    let key: TemplateEditorKey
    let text: String
    let editable: Bool
    let fields: [String]
    let request: NoteTypeEditorRequest?
    let insertMenu: [InsertMenuEntry]
    let accessibilityLabel: String
    let onChange: (String) -> Void
    let onConsume: (UInt64) -> Void
    let onFlip: (() -> Void)?
    let syntax: (String, Bool) async -> [NotetypeSyntaxSpan]

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> CodeTextView {
        let textView = CodeTextView(usingTextLayoutManager: false)
        textView.coordinator = context.coordinator
        textView.autocorrectionType = .no
        textView.autocapitalizationType = .none
        textView.spellCheckingType = .no
        textView.smartQuotesType = .no
        textView.smartDashesType = .no
        textView.smartInsertDeleteType = .no
        textView.inlinePredictionType = .no
        textView.isFindInteractionEnabled = true
        textView.font = Coordinator.font
        textView.adjustsFontForContentSizeCategory = true
        textView.typingAttributes = Coordinator.plain
        textView.backgroundColor = .clear
        textView.delegate = context.coordinator
        textView.accessibilityIdentifier = "noteTypes.cards.editor"
        context.coordinator.textView = textView
        context.coordinator.parent = self
        context.coordinator.show(key: key, text: text)
        return textView
    }

    func updateUIView(_ textView: CodeTextView, context: Context) {
        let coordinator = context.coordinator
        coordinator.parent = self
        textView.isEditable = editable
        textView.accessibilityLabel = accessibilityLabel
        if coordinator.key != key || textView.text != text {
            coordinator.show(key: key, text: text)
        }
        if let request, request.notetype == key.notetype, request.card == key.card, request.part == key.part,
           coordinator.applied != request.token {
            coordinator.applied = request.token
            coordinator.apply(request.command)
            let token = request.token
            DispatchQueue.main.async { onConsume(token) }
        }
    }

    @MainActor final class Coordinator: NSObject, UITextViewDelegate {
        static var font: UIFont {
            let body = UIFont.preferredFont(forTextStyle: .body)
            return body.fontDescriptor.withDesign(.monospaced).map { UIFont(descriptor: $0, size: 0) } ?? body
        }
        static var plain: [NSAttributedString.Key: Any] { [.font: font, .foregroundColor: UIColor.label] }
        static let well: CGFloat = 22

        weak var textView: CodeTextView?
        var parent: TemplateCodeEditor?
        var key: TemplateEditorKey?
        var applied: UInt64?
        private var selections: [TemplateEditorKey: NSRange] = [:]
        private var generation: UInt64 = 0
        private var colours: [NSRange] = []
        private var swatches: [UIColorWell] = []

        func show(key newKey: TemplateEditorKey, text: String) {
            guard let textView else { return }
            if let key, key != newKey { selections[key] = textView.selectedRange }
            let sameKey = key == newKey
            key = newKey
            if textView.text != text {
                textView.text = text
                // Another text, or this one changed elsewhere: its history no longer applies.
                textView.undoManager?.removeAllActions()
            }
            let length = (text as NSString).length
            let selection = sameKey ? textView.selectedRange : (selections[newKey] ?? NSRange(location: 0, length: 0))
            textView.selectedRange = NSRange(location: min(selection.location, length), length: 0)
            recolour()
        }

        func textViewDidChange(_ textView: UITextView) {
            parent?.onChange(textView.text)
            recolour()
        }

        func textView(_ textView: UITextView, editMenuForTextIn range: NSRange, suggestedActions: [UIMenuElement]) -> UIMenu? {
            guard key?.part == .back, let flip = parent?.onFlip, textView.isEditable else { return nil }
            let action = UIAction(title: NoteTypesStrings.text("noteTypes.editor.flip")) { _ in flip() }
            return UIMenu(children: suggestedActions + [action])
        }

        func apply(_ command: NoteTypeEditorRequest.Command) {
            guard let textView else { return }
            let text = textView.text ?? ""
            let selection = textView.selectedRange
            let range = selection.location..<(selection.location + selection.length)
            let edit: NoteTypeTextEdit
            switch command {
            case .insert(let item): edit = NoteTypeTemplateEditing.insert(item, in: text, selection: range)
            case .toggleComment: edit = NoteTypeTemplateEditing.toggleComment(text, selection: range, styling: key?.part == .styling)
            case .replaceAll(let replacement):
                let length = (text as NSString).length
                edit = NoteTypeTextEdit(range: 0..<length, text: replacement, caret: min(selection.location, (replacement as NSString).length))
            case .append(let suffix):
                let length = (text as NSString).length
                edit = NoteTypeTextEdit(range: length..<length, text: suffix, caret: length + (suffix as NSString).length)
            case .moveCaret(let line):
                let offset = NoteTypeTemplateEditing.offset(ofLine: line, in: text)
                textView.becomeFirstResponder()
                textView.selectedRange = NSRange(location: offset, length: 0)
                textView.scrollRangeToVisible(NSRange(location: offset, length: 0))
                return
            }
            perform(edit)
        }

        /// An edit through the text input, so the text view's undo records it.
        func perform(_ edit: NoteTypeTextEdit) {
            guard let textView, textView.isEditable,
                  let start = textView.position(from: textView.beginningOfDocument, offset: edit.range.lowerBound),
                  let end = textView.position(from: textView.beginningOfDocument, offset: edit.range.upperBound),
                  let range = textView.textRange(from: start, to: end) else { return }
            textView.replace(range, withText: edit.text)
            textView.selectedRange = NSRange(location: edit.caret, length: 0)
            textView.scrollRangeToVisible(textView.selectedRange)
        }

        func recolour() {
            guard let textView, let parent else { return }
            generation &+= 1
            let generation = generation
            let text = textView.text ?? ""
            let styling = key?.part == .styling
            Task { @MainActor [weak self] in
                let spans = await parent.syntax(text, styling)
                guard let self, self.generation == generation, let textView = self.textView, textView.text == text,
                      textView.markedTextRange == nil else { return }
                self.colour(spans)
            }
        }

        private func colour(_ spans: [NotetypeSyntaxSpan]) {
            guard let textView else { return }
            let storage = textView.textStorage
            let length = storage.length
            let selection = textView.selectedRange
            storage.beginEditing()
            storage.setAttributes(Self.plain, range: NSRange(location: 0, length: length))
            var colours: [(NSRange, CardTextColour)] = []
            for span in spans where span.start < span.end && span.end <= length {
                let range = NSRange(location: span.start, length: span.end - span.start)
                if let colour = TemplateSyntaxColour(span.kind) {
                    storage.addAttribute(.foregroundColor, value: colour.colour, range: range)
                } else if case .colour(let value) = span.kind, span.start > 0 {
                    storage.addAttribute(.kern, value: Self.well + Spacing.xxSmall, range: NSRange(location: span.start - 1, length: 1))
                    colours.append((range, value))
                }
            }
            storage.endEditing()
            textView.selectedRange = selection
            textView.typingAttributes = Self.plain
            self.colours = colours.map(\.0)
            layoutSwatches(colours)
        }

        func layoutSwatches() { if !colours.isEmpty { recolour() } }

        private func layoutSwatches(_ colours: [(NSRange, CardTextColour)]) {
            guard let textView else { return }
            let manager = textView.layoutManager
            manager.ensureLayout(for: textView.textContainer)
            while swatches.count < colours.count {
                let well = UIColorWell()
                well.supportsAlpha = true
                well.addTarget(self, action: #selector(wellChanged(_:)), for: .valueChanged)
                textView.addSubview(well)
                swatches.append(well)
            }
            for (index, well) in swatches.enumerated() {
                guard index < colours.count else { well.isHidden = true; continue }
                let (range, value) = colours[index]
                let glyphs = manager.glyphRange(forCharacterRange: NSRange(location: range.location, length: 1), actualCharacterRange: nil)
                let rect = manager.boundingRect(forGlyphRange: glyphs, in: textView.textContainer)
                let inset = textView.textContainerInset
                well.frame = CGRect(x: inset.left + rect.minX - Self.well - Spacing.xxSmall / 2, y: inset.top + rect.midY - Self.well / 2,
                                    width: Self.well, height: Self.well)
                well.selectedColor = .swatch(value)
                well.tag = index
                well.isEnabled = textView.isEditable
                well.isHidden = false
                well.accessibilityIdentifier = "noteTypes.cards.swatch.\(index)"
            }
        }

        @objc private func wellChanged(_ well: UIColorWell) {
            guard colours.indices.contains(well.tag), let colour = well.selectedColor else { return }
            var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
            guard colour.getRed(&red, green: &green, blue: &blue, alpha: &alpha) else { return }
            let range = colours[well.tag]
            let hex = SwatchHex.string(red: red, green: green, blue: blue, alpha: alpha)
            colours[well.tag] = NSRange(location: range.location, length: (hex as NSString).length)
            perform(NoteTypeTextEdit(range: range.location..<(range.location + range.length), text: hex,
                                     caret: range.location + (hex as NSString).length))
        }
    }
}

/// The editor's text view on iPad: ⌘/ comments the line.
final class CodeTextView: UITextView {
    weak var coordinator: TemplateCodeEditor.Coordinator?

    override var keyCommands: [UIKeyCommand]? {
        let comment = UIKeyCommand(title: NoteTypesStrings.text("noteTypes.editor.toggleComment"), action: #selector(toggleComment),
                                   input: "/", modifierFlags: .command)
        return (super.keyCommands ?? []) + [comment]
    }

    @objc private func toggleComment() { coordinator?.apply(.toggleComment) }

    override func layoutSubviews() {
        let width = bounds.width
        super.layoutSubviews()
        if width != laidOutWidth {
            laidOutWidth = width
            coordinator?.layoutSwatches()
        }
    }

    private var laidOutWidth: CGFloat = 0
}
#endif
