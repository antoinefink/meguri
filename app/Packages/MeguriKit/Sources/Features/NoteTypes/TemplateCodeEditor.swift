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

    /// The text's own undo history: one per front and back, and one for
    /// the styling every card type shares.
    var history: TemplateEditorKey { part == .styling ? TemplateEditorKey(notetype: notetype, card: 0, part: .styling) : self }
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

/// Where a colour swatch sits on its line (W6): centred on the capitals and
/// inside the line's ascender and descender, so at any text size it stays on
/// its own line and touches neither the line above nor the line below.
nonisolated enum SwatchMetrics {
    /// The swatch's side and the height of its centre above the baseline.
    static func place(capHeight: CGFloat, ascender: CGFloat, descender: CGFloat) -> (side: CGFloat, centre: CGFloat) {
        let centre = capHeight / 2
        // The room from the centre up to the ascender and down to the descender (negative).
        let room = min(ascender - centre, centre - descender)
        // A point clear of each, and no larger than the capitals call for.
        let side = min(capHeight * 1.4, 2 * (room - 1)).rounded(.down)
        return (max(side, 1), centre)
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
    /// A15's Flip for the shown card type; nil for a cloze kind.
    let onFlip: (() -> NoteTypeFlip?)?
    /// Writes the shown card type's front while it still reads `expected`.
    let onReplaceFront: (_ expected: String, _ text: String) -> Bool
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
        /// A colour swatch's side and where it sits on its line.
        static let swatch = SwatchMetrics.place(capHeight: font.capHeight, ascender: font.ascender, descender: font.descender)

        weak var textView: CodeTextView?
        var parent: TemplateCodeEditor?
        var key: TemplateEditorKey?
        var applied: UInt64?
        private var undoManagers: [TemplateEditorKey: UndoManager] = [:]
        /// The text each history was recorded on, as it last stood.
        private var historyText: [TemplateEditorKey: String] = [:]
        private var selections: [TemplateEditorKey: NSRange] = [:]
        private var generation: UInt64 = 0
        /// The colour values of the shown styling, as last coloured.
        private var colours: [NSRange] = []
        private var swatches: [ColourSwatch] = []
        /// A swatch is writing its value: its own range moves with it.
        private var applyingSwatch = false
        /// The completion the text view is showing.
        var completion: NoteTypeTemplateEditing.CompletionContext?

        /// Shows another text, or the same one changed elsewhere (Revert,
        /// Flip): the text view's history for it no longer applies.
        func show(key newKey: TemplateEditorKey, text: String) {
            guard let textView else { return }
            if let key, key != newKey { selections[key.history] = textView.selectedRange() }
            // The swatches' ranges belong to the text shown before: they wait for its colouring.
            if key != newKey || textView.string != text { swatches.forEach { $0.isEnabled = false } }
            let sameKey = key?.history == newKey.history
            key = newKey
            // A text changed while elsewhere (Flip, Revert, a save that renumbered
            // card types): its history was recorded on other characters.
            if historyText[newKey.history] != text { undoManagers[newKey.history]?.removeAllActions() }
            historyText[newKey.history] = text
            if textView.string != text {
                textView.string = text
            }
            let length = (text as NSString).length
            let selection = sameKey ? textView.selectedRange() : (selections[newKey.history] ?? NSRange(location: 0, length: 0))
            textView.setSelectedRange(NSRange(location: min(selection.location, length), length: 0))
            recolour()
        }

        func undoManager(for view: NSTextView) -> UndoManager? {
            guard let key = key?.history else { return nil }
            if let manager = undoManagers[key] { return manager }
            let manager = UndoManager()
            undoManagers[key] = manager
            return manager
        }

        func textDidChange(_ notification: Notification) {
            guard let textView else { return }
            if let key { historyText[key.history] = textView.string }
            // The swatches' ranges are the last colouring's until the next lands.
            if !applyingSwatch { swatches.forEach { $0.isEnabled = false } }
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
            // Nothing is preselected: typing on goes on as typed.
            index?.pointee = -1
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
                    storage.addAttribute(.kern, value: Self.swatch.side + Spacing.xSmall, range: NSRange(location: span.start - 1, length: 1))
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
                let well = ColourSwatch()
                well.target = self
                well.action = #selector(wellChanged(_:))
                textView.addSubview(well)
                swatches.append(well)
            }
            for (index, well) in swatches.enumerated() {
                guard index < colours.count else { well.isHidden = true; continue }
                let (range, value) = colours[index]
                let glyphs = manager.glyphRange(forCharacterRange: NSRange(location: range.location, length: 1), actualCharacterRange: nil)
                let rect = manager.boundingRect(forGlyphRange: glyphs, in: container)
                // On the value's baseline, centred on its capitals (flipped: y grows down).
                let baseline = manager.lineFragmentRect(forGlyphAt: glyphs.location, effectiveRange: nil).minY
                    + manager.location(forGlyphAt: glyphs.location).y
                let origin = textView.textContainerOrigin, side = Self.swatch.side
                well.frame = NSRect(x: origin.x + rect.minX - side - Spacing.xSmall / 2,
                                    y: origin.y + baseline - Self.swatch.centre - side / 2, width: side, height: side)
                well.color = .swatch(value)
                well.tag = index
                well.isEnabled = textView.isEditable
                well.isHidden = false
                well.setAccessibilityIdentifier("noteTypes.cards.swatch.\(index)")
            }
        }

        /// A colour chosen in a well rewrites only that value's characters.
        @objc private func wellChanged(_ well: ColourSwatch) {
            guard well.isEnabled, colours.indices.contains(well.tag), let colour = well.color.usingColorSpace(.sRGB) else { return }
            // The others' ranges move with this edit: they wait for the next colouring.
            for other in swatches where other !== well { other.isEnabled = false }
            applyingSwatch = true
            defer { applyingSwatch = false }
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

        /// A15's Flip from the Back's context menu, as one step of the Back's
        /// own undo: Undo puts back both texts, Redo flips them again.
        func flip() {
            guard let textView, textView.isEditable, key?.part == .back, let flip = parent?.onFlip?(),
                  let manager = undoManager(for: textView) else { return }
            textView.breakUndoCoalescing()
            manager.beginUndoGrouping()
            replaceFront(flip.previousFront, with: flip.front, manager: manager)
            let length = (textView.string as NSString).length
            perform(NoteTypeTextEdit(range: 0..<length, text: flip.back,
                                     caret: min(textView.selectedRange().location, (flip.back as NSString).length)))
            manager.endUndoGrouping()
            manager.setActionName(NoteTypesStrings.text("noteTypes.editor.flip"))
            textView.breakUndoCoalescing()
        }

        /// Flip's front, which is not shown, and its undo. A front edited
        /// since is kept: the undo then puts back only the Back.
        private func replaceFront(_ expected: String, with text: String, manager: UndoManager) {
            guard parent?.onReplaceFront(expected, text) == true else { return }
            manager.registerUndo(withTarget: self) { [weak manager] coordinator in
                MainActor.assumeIsolated {
                    guard let manager else { return }
                    coordinator.replaceFront(text, with: expected, manager: manager)
                }
            }
        }
    }
}

/// W6's colour swatch on the Mac: the value's colour in a circle drawn
/// within its frame, which is the size of the line's capitals; a click opens
/// the system colour panel on it. Not `NSColorWell`: every style of the
/// system well draws larger than a line of code (checked in the test VM,
/// 2026-11-22) and so covered the lines above and below.
final class ColourSwatch: NSControl {
    var color: NSColor = .clear { didSet { needsDisplay = true } }
    /// The swatch the colour panel sends its changes to, while one is.
    private static weak var bound: ColourSwatch?

    /// A swatch disabled (its value moved or went) lets the panel go: a later
    /// change never lands on the value it is given next.
    override var isEnabled: Bool {
        didSet {
            guard !isEnabled, Self.bound === self else { return }
            NSColorPanel.shared.setTarget(nil)
            NSColorPanel.shared.setAction(nil)
            Self.bound = nil
        }
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.colorWell)
    }

    required init?(coder: NSCoder) { nil }

    override func draw(_ dirtyRect: NSRect) {
        let path = NSBezierPath(ovalIn: bounds.insetBy(dx: 0.5, dy: 0.5))
        color.setFill()
        path.fill()
        // A hairline keeps a colour close to the background visible.
        NSColor.separatorColor.setStroke()
        path.lineWidth = 1
        path.stroke()
    }

    override func mouseDown(with event: NSEvent) { showPanel() }

    override func accessibilityPerformPress() -> Bool {
        showPanel()
        return true
    }

    override func accessibilityValue() -> Any? {
        guard let rgb = color.usingColorSpace(.sRGB) else { return nil }
        return SwatchHex.string(red: rgb.redComponent, green: rgb.greenComponent, blue: rgb.blueComponent, alpha: rgb.alphaComponent)
    }

    private func showPanel() {
        guard isEnabled else { return }
        let panel = NSColorPanel.shared
        panel.setTarget(nil)
        panel.showsAlpha = true
        panel.color = color
        panel.setTarget(self)
        panel.setAction(#selector(panelChanged(_:)))
        Self.bound = self
        panel.orderFront(nil)
    }

    @objc private func panelChanged(_ panel: NSColorPanel) {
        guard isEnabled, Self.bound === self else { return }
        color = panel.color
        sendAction(action, to: target)
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
        // `}}` and the close follow only a name chosen with Return or Tab.
        let chosen = movement == NSTextMovement.return.rawValue || movement == NSTextMovement.tab.rawValue
        guard flag, chosen, let context = coordinator?.completion else {
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

    @objc private func flipCard() { coordinator?.flip() }

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
    /// A15's Flip for the shown card type; nil for a cloze kind.
    let onFlip: (() -> NoteTypeFlip?)?
    /// Writes the shown card type's front while it still reads `expected`.
    let onReplaceFront: (_ expected: String, _ text: String) -> Bool
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
        /// A colour swatch's side and where it sits on its line, at the text's size.
        static var swatch: (side: CGFloat, centre: CGFloat) {
            SwatchMetrics.place(capHeight: font.capHeight, ascender: font.ascender, descender: font.descender)
        }

        weak var textView: CodeTextView?
        var parent: TemplateCodeEditor?
        var key: TemplateEditorKey?
        var applied: UInt64?
        private var selections: [TemplateEditorKey: NSRange] = [:]
        private var undoManagers: [TemplateEditorKey: UndoManager] = [:]
        private var historyText: [TemplateEditorKey: String] = [:]
        private var generation: UInt64 = 0
        private var colours: [NSRange] = []
        private var swatches: [UIColorWell] = []
        private var applyingSwatch = false

        func show(key newKey: TemplateEditorKey, text: String) {
            guard let textView else { return }
            if let key, key != newKey { selections[key.history] = textView.selectedRange }
            if key != newKey || textView.text != text { swatches.forEach { $0.isEnabled = false } }
            let sameKey = key?.history == newKey.history
            key = newKey
            // Each text keeps its own history; one changed while elsewhere loses it.
            let manager = undoManagers[newKey.history] ?? UndoManager()
            undoManagers[newKey.history] = manager
            if historyText[newKey.history] != text { manager.removeAllActions() }
            historyText[newKey.history] = text
            textView.history = manager
            if textView.text != text {
                textView.text = text
            }
            let length = (text as NSString).length
            let selection = sameKey ? textView.selectedRange : (selections[newKey.history] ?? NSRange(location: 0, length: 0))
            textView.selectedRange = NSRange(location: min(selection.location, length), length: 0)
            recolour()
        }

        func textViewDidChange(_ textView: UITextView) { publish(textView) }

        /// The text as it now stands goes to the draft. Typing reaches it through
        /// the delegate; an edit made in code (a fix, Insert, Flip) through `perform`.
        private func publish(_ textView: UITextView) {
            if let key { historyText[key.history] = textView.text }
            if !applyingSwatch { swatches.forEach { $0.isEnabled = false } }
            parent?.onChange(textView.text)
            recolour()
        }

        func textView(_ textView: UITextView, editMenuForTextIn range: NSRange, suggestedActions: [UIMenuElement]) -> UIMenu? {
            guard key?.part == .back, parent?.onFlip != nil, textView.isEditable else { return nil }
            let action = UIAction(title: NoteTypesStrings.text("noteTypes.editor.flip")) { [weak self] _ in self?.flip() }
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
            publish(textView)
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
                    storage.addAttribute(.kern, value: Self.swatch.side + Spacing.xSmall, range: NSRange(location: span.start - 1, length: 1))
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
                // On the value's baseline, centred on its capitals.
                let baseline = manager.lineFragmentRect(forGlyphAt: glyphs.location, effectiveRange: nil).minY
                    + manager.location(forGlyphAt: glyphs.location).y
                let inset = textView.textContainerInset, place = Self.swatch
                // The system well draws at its own size, larger than a line of code:
                // it keeps that size and is scaled into the side, on its line.
                well.transform = .identity
                let natural = well.intrinsicContentSize
                let drawn = max(natural.width, natural.height, place.side)
                well.bounds = CGRect(x: 0, y: 0, width: drawn, height: drawn)
                well.transform = CGAffineTransform(scaleX: place.side / drawn, y: place.side / drawn)
                well.center = CGPoint(x: inset.left + rect.minX - Spacing.xSmall / 2 - place.side / 2, y: inset.top + baseline - place.centre)
                well.selectedColor = .swatch(value)
                well.tag = index
                well.isEnabled = textView.isEditable
                well.isHidden = false
                // One element per swatch: the well's own parts stay out of the tree.
                well.layoutIfNeeded()
                for part in well.subviews {
                    part.isAccessibilityElement = false
                    part.accessibilityElementsHidden = true
                }
                well.accessibilityElements = []
                well.isAccessibilityElement = true
                well.accessibilityTraits = .button
                well.accessibilityLabel = SwatchHex.string(red: Double(value.red) / 255, green: Double(value.green) / 255,
                                                           blue: Double(value.blue) / 255, alpha: Double(value.alpha) / 255)
                well.accessibilityIdentifier = "noteTypes.cards.swatch.\(index)"
            }
        }

        /// A15's Flip from the Back's edit menu, as one step of the Back's own
        /// undo: Undo puts back both texts, Redo flips them again.
        func flip() {
            guard let textView, textView.isEditable, key?.part == .back, let flip = parent?.onFlip?(),
                  let manager = textView.undoManager else { return }
            // Both halves are this coordinator's own undo actions, in one group: the
            // Back is not written through the text input, so UIKit's typing undo,
            // which coalesces consecutive typing, cannot fold the last typing into it.
            manager.beginUndoGrouping()
            replaceFront(flip.previousFront, with: flip.front, manager: manager)
            replaceBack(with: flip.back, manager: manager)
            manager.endUndoGrouping()
            manager.setActionName(NoteTypesStrings.text("noteTypes.editor.flip"))
        }

        /// Flip's Back, written straight into the text storage, and its undo.
        private func replaceBack(with text: String, manager: UndoManager) {
            guard let textView, key?.part == .back else { return }
            let before = textView.text ?? ""
            let caret = min(textView.selectedRange.location, (text as NSString).length)
            textView.textStorage.replaceCharacters(in: NSRange(location: 0, length: textView.textStorage.length),
                                                   with: NSAttributedString(string: text, attributes: Self.plain))
            textView.selectedRange = NSRange(location: caret, length: 0)
            publish(textView)
            manager.registerUndo(withTarget: self) { [weak manager] coordinator in
                MainActor.assumeIsolated {
                    guard let manager else { return }
                    coordinator.replaceBack(with: before, manager: manager)
                }
            }
        }

        /// Flip's front, which is not shown, and its undo. A front edited
        /// since is kept: the undo then puts back only the Back.
        private func replaceFront(_ expected: String, with text: String, manager: UndoManager) {
            guard parent?.onReplaceFront(expected, text) == true else { return }
            manager.registerUndo(withTarget: self) { [weak manager] coordinator in
                MainActor.assumeIsolated {
                    guard let manager else { return }
                    coordinator.replaceFront(text, with: expected, manager: manager)
                }
            }
        }

        @objc private func wellChanged(_ well: UIColorWell) {
            guard well.isEnabled, colours.indices.contains(well.tag), let colour = well.selectedColor else { return }
            for other in swatches where other !== well { other.isEnabled = false }
            applyingSwatch = true
            defer { applyingSwatch = false }
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
    /// The shown text's own history (one per front, back and styling).
    var history: UndoManager?
    override var undoManager: UndoManager? { history ?? super.undoManager }

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
